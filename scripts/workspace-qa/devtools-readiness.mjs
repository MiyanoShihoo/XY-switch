import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { join } from 'node:path';
import { setTimeout as delay } from 'node:timers/promises';

export async function waitForDevTools({
  profile,
  assertAlive = () => {},
  timeoutMs = 60000,
  probeTimeoutMs = 5000,
  intervalMs = 250,
  readActivePort = () => readFile(join(profile, 'DevToolsActivePort'), 'utf8').catch(error => {
    if (error.code === 'ENOENT') return '';
    throw error;
  }),
  request = fetch,
  now = () => performance.now(),
  pause = delay,
  onProgress = () => {},
} = {}) {
  assert(typeof profile === 'string' && profile, 'A dedicated browser profile is required');
  for (const value of [timeoutMs, probeTimeoutMs, intervalMs]) {
    assert(Number.isFinite(value) && value > 0, 'Browser readiness time limits must be positive');
  }
  const started = now();
  let attempts = 0;
  let lastError = 'DevToolsActivePort is not available yet';
  while (now() - started < timeoutMs) {
    assertAlive();
    const activePort = await readActivePort();
    const [portLine, browserPath] = activePort.trim().split(/\r?\n/);
    const port = Number(portLine);
    const validPort = /^\d+$/.test(portLine) && Number.isInteger(port) && port > 0 && port <= 65535;
    const validPath = /^\/devtools\/browser\/[a-zA-Z0-9-]+$/.test(browserPath ?? '');
    if (validPort && validPath && now() - started < timeoutMs) {
      const endpoint = 'http://127.0.0.1:' + port;
      attempts++;
      onProgress({ phase: 'http-version', attempts, elapsedMs: Math.round(now() - started), lastError });
      let version;
      try {
        const response = await request(endpoint + '/json/version', {
          signal: AbortSignal.timeout(Math.max(1, Math.ceil(Math.min(probeTimeoutMs, timeoutMs - (now() - started))))),
          redirect: 'error',
        });
        if (!response.ok) throw new Error('CDP version returned HTTP ' + response.status);
        version = await response.json();
      } catch (error) {
        lastError = String(error);
      }
      if (version !== undefined) {
        assert(typeof version?.Browser === 'string' && version.Browser, 'Invalid Chromium version response');
        const websocket = new URL(version.webSocketDebuggerUrl);
        assert.equal(websocket.origin, endpoint.replace('http:', 'ws:'), 'Browser CDP must stay on its exact IPv4 loopback origin');
        assert.equal(websocket.pathname, browserPath, 'Browser CDP must match the dedicated profile');
        assert(!websocket.username && !websocket.password, 'Browser CDP must not contain credentials');
        assertAlive();
        const result = { endpoint, version, attempts, elapsedMs: Math.round(now() - started) };
        onProgress({ phase: 'ready', attempts, elapsedMs: result.elapsedMs });
        return result;
      }
    } else {
      lastError = activePort ? 'DevToolsActivePort is not fully written or contains an invalid endpoint' : 'DevToolsActivePort is not available yet';
    }
    const remaining = timeoutMs - (now() - started);
    if (remaining > 0) await pause(Math.min(intervalMs, remaining));
  }
  assertAlive();
  onProgress({ phase: 'timeout', attempts, elapsedMs: Math.round(now() - started), lastError });
  throw new Error('Isolated browser CDP readiness timed out after ' + timeoutMs + ' ms (' + attempts + ' HTTP probes): ' + lastError);
}
