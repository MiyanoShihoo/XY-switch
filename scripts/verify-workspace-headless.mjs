import assert from 'node:assert/strict';
import { spawn, execFileSync } from 'node:child_process';
import { access, mkdir, mkdtemp, writeFile } from 'node:fs/promises';
import { isAbsolute, join, relative, resolve, sep } from 'node:path';
import { fileURLToPath } from 'node:url';
import { setTimeout as delay } from 'node:timers/promises';
import { createServer } from 'vite';
import { waitForDevTools } from './workspace-qa/devtools-readiness.mjs';

const root = fileURLToPath(new URL('../', import.meta.url));
const artifacts = resolve(root, 'artifacts');
const output = resolve(process.env.QA_OUTPUT ?? join(artifacts, 'workspace-headless-qa'));
const outputRelative = relative(artifacts, output);
assert(outputRelative && outputRelative !== '..' && !outputRelative.startsWith('..' + sep) && !isAbsolute(outputRelative), 'QA output must stay inside this repository artifacts directory');
const suites = new Map([
  ['navigation', 'verify-workspace-navigation-ui.mjs'],
  ['workspace', 'verify-workspace-ui.mjs'],
  ['i18n', 'verify-i18n-ui.mjs'],
  ['sessions', 'verify-session-favorites-ui.mjs'],
  ['governance', 'verify-governance-ui.mjs'],
  ['sync', 'verify-encrypted-sync-ui.mjs'],
]);
const args = process.argv.slice(2);
assert(args.length === 0 || (args.length === 2 && args[0] === '--suites'), 'Usage: node scripts/verify-workspace-headless.mjs [--suites navigation,workspace,i18n,sessions,governance,sync]');
const selected = args.length ? args[1].split(',') : [...suites.keys()];
assert(selected.length && new Set(selected).size === selected.length && selected.every(name => suites.has(name)), 'Unknown or repeated suite');
assert(!process.env.QA_ONLY, 'Headless acceptance must run complete selected suites');
const candidates = process.env.CHROME_BIN ? [process.env.CHROME_BIN] : process.platform === 'win32' ? [
  'C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe',
  'C:/Program Files/Google/Chrome/Application/chrome.exe',
] : process.platform === 'darwin' ? ['/Applications/Google Chrome.app/Contents/MacOS/Google Chrome'] : [
  '/usr/bin/google-chrome', '/usr/bin/google-chrome-stable', '/usr/bin/chromium', '/usr/bin/chromium-browser',
];
let executable;
for (const candidate of candidates) {
  assert(isAbsolute(candidate), 'Browser executable must be an absolute path');
  if (await access(candidate).then(() => true, () => false)) { executable = candidate; break; }
}
assert(executable, 'Set CHROME_BIN to an existing Chromium-based browser; this wrapper never installs a browser');
await mkdir(output, { recursive: true });
const profile = await mkdtemp(join(output, 'browser-profile-'));
const report = {
  sourceCommit: execFileSync('git', ['rev-parse', 'HEAD'], { cwd: root, encoding: 'utf8' }).trim(),
  platform: process.platform,
  runner: process.env.RUNNER_ENVIRONMENT ?? 'local',
  executable, profile, suites: [],
  networkPolicy: 'Exact renderer HTTP origin and its matching WebSocket origin; no external requests',
  packagedApplicationExecuted: false, realProviderRequests: false, passed: false,
};
if (process.platform === 'linux') {
  report.cjkFont = execFileSync('fc-match', ['--format=%{family}', 'sans-serif:lang=zh-cn'], { encoding: 'utf8' }).trim();
  assert(/CJK/.test(report.cjkFont), 'Install fonts-noto-cjk before Chinese renderer acceptance');
}
let server, browser, browserClosed, startupError, version;
let browserLog = '';
const runningTests = new Set();
async function closeBrowser() {
  if (version?.webSocketDebuggerUrl && browser?.exitCode === null && browser.signalCode === null) {
    let socket;
    try {
      assert.equal(new URL(version.webSocketDebuggerUrl).hostname, '127.0.0.1');
      socket = new WebSocket(version.webSocketDebuggerUrl);
      await new Promise((resolve, reject) => {
        const timer = setTimeout(() => reject(new Error('Browser.close timed out')), 3000);
        const done = () => { clearTimeout(timer); resolve(); };
        socket.addEventListener('open', () => socket.send(JSON.stringify({ id: 1, method: 'Browser.close' })), { once: true });
        socket.addEventListener('close', done, { once: true });
        socket.addEventListener('error', done, { once: true });
      });
    } catch { /* Only the browser created with this disposable profile is terminated below. */ }
    finally { socket?.close(); }
  }
  if (!browser) return;
  await Promise.race([browserClosed, delay(2000)]);
  if (browser.exitCode === null && browser.signalCode === null) browser.kill('SIGTERM');
  await Promise.race([browserClosed, delay(2000)]);
  if (browser.exitCode === null && browser.signalCode === null) browser.kill('SIGKILL');
  await Promise.race([browserClosed, delay(2000)]);
  report.browserClosed = browser.exitCode !== null || browser.signalCode !== null || Boolean(startupError);
}
async function runSuite(name, endpoint) {
  const suiteOutput = join(output, name);
  await mkdir(suiteOutput, { recursive: true });
  let log = '';
  const child = spawn(process.execPath, [join(root, 'scripts', suites.get(name))], {
    cwd: root, windowsHide: true,
    env: { ...process.env, APP_URL: report.appUrl, CDP_ENDPOINT: endpoint, QA_OUTPUT: suiteOutput, I18N_QA_OUTPUT: suiteOutput },
    stdio: ['ignore', 'pipe', 'pipe'],
  });
  runningTests.add(child);
  child.stdout.on('data', data => { log += data; process.stdout.write(data); });
  child.stderr.on('data', data => { log += data; process.stderr.write(data); });
  let code;
  try {
    code = await new Promise((resolve, reject) => {
      const timer = setTimeout(() => { child.kill(); reject(new Error(name + ' verification timed out')); }, 300000);
      child.once('error', error => { clearTimeout(timer); reject(error); });
      child.once('close', code => { clearTimeout(timer); resolve(code); });
    });
  } finally {
    runningTests.delete(child);
    await writeFile(join(suiteOutput, 'verification.log'), log);
    report.suites.push({ name, exitCode: code ?? null, passed: code === 0, output: suiteOutput });
  }
  assert.equal(code, 0, name + ' renderer acceptance failed');
}
try {
  report.stage = 'renderer-server';
  server = await createServer({ root, clearScreen: false, optimizeDeps: { entries: ['index.html'] }, server: { host: '127.0.0.1', port: 0, strictPort: true, open: false } });
  await server.listen();
  const address = server.httpServer.address();
  assert.equal(address.address, '127.0.0.1', 'Renderer must bind only the IPv4 loopback interface');
  report.appUrl = 'http://127.0.0.1:' + address.port;
  assert((await fetch(report.appUrl, { signal: AbortSignal.timeout(10000) })).ok, 'Renderer server did not respond');
  report.stage = 'browser-startup';
  browser = spawn(executable, [
    '--headless=new', '--remote-debugging-port=0', '--remote-debugging-address=127.0.0.1',
    '--user-data-dir=' + profile, '--no-first-run', '--no-default-browser-check',
    '--disable-background-networking', '--disable-component-update', '--disable-sync',
    '--disable-extensions', '--disable-crash-reporter', 'about:blank',
  ], { windowsHide: true, stdio: ['ignore', 'ignore', 'pipe'] });
  browser.on('error', error => { startupError = error; });
  browserClosed = new Promise(resolve => browser.once('close', resolve));
  browser.stderr.on('data', data => { browserLog = (browserLog + data).slice(-100000); });
  report.stage = 'devtools-readiness';
  const ready = await waitForDevTools({
    profile,
    assertAlive: () => {
      if (startupError) throw startupError;
      assert(browser.exitCode === null && browser.signalCode === null, 'Dedicated browser exited early');
    },
    onProgress: progress => { report.devtoolsReadiness = progress; },
  });
  version = ready.version;
  report.browser = version.Browser;
  for (const name of selected) {
    report.stage = 'suite:' + name;
    await runSuite(name, ready.endpoint);
  }
  report.stage = 'completed';
  report.passed = true;
} catch (error) {
  report.error = String(error);
  report.errorStack = error.stack;
  process.exitCode = 1;
} finally {
  for (const child of runningTests) child.kill();
  await closeBrowser();
  if (server) { await server.close(); report.serverClosed = true; }
  if (browser && !report.browserClosed) { report.passed = false; report.error = 'Dedicated browser did not close'; process.exitCode = 1; }
  await writeFile(join(output, 'browser.log'), browserLog);
  await writeFile(join(output, 'headless-acceptance.json'), JSON.stringify(report, null, 2));
  console.log(JSON.stringify(report, null, 2));
}
