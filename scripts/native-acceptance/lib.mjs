import assert from 'node:assert/strict';
import { createDecipheriv, createHash, pbkdf2Sync } from 'node:crypto';
import { readdir, readFile, stat } from 'node:fs/promises';
import { basename, join, resolve } from 'node:path';
import { setTimeout as delay } from 'node:timers/promises';

export function requireDisposableRunner(platform = process.platform, env = process.env) {
  assert.equal(env.GITHUB_ACTIONS, 'true', 'Native acceptance requires GitHub Actions');
  assert.equal(env.RUNNER_ENVIRONMENT, 'github-hosted', 'Native acceptance requires a disposable GitHub-hosted runner');
  assert.equal(platform, 'win32', 'This native acceptance harness requires Windows');
}
export async function waitForNativePage(client, { timeoutMs = 60_000, intervalMs = 150 } = {}) {
  assert(Number.isSafeInteger(timeoutMs) && timeoutMs > 0);
  assert(Number.isSafeInteger(intervalMs) && intervalMs >= 0);
  const deadline = Date.now() + timeoutMs;
  let state;
  do {
    try {
      state = await client.evaluate(`({url:location.href,origin:location.origin,title:document.title,
        readyState:document.readyState,productionIpc:Boolean(window.__TAURI_INTERNALS__?.invoke),
        mockIpc:Boolean(window.__XY_QA__),workspace:Boolean(document.querySelector('.provider-workspace'))})`);
    } catch (error) {
      if (!/Execution context was destroyed|Cannot find context with specified id/.test(String(error))) throw error;
      state = undefined;
    }
    if (state) {
      assert(!state.mockIpc, 'Native acceptance must not attach to mock IPC');
      assert(['null', 'http://tauri.localhost'].includes(state.origin), 'Unexpected native page origin');
      if (state.origin === 'http://tauri.localhost' && state.readyState === 'complete' && state.productionIpc && state.workspace) return state;
    }
    await delay(intervalMs);
  } while (Date.now() < deadline);
  throw new Error('Packaged native page did not become ready: ' + JSON.stringify(state));
}
export const sha256 = data => createHash('sha256').update(data).digest('hex');
export function decryptSync(file, passphrase) {
  assert(Buffer.isBuffer(file) && file.length >= 68 && file.length <= 8 * 1024 * 1024 + 68, 'Invalid sync size');
  assert.equal(file.subarray(0, 8).toString('ascii'), 'XYSYNC01');
  const key = pbkdf2Sync(passphrase, file.subarray(8, 40), 600_000, 32, 'sha256');
  try {
    const cipher = createDecipheriv('aes-256-gcm', key, file.subarray(40, 52));
    cipher.setAAD(file.subarray(0, 52));
    cipher.setAuthTag(file.subarray(-16));
    return JSON.parse(Buffer.concat([cipher.update(file.subarray(52, -16)), cipher.final()]).toString('utf8'));
  } finally { key.fill(0); }
}
export function safeFilename(name) {
  assert(typeof name === 'string' && name && basename(name) === name && !/[\\/:\x00-\x1f]/.test(name) && name !== '.' && name !== '..', 'Unsafe artifact filename');
  return name;
}
async function find(root, match, depth = 0) {
  assert(depth < 8, 'Unexpected artifact nesting');
  const results = [];
  for (const entry of await readdir(root, { withFileTypes: true })) {
    assert(!entry.isSymbolicLink(), 'Artifact links are not accepted');
    const path = join(root, entry.name);
    if (entry.isDirectory()) results.push(...await find(path, match, depth + 1));
    else if (entry.isFile() && match(entry.name)) results.push(path);
  }
  return results;
}
export async function packageInput(root, platform, version) {
  root = resolve(root);
  if (platform === 'windows-x64') {
    const matches = (await find(root, name => name === 'manifest.json')).filter(path => basename(resolve(path, '..')) === platform);
    assert.equal(matches.length, 1, 'Expected one Windows package manifest');
    const directory = resolve(matches[0], '..');
    const manifest = JSON.parse(await readFile(matches[0], 'utf8'));
    assert.equal(manifest.version, version);
    assert(Array.isArray(manifest.files) && manifest.files.length > 0);
    for (const entry of manifest.files) {
      const path = join(directory, safeFilename(entry.name));
      assert.equal((await stat(path)).size, entry.bytes);
      assert.equal(sha256(await readFile(path)), entry.sha256);
    }
    const nsis = manifest.files.filter(file => file.format === 'nsis');
    assert.equal(nsis.length, 1);
    const original = JSON.parse((await readFile(join(directory, 'native-nsis-payload-check.json'), 'utf8')).replace(/^\uFEFF/, ''));
    assert.match(original.installedSha256, /^[0-9a-f]{64}$/);
    return { directory, installer: join(directory, nsis[0].name), installerSha256: nsis[0].sha256, installedSha256: original.installedSha256, version, platform };
  }
  throw new Error('Unsupported native acceptance package: ' + platform);
}
