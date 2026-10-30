import assert from 'node:assert/strict';
import test from 'node:test';
import { waitForDevTools } from './devtools-readiness.mjs';

const activePort = '4321\n/devtools/browser/fixture\n';
const version = { Browser: 'Chrome/fixture', webSocketDebuggerUrl: 'ws://127.0.0.1:4321/devtools/browser/fixture' };
const readyResponse = () => ({ ok: true, json: async () => version });
function fixture(overrides = {}) {
  let clock = 0;
  const progress = [];
  const requests = [];
  const options = {
    profile: '/qa-profile', timeoutMs: 100, probeTimeoutMs: 30, intervalMs: 10,
    now: () => clock,
    pause: async ms => { clock += ms; },
    readActivePort: async () => activePort,
    request: async (url, options) => { requests.push({ url, options }); return readyResponse(); },
    onProgress: value => progress.push(value),
    ...overrides,
  };
  return { options, progress, requests, advance: ms => { clock += ms; } };
}
test('returns only after the exact dedicated browser endpoint responds', async () => {
  const qa = fixture();
  const result = await waitForDevTools(qa.options);
  assert.equal(result.endpoint, 'http://127.0.0.1:4321');
  assert.equal(result.version, version);
  assert.equal(result.attempts, 1);
  assert.equal(qa.requests[0].options.redirect, 'error');
  assert(qa.requests[0].options.signal instanceof AbortSignal);
  assert.equal(qa.progress.at(-1).phase, 'ready');
});
test('waits for the active-port file, including partial writes', async () => {
  const ports = ['', '4321\n', activePort.replace(/\n/g, '\r\n')];
  const qa = fixture({ readActivePort: async () => ports.shift() });
  const result = await waitForDevTools(qa.options);
  assert.equal(result.elapsedMs, 20);
  assert.equal(result.attempts, 1);
});
test('retries a warmup HTTP timeout before running any suite', async () => {
  let count = 0;
  const qa = fixture({ request: async () => {
    if (++count === 1) throw new DOMException('warmup', 'TimeoutError');
    return readyResponse();
  } });
  const result = await waitForDevTools(qa.options);
  assert.equal(result.attempts, 2);
  assert.equal(result.elapsedMs, 10);
  assert.match(qa.progress[1].lastError, /TimeoutError: warmup/);
});
test('retries temporary connection refusal and HTTP unavailability', async () => {
  let count = 0;
  const qa = fixture({ request: async () => {
    count++;
    if (count === 1) throw new TypeError('fetch failed');
    if (count === 2) return { ok: false, status: 503 };
    return readyResponse();
  } });
  assert.equal((await waitForDevTools(qa.options)).attempts, 3);
});
test('never exceeds the overall readiness deadline when probes use the budget', async () => {
  const qa = fixture();
  qa.options.request = async () => { qa.advance(Math.min(30, qa.options.timeoutMs - qa.options.now())); throw new Error('CDP warming up'); };
  await assert.rejects(waitForDevTools(qa.options), /timed out after 100 ms.*3 HTTP probes.*CDP warming up/);
  assert.equal(qa.progress.at(-1).elapsedMs, 100);
  assert.equal(qa.progress.at(-1).phase, 'timeout');
});
test('missing port file times out without sending any HTTP request', async () => {
  const qa = fixture({ readActivePort: async () => '' });
  await assert.rejects(waitForDevTools(qa.options), /0 HTTP probes.*not available/);
  assert.equal(qa.requests.length, 0);
});
test('invalid ports never result in a request', async () => {
  for (const port of ['0', '65536', '-1', 'not-a-port']) {
    const qa = fixture({ readActivePort: async () => port + '\n/devtools/browser/fixture\n' });
    await assert.rejects(waitForDevTools(qa.options), /invalid endpoint/);
    assert.equal(qa.requests.length, 0);
  }
});
test('an early browser exit is not hidden by a retry timeout', async () => {
  const qa = fixture({ assertAlive: () => { throw new Error('Dedicated browser exited early'); } });
  await assert.rejects(waitForDevTools(qa.options), /exited early/);
  assert.equal(qa.requests.length, 0);
});
test('rejects other origins, protocols, ports and CDP credentials immediately', async () => {
  for (const url of ['ws://localhost:4321/devtools/browser/fixture', 'ws://127.0.0.1:4322/devtools/browser/fixture', 'wss://127.0.0.1:4321/devtools/browser/fixture', 'ws://user:secret@127.0.0.1:4321/devtools/browser/fixture']) {
    const qa = fixture({ request: async () => ({ ok: true, json: async () => ({ ...version, webSocketDebuggerUrl: url }) }) });
    await assert.rejects(waitForDevTools(qa.options), /exact IPv4 loopback origin|must not contain credentials/);
    assert.equal(qa.progress.length, 1);
  }
});
test('rejects a CDP endpoint belonging to another browser profile', async () => {
  const qa = fixture({ request: async () => ({ ok: true, json: async () => ({ ...version, webSocketDebuggerUrl: 'ws://127.0.0.1:4321/devtools/browser/another' }) }) });
  await assert.rejects(waitForDevTools(qa.options), /match the dedicated profile/);
});
test('rejects a non-Chromium version response', async () => {
  const qa = fixture({ request: async () => ({ ok: true, json: async () => ({ webSocketDebuggerUrl: version.webSocketDebuggerUrl }) }) });
  await assert.rejects(waitForDevTools(qa.options), /Invalid Chromium version response/);
});
test('retries an incomplete JSON response', async () => {
  let count = 0;
  const qa = fixture({ request: async () => ++count === 1 ? ({ ok: true, json: async () => { throw new SyntaxError('Unexpected end of JSON'); } }) : readyResponse() });
  assert.equal((await waitForDevTools(qa.options)).attempts, 2);
});
test('rejects missing profiles and invalid time limits', async () => {
  await assert.rejects(waitForDevTools(), /dedicated browser profile/);
  for (const value of [0, -1, NaN, Infinity]) {
    const qa = fixture({ timeoutMs: value });
    await assert.rejects(waitForDevTools(qa.options), /time limits must be positive/);
  }
});
