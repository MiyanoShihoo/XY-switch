import assert from 'node:assert/strict';
import { spawn, execFileSync } from 'node:child_process';
import { createServer } from 'node:http';
import { access, mkdir, mkdtemp, readFile, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { setTimeout as delay } from 'node:timers/promises';

assert.equal(process.platform, 'linux', 'Website CI requires Linux');
assert.equal(process.env.GITHUB_ACTIONS, 'true', 'Website CI requires GitHub Actions');
assert.equal(process.env.RUNNER_ENVIRONMENT, 'github-hosted', 'Website CI requires a disposable runner');
const root = fileURLToPath(new URL('../', import.meta.url));
const output = resolve(process.env.WEBSITE_QA_OUTPUT ?? join(root, 'artifacts/website-ui-qa'));
const executable = process.env.CHROME_BIN ?? '/usr/bin/google-chrome';
await access(executable);
const cjkFont = execFileSync('fc-match', ['--format=%{family}', 'sans-serif:lang=zh-cn'], { encoding: 'utf8' }).trim();
assert(/\bCJK\b/.test(cjkFont), 'Install fonts-noto-cjk before capturing the Chinese website');
await mkdir(output, { recursive: true });
const profile = await mkdtemp(join(tmpdir(), 'xy-desktop-website-'));
const report = {
  sourceCommit: execFileSync('git', ['rev-parse', 'HEAD'], { cwd: root, encoding: 'utf8' }).trim(),
  runner: process.env.RUNNER_ENVIRONMENT,
  cjkFont,
  profile,
  packagedApplicationExecuted: false,
  passed: false,
};
const files = new Map([
  ['/website/', ['website/index.html', 'text/html; charset=utf-8']],
  ['/website/index.html', ['website/index.html', 'text/html; charset=utf-8']],
  ['/public/app-icon.png', ['public/app-icon.png', 'image/png']],
]);
const server = createServer(async (request, response) => {
  const selected = files.get(new URL(request.url, 'http://127.0.0.1').pathname);
  if (!selected || !['GET', 'HEAD'].includes(request.method)) {
    response.writeHead(404); response.end(); return;
  }
  try {
    const content = await readFile(join(root, selected[0]));
    response.writeHead(200, { 'Content-Type': selected[1], 'Content-Length': content.length });
    response.end(request.method === 'HEAD' ? undefined : content);
  } catch { response.writeHead(500); response.end(); }
});
let browser, browserClosed, startupError, endpoint, version;
let browserLog = '', testLog = '';
try {
  await new Promise((resolve, reject) => {
    server.once('error', reject);
    server.listen(0, '127.0.0.1', resolve);
  });
  report.appUrl = 'http://127.0.0.1:' + server.address().port;
  browser = spawn(executable, [
    '--headless=new', '--remote-debugging-port=0', '--remote-debugging-address=127.0.0.1',
    '--user-data-dir=' + profile, '--no-first-run', '--no-default-browser-check',
    '--disable-background-networking', '--disable-component-update', '--disable-sync',
    '--disable-extensions', '--disable-crash-reporter', 'about:blank',
  ], { stdio: ['ignore', 'ignore', 'pipe'] });
  browser.on('error', error => { startupError = error; });
  browserClosed = new Promise(resolve => browser.once('close', resolve));
  browser.stderr.on('data', data => { browserLog = (browserLog + data).slice(-100000); });
  for (let i = 0; i < 120; i++) {
    if (startupError) throw startupError;
    assert(browser.exitCode === null && browser.signalCode === null, 'Dedicated browser exited early');
    const activePort = await readFile(join(profile, 'DevToolsActivePort'), 'utf8').catch(() => '');
    if (/^\d+\r?\n/.test(activePort)) {
      endpoint = 'http://127.0.0.1:' + activePort.split(/\r?\n/)[0]; break;
    }
    await delay(250);
  }
  assert(endpoint, 'Isolated browser CDP did not start');
  version = await fetch(endpoint + '/json/version', { signal: AbortSignal.timeout(5000) }).then(r => r.json());
  report.browser = version.Browser;
  const child = spawn(process.execPath, ['scripts/verify-website-ui.mjs'], {
    cwd: root,
    env: { ...process.env, APP_URL: report.appUrl, CDP_ENDPOINT: endpoint, WEBSITE_QA_OUTPUT: output, WEBSITE_QA_EXPECT_CJK: 'true' },
    stdio: ['ignore', 'pipe', 'pipe'],
  });
  child.stdout.on('data', data => { testLog += data; process.stdout.write(data); });
  child.stderr.on('data', data => { testLog += data; process.stderr.write(data); });
  const code = await new Promise((resolve, reject) => {
    const timer = setTimeout(() => { child.kill(); reject(new Error('Website verification timed out')); }, 180000);
    child.once('error', error => { clearTimeout(timer); reject(error); });
    child.once('close', code => { clearTimeout(timer); resolve(code); });
  });
  assert.equal(code, 0, 'Website interaction acceptance failed');
  report.passed = true;
} catch (error) {
  report.error = String(error);
  throw error;
} finally {
  if (version?.webSocketDebuggerUrl && browser?.exitCode === null && browser?.signalCode === null) {
    let socket;
    try {
      assert.equal(new URL(version.webSocketDebuggerUrl).hostname, '127.0.0.1');
      socket = new WebSocket(version.webSocketDebuggerUrl);
      await new Promise((resolve, reject) => {
        const timer = setTimeout(() => reject(new Error('Browser close timed out')), 3000);
        const done = () => { clearTimeout(timer); resolve(); };
        socket.addEventListener('open', () => socket.send(JSON.stringify({ id: 1, method: 'Browser.close' })), { once: true });
        socket.addEventListener('close', done, { once: true });
        socket.addEventListener('error', done, { once: true });
      });
    } catch { /* Only the dedicated disposable browser is terminated below. */ }
    finally { socket?.close(); }
  }
  if (browser) {
    await Promise.race([browserClosed, delay(2000)]);
    if (browser.exitCode === null && browser.signalCode === null) browser.kill('SIGTERM');
    await Promise.race([browserClosed, delay(2000)]);
    if (browser.exitCode === null && browser.signalCode === null) browser.kill('SIGKILL');
    await Promise.race([browserClosed, delay(2000)]);
    report.browserClosed = browser.exitCode !== null || browser.signalCode !== null || Boolean(startupError);
  }
  if (browser && !report.browserClosed) {
    report.cleanupError = 'Dedicated browser did not exit';
    report.passed = false;
  }
  server.closeAllConnections();
  await new Promise(resolve => server.close(resolve));
  await writeFile(join(output, 'test.log'), testLog);
  await writeFile(join(output, 'browser.log'), browserLog);
  await writeFile(join(output, 'environment.json'), JSON.stringify(report, null, 2));
  console.log(JSON.stringify({ output, passed: report.passed, browserClosed: report.browserClosed }));
  if (report.cleanupError && !report.error) throw new Error(report.cleanupError);
}
