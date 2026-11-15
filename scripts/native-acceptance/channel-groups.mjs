import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { spawn } from 'node:child_process';
import { once } from 'node:events';
import { createReadStream } from 'node:fs';
import { copyFile, mkdir, mkdtemp, open, readFile, realpath, writeFile } from 'node:fs/promises';
import { createServer } from 'node:http';
import net from 'node:net';
import path from 'node:path';
import { DatabaseSync } from 'node:sqlite';
import { setTimeout as delay } from 'node:timers/promises';
import { connect } from '../workspace-qa/cdp.mjs';
import { resolveIsolatedDebugRuntime } from '../workspace-qa/isolated-debug.mjs';

const [sourceExecutable, outputDirectory] = process.argv.slice(2);
assert.equal(process.platform, 'win32', 'This native acceptance runs on Windows');
assert(sourceExecutable && outputDirectory, 'Usage: node scripts/native-acceptance/channel-groups.mjs DEBUG_EXE OUTPUT_UNDER_OUT');
const source = await realpath(sourceExecutable);
assert(source.toLowerCase().includes(path.sep + 'debug' + path.sep), 'Use a debug build; release builds do not honor isolation overrides');
const workspace = await realpath(process.cwd());
const outputRoot = await realpath(path.join(workspace, 'out'));
const output = path.resolve(outputDirectory);
const relativeOutput = path.relative(outputRoot, output);
assert(relativeOutput && !relativeOutput.startsWith('..') && !path.isAbsolute(relativeOutput), 'Output must be inside workspace out');
await mkdir(output, { recursive: true });
const runtimeRoot = await mkdtemp(path.join(output, 'xy-switch_native-'));
for (const directory of ['data', 'home', 'bin', 'webview', 'screenshots']) await mkdir(path.join(runtimeRoot, directory));
const runtime = await resolveIsolatedDebugRuntime(runtimeRoot);
const executable = path.join(runtimeRoot, 'bin', 'xy-switch.exe');
await copyFile(source, executable);
const hash = createHash('sha256');
for await (const chunk of createReadStream(executable)) hash.update(chunk);
const version = JSON.parse(await readFile(path.join(workspace, 'package.json'), 'utf8')).version;
const reports = [];
const simulations = {};
const sinks = [];
let child, childExit, c, currentStep, currentGroup, originalProviders;
let upstreamRequests = 0;
let launchCount = 0;
const summary = { version, platform: 'windows', mode: 'isolated-debug-real-ipc', runtimeRoot, executableSha256: hash.digest('hex'),
  liveModelPosts: 0, billingHold: true, reports, simulations };

async function freePort() {
  const listener = net.createServer();
  listener.listen(0, '127.0.0.1'); await once(listener, 'listening');
  const port = listener.address().port;
  await new Promise((resolve, reject) => listener.close(error => error ? reject(error) : resolve()));
  return port;
}

async function launch() {
  const port = await freePort();
  const endpoint = `http://127.0.0.1:${port}`;
  const environment = { ...process.env,
    XY_SWITCH_TEST_DATA_DIR: runtime.dataDir, XY_SWITCH_TEST_HOME_DIR: runtime.homeDir,
    XY_SWITCH_TEST_AGENT_CODEX_ROOT: path.join(runtime.homeDir, '.codex', 'sessions'),
    XY_SWITCH_TEST_AGENT_CODEX_ARCHIVE_ROOT: path.join(runtime.homeDir, '.codex', 'archived_sessions'),
    XY_SWITCH_TEST_AGENT_DSH_ROOT: path.join(runtime.homeDir, 'missing-dsh'),
    WEBVIEW2_USER_DATA_FOLDER: path.join(runtimeRoot, 'webview'),
    WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: `--remote-debugging-address=127.0.0.1 --remote-debugging-port=${port} --no-first-run`,
    RUST_LOG: 'warn' };
  for (const name of Object.keys(environment)) if (/(?:API_KEY|ACCESS_TOKEN|AUTH_TOKEN)$/.test(name)) delete environment[name];
  const stdout = await open(path.join(runtimeRoot, `launch-${++launchCount}.stdout.log`), 'a');
  const stderr = await open(path.join(runtimeRoot, `launch-${launchCount}.stderr.log`), 'a');
  child = spawn(executable, [], { cwd: path.dirname(executable), env: environment, windowsHide: true, stdio: ['ignore', stdout.fd, stderr.fd] });
  childExit = new Promise((resolve, reject) => { child.once('exit', resolve); child.once('error', reject); });
  childExit.catch(() => {});
  await Promise.all([stdout.close(), stderr.close()]);
  let ready = false;
  const deadline = Date.now() + 60_000;
  while (Date.now() < deadline) {
    assert.equal(child.exitCode, null, 'Native application exited during startup');
    try {
      const pages = await fetch(endpoint + '/json', { signal: AbortSignal.timeout(1500) }).then(response => response.json());
      if (pages.some(page => page.type === 'page' && page.url.startsWith('http://tauri.localhost'))) { ready = true; break; }
    } catch {}
    await delay(200);
  }
  assert(ready, 'Native WebView did not become ready');
  c = await connect(endpoint, { native: true, isolatedDebugRoot: runtimeRoot });
  await c.waitFor('Boolean(document.querySelector(".xy-shell-root"))', 30_000);
  assert.equal(await invoke('app_version'), version);
  return await invoke('app_paths');
}

async function stop() {
  const connection = c;
  c = null;
  try {
    if (connection) await connection.close();
  } finally {
    if (child && child.exitCode === null && child.signalCode === null) {
      const timeout = new AbortController();
      try {
        child.kill();
        await Promise.race([childExit, delay(15_000, undefined, { signal: timeout.signal })
          .then(() => { throw new Error('Owned native process did not exit'); })]);
      } finally { timeout.abort(); }
    }
    child = null;
  }
}

async function invoke(command, args = {}) {
  const result = await c.evaluate(`window.__TAURI_INTERNALS__.invoke(${JSON.stringify(command)},${JSON.stringify(args)}).then(value=>({ok:true,value}),error=>({ok:false,error:typeof error==="string"?error:JSON.stringify(error)}))`);
  if (!result.ok) throw new Error(result.error);
  return result.value;
}
async function step(name, task) {
  currentStep = name;
  const started = Date.now();
  await task();
  reports.push({ name, passed: true, durationMs: Date.now() - started });
  console.log(`PASS ${name}`);
}
async function fillLabel(label, value, scope = 'document') {
  const id = await c.evaluate(`(()=>{const root=${scope};return [...root.querySelectorAll('label[for]')].find(item=>item.textContent.trim().replace(/\\s*\\*$/, '')===${JSON.stringify(label)})?.htmlFor;})()`);
  assert(id, `Missing field ${label}`);
  await c.fill(`[id=${JSON.stringify(id)}]`, value);
}
const shot = name => c.screenshot(path.join(runtimeRoot, 'screenshots', `${name}.png`));
const getGroup = async () => (await invoke('channel_groups_list')).configuration.groups[0];
async function saveGroup(change) {
  const snapshot = await invoke('channel_groups_save', { input: { group: { ...await getGroup(), ...change }, project: null } });
  currentGroup = snapshot.configuration.groups[0];
  return currentGroup;
}
function request(id, atMs, fields = {}) {
  return { kind: 'request', atMs, request: { id, groupId: currentGroup.id, reserveTokens: 20, reserveMicrousd: 10,
    stateful: false, sessionKey: null, previousResponseId: null, conversationId: null, ...fields } };
}
function complete(requestId, atMs, result = {}) {
  return { kind: 'complete', requestId, atMs, result: { outcome: 'http', status: 200, usage: { tokens: 10, microusd: 1 },
    headers: {}, responseId: null, conversationId: null, ...result } };
}
async function simulate(name, steps) {
  const result = await invoke('channel_groups_simulate', { input: { groupId: currentGroup.id, replayEventIds: [], steps } });
  assert.equal(result.source, 'offlineSimulation');
  assert.equal(result.liveDispatches, 0); assert.equal(result.ledgerWrites, 0);
  assert(result.decisions.every(item => item.dispatchAllowed === false));
  simulations[name] = result;
  return result;
}
function ledgerCounts() {
  const database = new DatabaseSync(runtime.databasePath, { readOnly: true });
  try {
    return Object.fromEntries(['request_events', 'usage_events', 'budget_ledger'].map(table =>
      [table, database.prepare(`SELECT count(*) AS count FROM ${table}`).get().count]));
  } finally { database.close(); }
}

try {
  await step('isolated-native-startup', async () => {
    summary.appPaths = await launch();
    const proxy = await invoke('get_proxy_status');
    assert.equal(proxy.running, false); assert.equal(proxy.settings.enabled, false);
    const snapshot = await invoke('channel_groups_list');
    assert.equal(snapshot.phase, 'offlinePilot'); assert.equal(snapshot.liveEnabled, false); assert.equal(snapshot.billingHold, true);
    assert.equal(snapshot.configuration.groups.length, 0);
    originalProviders = await invoke('list_providers');
    summary.ledgerBefore = ledgerCounts();
    await invoke('set_interface_language', { preference: 'zh-CN', resolvedLanguage: 'zh-CN' });
    await c.navigate(await c.evaluate('location.href'));
  });
  await step('native-provider-references-with-loopback-sinks', async () => {
    for (const name of ['alpha', 'beta', 'gamma']) {
      const sink = createServer((request, response) => { upstreamRequests++; request.resume(); response.writeHead(503); response.end('Unexpected dispatch to native fixture'); });
      sink.listen(0, '127.0.0.1'); await once(sink, 'listening'); sinks.push(sink);
      await invoke('save_provider', { provider: { id: `qa-native-${name}`, name: `Native ${name}`, providerType: 'openai_compatible',
        endpoint: `http://127.0.0.1:${sink.address().port}/v1`, model: 'qa-native-model', group: 'Codex', enabled: true,
        advanced: { upstreamFormat: 'openai_chat' } } });
    }
    assert.equal((await invoke('list_providers')).filter(provider => provider.id.startsWith('qa-native-')).length, 3);
    await c.navigate(await c.evaluate('location.href'));
    await c.workspace('providers'); await c.button('渠道组');
    await c.waitFor('Boolean(document.querySelector(".channel-group-directory"))');
    await shot('01-empty');
  });
  await step('create-three-channel-group-through-native-ui', async () => {
    await c.button('新建渠道组');
    await fillLabel('渠道组名称', 'Native acceptance'); await fillLabel('模型别名', 'qa-native');
    const providers = ['qa-native-alpha', 'qa-native-beta', 'qa-native-gamma'];
    for (let index = 0; index < providers.length; index++) {
      if (index) await c.button('添加渠道');
      const scope = `document.querySelectorAll('.channel-group-channel')[${index}]`;
      await fillLabel('供应商引用', providers[index], scope); await fillLabel('上游模型 ID', 'qa-native-model', scope);
    }
    await shot('02-editor'); await c.button('保存离线配置');
    await c.waitFor('!document.querySelector(".channel-group-editor")');
    currentGroup = await getGroup(); assert.equal(currentGroup.channels.length, 3); assert.equal(currentGroup.revision, 1);
    assert.equal(currentGroup.virtualModel, 'xy-group/qa-native'); await shot('03-saved');
  });
  await step('edit-disable-enable-and-reject-stale-revision', async () => {
    const stale = structuredClone(currentGroup);
    await c.button('编辑配置'); await fillLabel('渠道组名称', 'Native acceptance persisted');
    await c.button('保存离线配置'); await c.waitFor('!document.querySelector(".channel-group-editor")');
    await c.button('停用组（离线）'); await c.waitFor('document.body.innerText.includes("离线已停用")');
    assert.equal((await getGroup()).enabled, false);
    await c.button('启用组（离线）'); await c.waitFor('document.body.innerText.includes("离线已启用")');
    currentGroup = await getGroup(); assert.equal(currentGroup.enabled, true); assert.equal(currentGroup.revision, 4);
    await assert.rejects(invoke('channel_groups_save', { input: { group: stale, project: null } }), /group_revision_conflict/);
    assert.deepEqual(await getGroup(), currentGroup);
  });
  await step('native-ui-simulation-and-recorded-event-read', async () => {
    await c.button('读取本地记录'); await c.waitFor('document.body.innerText.includes("暂无可回放的本地记录")');
    await c.button('运行固定模拟'); await c.waitFor('Boolean(document.querySelector(".channel-group-trial-result"))');
    assert.equal(await c.evaluate('document.querySelectorAll(".channel-group-decisions > li").length'), 6);
    await shot('04-native-simulation');
  });
  await step('real-ipc-round-robin-and-quota-awareness', async () => {
    const at = Date.now();
    const rr = await simulate('roundRobin', Array.from({ length: 9 }, (_, index) => [request(`rr-${index}`, at + index * 3), complete(`rr-${index}`, at + index * 3 + 1)]).flat());
    assert.deepEqual(rr.decisions.map(item => item.channelId), Array.from({ length: 9 }, (_, index) => currentGroup.channels[index % 3].id));
    assert.equal(new Set(rr.channels.map(channel => channel.poolId)).size, 1);
    const shared = await simulate('sharedQuota', [request('s0', at), complete('s0', at + 1, { headers: { 'x-ratelimit-remaining-requests': '0', 'x-ratelimit-reset-requests': '60s' } }), request('s1', at + 2)]);
    assert.equal(shared.decisions[1].channelId, null);
    await saveGroup({ strategy: 'quotaAware', channels: currentGroup.channels.map(channel => ({ ...channel, quotaIndependent: true })) });
    const quota = await simulate('quotaAware', [request('q0', at), complete('q0', at + 1, { headers: { 'x-ratelimit-remaining-requests': '0', 'x-ratelimit-reset-requests': '60s' } }),
      request('q1', at + 2), complete('q1', at + 3, { headers: { 'x-ratelimit-remaining-requests': '100', 'x-ratelimit-reset-requests': '60s' } }), request('q2', at + 4)]);
    assert.equal(quota.decisions[0].channelId, currentGroup.channels[0].id);
    assert.equal(quota.decisions[1].channelId, currentGroup.channels[1].id);
    assert.equal(quota.decisions[2].channelId, currentGroup.channels[1].id);
    assert.equal(quota.channels.find(channel => channel.channelId === currentGroup.channels[2].id).quota.remainingRequests, null);
  });
  await step('real-ipc-cooldown-auth-and-stream-affinity', async () => {
    await saveGroup({ strategy: 'roundRobin' });
    const at = Date.now();
    const limited = await simulate('retryAfter', [request('limited', at), complete('limited', at + 1, { status: 429,
      headers: { 'retry-after': '30', 'x-ratelimit-reset-requests': '45s' }, usage: { tokens: null, microusd: null } }), request('after-limit', at + 2)]);
    assert.notEqual(limited.decisions[1].channelId, currentGroup.channels[0].id);
    assert(limited.channels.find(channel => channel.channelId === currentGroup.channels[0].id).recoveryAtMs >= at + 45_001);
    const denied = await simulate('authDenied', [request('denied', at), complete('denied', at + 1, { status: 401, usage: { tokens: null, microusd: null } }), request('after-auth', at + 2)]);
    assert.notEqual(denied.decisions[1].channelId, currentGroup.channels[0].id);
    assert(denied.decisions[1].excluded.some(item => item.channelId === currentGroup.channels[0].id));
    const affinity = { stateful: true, sessionKey: 'native-session' };
    const unknown = await simulate('streamUnknown', [request('stream', at, affinity), { kind: 'streamStarted', atMs: at + 1, requestId: 'stream' },
      complete('stream', at + 2, { outcome: 'timeout', status: null, usage: { tokens: null, microusd: null } }), request('continuation', at + 3, affinity)]);
    const streamedChannel = unknown.channels.find(channel => channel.channelId === currentGroup.channels[0].id);
    assert.equal(unknown.decisions[1].channelId, null); assert.equal(streamedChannel.unknownResults, 1);
    assert.equal(streamedChannel.simulatedUsage.costKnownRequests, 0);
  });
  await step('real-loopback-proxy-rejects-virtual-models-without-ledger-writes', async () => {
    await invoke('set_current_provider', { id: 'qa-native-alpha' });
    const status = await invoke('get_proxy_status');
    const port = await freePort();
    await invoke('save_proxy_settings', { settings: { ...status.settings, host: '127.0.0.1', port, enabled: false } });
    const running = await invoke('set_proxy_enabled', { enabled: true });
    assert.equal(running.running, true);
    summary.proxyRejections = [];
    try {
      for (const route of ['/v1/chat/completions', '/chat/completions', '/v1/responses', '/responses', '/v1/messages', '/messages']) {
        const response = await fetch(`http://127.0.0.1:${port}${route}`, { method: 'POST', headers: { 'content-type': 'application/json' },
          body: JSON.stringify({ model: 'xy-group/qa-native', messages: [{ role: 'user', content: 'local gate acceptance' }], input: 'local gate acceptance', max_tokens: 1, stream: true }), signal: AbortSignal.timeout(5000) });
        const body = await response.text(); assert.equal(response.status, 400); assert(body.includes('channel_group_pilot_only'));
        summary.proxyRejections.push({ route, status: response.status, code: 'channel_group_pilot_only' });
      }
    } finally { await invoke('set_proxy_enabled', { enabled: false }); }
    assert.equal(upstreamRequests, 0); assert.deepEqual(ledgerCounts(), summary.ledgerBefore);
  });
  await step('owned-process-restart-persists-native-configuration', async () => {
    const configuration = (await invoke('channel_groups_list')).configuration;
    await stop(); await launch();
    assert.deepEqual((await invoke('channel_groups_list')).configuration, configuration);
    assert.equal((await invoke('get_proxy_status')).running, false);
    const preserved = (await invoke('list_providers')).filter(provider => originalProviders.some(original => original.id === provider.id));
    assert.deepEqual(preserved, originalProviders);
    await c.workspace('providers'); await c.button('渠道组');
    await c.waitFor('Boolean(document.querySelector("[data-group-open]"))');
    await c.click('[data-group-open]'); await shot('05-restarted');
    summary.ledgerAfter = ledgerCounts(); assert.deepEqual(summary.ledgerAfter, summary.ledgerBefore);
    assert.equal(upstreamRequests, 0); assert.equal(c.exceptions.length, 0);
  });
  summary.passed = true;
} catch (error) {
  summary.passed = false; summary.error = String(error.stack ?? error);
  reports.push({ name: currentStep, passed: false, error: String(error.message ?? error) });
  if (c) {
    await shot('failure').catch(() => {});
    await writeFile(path.join(runtimeRoot, 'failure.json'), JSON.stringify(await c.snapshot().catch(() => null), null, 2));
  }
  console.error(error); process.exitCode = 1;
} finally {
  await stop().catch(error => { summary.passed = false; summary.shutdownError = String(error); process.exitCode = 1; });
  for (const sink of sinks) await new Promise(resolve => sink.close(resolve));
  summary.upstreamRequests = upstreamRequests;
  summary.finishedAt = new Date().toISOString();
  await writeFile(path.join(runtimeRoot, 'xy-switch_native-acceptance.json'), JSON.stringify(summary, null, 2) + '\n');
  await writeFile(path.join(output, 'xy-switch_native-latest.json'), JSON.stringify({ runtimeRoot, passed: summary.passed }, null, 2) + '\n');
  console.log(JSON.stringify({ passed: summary.passed, checks: reports.length, upstreamRequests, report: path.join(runtimeRoot, 'xy-switch_native-acceptance.json') }));
}
