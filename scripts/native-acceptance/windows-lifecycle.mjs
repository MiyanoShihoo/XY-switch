import assert from 'node:assert/strict';
import { execFile, spawn } from 'node:child_process';
import { randomBytes } from 'node:crypto';
import { createServer } from 'node:http';
import { access, mkdir, readFile, rename, writeFile } from 'node:fs/promises';
import { join, relative, resolve, sep } from 'node:path';
import { parseArgs, promisify } from 'node:util';
import { setTimeout as delay } from 'node:timers/promises';
import { connect } from '../workspace-qa/cdp.mjs';
import { packageInput, requireDisposableRunner, sha256, waitForNativePage } from './lib.mjs';

requireDisposableRunner();
const run = promisify(execFile);
const { values: args } = parseArgs({ options: Object.fromEntries(['artifact', 'previous', 'output', 'temporary', 'app-data', 'port'].map(name => [name, { type: 'string' }])) });
for (const name of ['artifact', 'previous', 'output', 'temporary', 'app-data', 'port']) assert(args[name], name);
const temporary = resolve(args.temporary), output = resolve(args.output), appData = resolve(args['app-data']);
assert(temporary.startsWith(resolve(process.env.RUNNER_TEMP) + sep));
assert.equal(appData, join(process.env.APPDATA, 'com.xyswitch.desktop'));
assert.match(args.port, /^\d+$/);
await assert.rejects(access(appData), { code: 'ENOENT' });
await mkdir(output, { recursive: true });
const version = JSON.parse(await readFile('package.json', 'utf8')).version;
const input = await packageInput(args.artifact, 'windows-x64', version);
const previous = JSON.parse((await readFile(join(args.previous, 'previous-release.json'), 'utf8')).replace(/^\uFEFF/, ''));
assert.equal(previous.verified, true); assert.equal(previous.version, '1.0.0'); assert.equal(previous.platform, 'windows');
assert.equal(previous.commit, '1f0c8c197b9aab301bb3ac39683c1e5871e2045e'); assert.equal(previous.tag, 'v1.0.0');
for (const file of previous.files) assert.equal(sha256(await readFile(file.path)), file.sha256);
const results = [], secrets = new Set();
let client, application, executable, currentInstaller, currentFormat, activeStep = 'setup', upstream;
const scrub = value => [...secrets].reduce((text, secret) => text.replaceAll(secret, '[redacted]'), String(value));
const exists = async path => access(path).then(() => true, error => { if (error.code === 'ENOENT') return false; throw error; });
const invoke = (command, payload = {}) => client.evaluate(`window.__TAURI_INTERNALS__.invoke(${JSON.stringify(command)},${JSON.stringify(payload)})`);
async function step(name, work) {
  activeStep = name; console.log('RUN ' + name);
  const started = Date.now(), evidence = await work();
  results.push({ name, passed: true, elapsedMs: Date.now() - started, ...(evidence ? { evidence } : {}) }); console.log('PASS ' + name);
}
async function installer(path, format, install = true) {
  if (format === 'msi') await run('msiexec.exe', [install ? '/i' : '/x', path, '/qn', '/norestart', ...(install ? ['INSTALLDIR=' + resolve(executable, '..')] : []), '/l*v', join(temporary, 'msi-' + Date.now() + '.log')], { windowsHide: true, timeout: 240_000 }).catch(error => { if (error.code !== 3010) throw error; });
  else await run(path, install ? ['/S', '/D=' + resolve(executable, '..')] : ['/S'], { windowsHide: true, timeout: 240_000 });
}
async function stop() {
  if (client) { await client.close().catch(() => {}); client = undefined; }
  if (application && application.exitCode === null) await run('taskkill.exe', ['/PID', String(application.pid), '/T', '/F'], { windowsHide: true }).catch(error => { if (application.exitCode === null) throw error; });
  application = undefined;
  for (let attempt = 0; attempt < 40; attempt++) {
    try { await fetch(`http://127.0.0.1:${args.port}/json`, { signal: AbortSignal.timeout(500) }); }
    catch { return; }
    await delay(250);
  }
  throw new Error('Previous WebView debugger remained alive');
}
async function launch(name, legacy = false) {
  const codex = join(temporary, 'codex'); await mkdir(codex, { recursive: true });
  application = spawn(executable, [], { windowsHide: true, cwd: resolve(executable, '..'), env: { ...process.env, CODEX_HOME: codex, WEBVIEW2_USER_DATA_FOLDER: join(temporary, 'webview'), WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: `--remote-debugging-port=${args.port} --remote-debugging-address=127.0.0.1` }, stdio: 'ignore' });
  let launchError; application.once('error', error => { launchError = error; });
  process.env.APP_URL = 'http://tauri.localhost';
  for (let attempt = 0; attempt < 180 && !client; attempt++) {
    if (launchError) throw launchError;
    assert.equal(application.exitCode, null, 'Packaged application exited early');
    try { client = await connect(`http://127.0.0.1:${args.port}`, { native: true }); } catch { await delay(500); }
  }
  assert(client, 'No packaged native WebView'); await waitForNativePage(client);
  if (!legacy) {
    await invoke('set_interface_language', { preference: 'zh-CN', resolvedLanguage: 'zh-CN' });
    await client.evaluate('localStorage.setItem("xy-switch:language","zh-CN")'); await client.call('Page.reload');
    await client.waitFor('document.documentElement.lang === "zh-CN" && Boolean(document.querySelector(".provider-workspace"))', 60_000);
  }
  await client.screenshot(join(output, name + '.png'));
}
async function routing() {
  await client.click('.xy-shell-nav-item[title="设置"]');
  await client.waitFor('Boolean(document.querySelector("#settings-tab-routing"))'); await client.click('#settings-tab-routing');
  await client.waitFor('Boolean(document.querySelector(".settings-takeover"))');
}
const takeoverPanel = '[...document.querySelectorAll(".settings-takeover")].find(panel=>panel.querySelector("h3")?.textContent.trim()==="Codex")';
async function expectTakeover(active) {
  const rows = await invoke('list_app_takeovers'); const row = rows.find(value => value.app === 'codex');
  assert(row); assert.equal(row.active, active); assert.equal(row.lastError, null);
}
async function freePort() {
  const server = createServer(); await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
  const port = server.address().port; await new Promise(resolve => server.close(resolve)); return port;
}
try {
  for (const format of ['nsis', 'msi']) {
    currentFormat = format;
    executable = join(temporary, format, 'xy-switch.exe'); await mkdir(resolve(executable, '..'), { recursive: true });
    const old = previous.files.find(file => format === 'nsis' ? file.name.endsWith('setup.exe') : file.name.endsWith('.msi'));
    const fresh = input.files.find(file => file.format === format); assert(old && fresh);
    const provider = { id: 'lifecycle-' + format, name: 'Native lifecycle ' + format, providerType: 'custom', endpoint: 'http://127.0.0.1:1/v1', model: 'native-lifecycle-model', appGroup: 'Codex', enabled: true, sortOrder: 0, advanced: { upstreamFormat: 'openai_chat' } };
    const prompt = { id: 'lifecycle-prompt-' + format, name: 'Native lifecycle prompt', content: 'Synthetic installation upgrade recovery fixture', description: 'Disposable CI only', enabled: true };
    await step(format + '-old-install-and-native-fixture', async () => {
      await installer(old.path, format); currentInstaller = old.path; assert(await exists(executable));
      await launch(format + '-v1.0.0', true); await invoke('save_provider', { provider }); await invoke('save_prompt', { prompt });
      assert((await invoke('list_providers')).some(value => value.id === provider.id)); await stop();
    });
    await step(format + '-upgrade-and-preserve-config', async () => {
      await installer(fresh.path, format); currentInstaller = fresh.path;
      assert.equal(sha256(await readFile(executable)), input.installedSha256);
      await launch(format + '-v' + version);
      const saved = (await invoke('list_providers')).find(value => value.id === provider.id); assert(saved);
      for (const name of ['name', 'endpoint', 'model', 'enabled']) assert.equal(saved[name], provider[name]);
      assert((await invoke('list_prompts')).some(value => value.id === prompt.id && value.content === prompt.content));
    });
    const apiKey = randomBytes(32).toString('base64url'); secrets.add(apiKey);
    let received = 0, authorizationMatched = false;
    const upstreamErrors = [];
    upstream = createServer(async (request, response) => {
      try {
        const chunks = []; for await (const chunk of request) chunks.push(chunk);
        authorizationMatched = request.headers.authorization === 'Bearer ' + apiKey;
        assert.equal(request.url, '/v1/chat/completions'); assert(authorizationMatched);
        const body = JSON.parse(Buffer.concat(chunks)); assert.equal(body.model, provider.model); received++;
        response.writeHead(200, { 'Content-Type': 'application/json' });
        response.end(JSON.stringify({ id: 'native-upstream', object: 'chat.completion', model: provider.model, choices: [{ index: 0, message: { role: 'assistant', content: 'NATIVE_CREDENTIAL_OK' }, finish_reason: 'stop' }], usage: { prompt_tokens: 4, completion_tokens: 3, total_tokens: 7 } }));
      } catch (error) {
        upstreamErrors.push(scrub(error.stack));
        response.writeHead(500, { 'Content-Type': 'application/json' }); response.end('{"error":"Synthetic upstream validation failed"}');
      }
    });
    await new Promise(resolve => upstream.listen(0, '127.0.0.1', resolve));
    provider.endpoint = `http://127.0.0.1:${upstream.address().port}/v1`;
    const proxyPort = await freePort();
    const codex = join(temporary, 'codex'), originalConfig = Buffer.from('model = "original-model"\n# Synthetic original\n'), originalAuth = Buffer.from('{"tokens":{"fixture":"original"}}\n');
    await writeFile(join(codex, 'config.toml'), originalConfig); await writeFile(join(codex, 'auth.json'), originalAuth);
    const restored = async () => {
      assert.deepEqual(await readFile(join(codex, 'config.toml')), originalConfig); assert.deepEqual(await readFile(join(codex, 'auth.json')), originalAuth);
      assert.equal(await exists(join(codex, 'xy-switch-model-catalog.json')), false); await expectTakeover(false);
    };
    const forwarded = async () => {
      const auth = JSON.parse(await readFile(join(codex, 'auth.json'), 'utf8')), localToken = auth.OPENAI_API_KEY;
      assert.equal(typeof localToken, 'string'); assert(localToken); secrets.add(localToken);
      const before = received;
      const response = await fetch(`http://127.0.0.1:${proxyPort}/v1/responses`, { method: 'POST', headers: { Authorization: 'Bearer ' + localToken, 'Content-Type': 'application/json' }, body: JSON.stringify({ model: provider.model, input: 'Synthetic credential check', stream: false }), signal: AbortSignal.timeout(30_000) });
      assert.deepEqual(upstreamErrors, []); assert.equal(response.status, 200);
      const body = await response.text(); assert(body.includes('NATIVE_CREDENTIAL_OK')); assert.equal(received, before + 1); assert(authorizationMatched);
      return { nativeAuthorizationMatched: true, responseSha256: sha256(Buffer.from(body)), upstreamRequests: received, chargedApiCalls: 0 };
    };
    await step(format + '-native-credential-and-ui-takeover', async () => {
      await invoke('save_provider', { provider }); await invoke('set_current_provider', { id: provider.id });
      await invoke('set_provider_api_key', { providerId: provider.id, apiKey });
      assert.equal(await invoke('has_provider_api_key', { providerId: provider.id }), true);
      await invoke('save_proxy_settings', { settings: { enabled: false, showInHome: true, host: '127.0.0.1', port: proxyPort, routeTargets: ['codex'] } });
      await routing(); await client.button('启动路由'); await client.waitFor('document.body.innerText.includes("停止路由")');
      assert.equal((await invoke('get_proxy_status')).running, true);
      await client.button('接入路由', takeoverPanel); await client.waitFor(`${takeoverPanel}?.innerText.includes("已接管")`); await expectTakeover(true);
      const evidence = await forwarded();
      await client.screenshot(join(output, format + '-routing.png'));
      await client.button('恢复原配置', takeoverPanel); await client.waitFor(`${takeoverPanel}?.innerText.includes("未接管")`); await restored();
      await client.button('接入路由', takeoverPanel); await client.waitFor(`${takeoverPanel}?.innerText.includes("已接管")`);
      await client.button('停止路由'); await client.waitFor('document.body.innerText.includes("启动路由")'); await restored();
      await assert.rejects(fetch(`http://127.0.0.1:${proxyPort}/health`, { signal: AbortSignal.timeout(2_000) }));
      return evidence;
    });
    await step(format + '-crash-cold-start-and-credential-persistence', async () => {
      await client.button('启动路由'); await client.waitFor('document.body.innerText.includes("停止路由")');
      await client.button('接入路由', takeoverPanel); await client.waitFor(`${takeoverPanel}?.innerText.includes("已接管")`); await stop();
      await launch(format + '-cold-start'); const status = await invoke('get_proxy_status'); assert.equal(status.running, false); assert.equal(status.settings.enabled, false);
      await restored(); assert.equal(await invoke('has_provider_api_key', { providerId: provider.id }), true);
      await routing(); await client.button('启动路由'); await client.waitFor('document.body.innerText.includes("停止路由")');
      await client.button('接入路由', takeoverPanel); await client.waitFor(`${takeoverPanel}?.innerText.includes("已接管")`);
      const evidence = await forwarded();
      await client.button('停止路由'); await client.waitFor('document.body.innerText.includes("启动路由")'); await restored();
      return { forcedTermination: true, recoveredOriginalBytes: true, ...evidence };
    });
    await step(format + '-configuration-export-import', async () => {
      const exported = await invoke('export_config'); const bundle = JSON.parse(exported); assert.equal(bundle.version, 2);
      assert(!exported.includes(apiKey)); await invoke('delete_provider', { id: provider.id }); await invoke('delete_prompt', { id: prompt.id });
      assert(!(await invoke('list_providers')).some(value => value.id === provider.id));
      const imported = await invoke('import_config', { content: exported }); assert(imported.providers > 0 && imported.prompts > 0);
      assert((await invoke('list_providers')).some(value => value.id === provider.id && value.endpoint === provider.endpoint));
      assert((await invoke('list_prompts')).some(value => value.id === prompt.id && value.content === prompt.content));
      await writeFile(join(output, format + '-config-backup.json'), exported);
    });
    await invoke('delete_provider_api_key', { providerId: provider.id }); assert.equal(await invoke('has_provider_api_key', { providerId: provider.id }), false);
    await stop(); await new Promise(resolve => upstream.close(resolve)); upstream = undefined;
    await step(format + '-uninstall-reinstall-and-config-restore', async () => {
      await installer(format === 'nsis' ? join(resolve(executable, '..'), 'uninstall.exe') : fresh.path, format, false);
      for (let attempt = 0; attempt < 60 && await exists(executable); attempt++) await delay(500);
      assert.equal(await exists(executable), false); currentInstaller = undefined;
      await installer(fresh.path, format); currentInstaller = fresh.path; await launch(format + '-reinstalled');
      assert((await invoke('list_providers')).some(value => value.id === provider.id));
      const backup = await readFile(join(output, format + '-config-backup.json'), 'utf8');
      await invoke('delete_provider', { id: provider.id }); await invoke('import_config', { content: backup });
      assert((await invoke('list_providers')).some(value => value.id === provider.id)); await restored(); await stop();
      await installer(format === 'nsis' ? join(resolve(executable, '..'), 'uninstall.exe') : fresh.path, format, false);
      for (let attempt = 0; attempt < 60 && await exists(executable); attempt++) await delay(500);
      assert.equal(await exists(executable), false); currentInstaller = undefined;
    });
    const archivedData = join(temporary, format + '-app-data'); assert(!relative(temporary, archivedData).startsWith('..'));
    await rename(appData, archivedData);
  }
} catch (error) { results.push({ name: activeStep, passed: false, error: scrub(error.stack) }); process.exitCode = 1; }
finally {
  await stop().catch(error => { results.push({ name: 'process-cleanup', passed: false, error: scrub(error) }); process.exitCode = 1; });
  if (upstream) await new Promise(resolve => upstream.close(resolve));
  if (currentInstaller) await installer(currentFormat === 'nsis' ? join(resolve(executable, '..'), 'uninstall.exe') : currentInstaller, currentFormat, false).catch(error => { results.push({ name: 'installer-cleanup', passed: false, error: scrub(error) }); process.exitCode = 1; });
  await writeFile(join(output, 'lifecycle-results.json'), JSON.stringify({ passed: process.exitCode !== 1, sourceCommit: process.env.NATIVE_SOURCE_COMMIT, harnessCommit: process.env.GITHUB_SHA, sourceRun: process.env.NATIVE_SOURCE_RUN, previousCommit: previous.commit, previousVersion: previous.version, currentVersion: version, installedSha256: input.installedSha256, apiCallsCharged: 0, results, scope: 'Real NSIS and MSI installation and upgrade; packaged GUI clicks and production IPC; native credential read by a forwarded request to synthetic loopback upstream; forced termination and startup restoration; export/import and reinstall' }, null, 2) + '\n');
}
