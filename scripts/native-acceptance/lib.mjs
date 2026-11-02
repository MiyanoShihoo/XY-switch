import assert from 'node:assert/strict';
import { createDecipheriv, createHash, pbkdf2Sync } from 'node:crypto';
import { readdir, readFile, stat } from 'node:fs/promises';
import { basename, join, resolve, win32 } from 'node:path';
import { setTimeout as delay } from 'node:timers/promises';

export function requireDisposableRunner(platform = process.platform, env = process.env) {
  assert.equal(env.GITHUB_ACTIONS, 'true', 'Native acceptance requires GitHub Actions');
  assert.equal(env.RUNNER_ENVIRONMENT, 'github-hosted', 'Native acceptance requires a disposable GitHub-hosted runner');
  assert.equal(platform, 'win32', 'This native acceptance harness requires Windows');
}
export const nativeArtifactNames = Object.freeze({ windows: 'xy-switch-windows-X64', linux: 'xy-switch-linux-X64', macos: 'xy-switch-macos-universal' });
export function msiInstalledExecutable(record, { requestedDirectory, programFiles, previousDirectory }) {
  for (const name of ['productCode', 'componentId']) assert.match(record[name] ?? '', /^\{[0-9a-f]{8}(-[0-9a-f]{4}){3}-[0-9a-f]{12}\}$/i);
  assert.match(record.installedPath ?? '', /^[a-z]:\\/i, 'MSI must register an absolute local component path');
  assert(typeof requestedDirectory === 'string' && typeof programFiles === 'string');
  const executable = win32.normalize(record.installedPath);
  assert.equal(win32.basename(executable).toLowerCase(), 'xy-switch.exe');
  const allowed = [win32.resolve(requestedDirectory), win32.resolve(programFiles, 'XY Switch')].map(directory => directory.toLowerCase());
  if (previousDirectory !== undefined) {
    assert.match(previousDirectory, /^[a-z]:\\/i);
    assert.equal(win32.dirname(win32.resolve(previousDirectory)).toLowerCase(), win32.dirname(win32.resolve(requestedDirectory)).toLowerCase(), 'Prior installer directory must belong to the same disposable task');
    allowed.push(win32.resolve(previousDirectory).toLowerCase());
  }
  assert(allowed.includes(win32.dirname(executable).toLowerCase()), 'MSI component escaped its requested or default application directory');
  return executable;
}
export function selectNativeArtifacts(artifacts) {
  assert(Array.isArray(artifacts));
  return Object.fromEntries(Object.entries(nativeArtifactNames).map(([platform, name]) => {
    const matches = artifacts.filter(artifact => artifact.name === name && !artifact.expired);
    assert.equal(matches.length, 1, `Expected one unexpired ${name}`);
    const artifact = matches[0];
    assert(Number.isSafeInteger(artifact.id) && artifact.id > 0);
    assert.match(artifact.digest ?? '', /^sha256:[0-9a-f]{64}$/);
    assert(Number.isSafeInteger(artifact.size_in_bytes) && artifact.size_in_bytes > 0);
    return [platform, { id: artifact.id, name, digest: artifact.digest, size: artifact.size_in_bytes }];
  }));
}
export async function setNativeInterfaceLanguage(client) {
  const languageSelector = '#settings-panel-general select:has(option[value="zh-CN"])';
  await client.waitFor('Boolean(document.querySelector(".xy-shell-nav-item[data-view=settings]"))');
  await client.click('.xy-shell-nav-item[data-view="settings"]');
  await client.waitFor('Boolean(document.querySelector("#settings-tab-general"))');
  await client.click('#settings-tab-general');
  await client.waitFor('Boolean(document.querySelector(' + JSON.stringify(languageSelector) + '))');
  const current = await client.evaluate('document.querySelector(' + JSON.stringify(languageSelector) + ').value');
  const selected = language => 'document.querySelector(' + JSON.stringify(languageSelector) + ')?.value === ' + JSON.stringify(language) + ' && !document.querySelector(' + JSON.stringify(languageSelector) + ')?.disabled && document.documentElement.lang === ' + JSON.stringify(language);
  if (current === 'zh-CN') {
    await client.fill(languageSelector, 'en');
    await client.waitFor(selected('en'), 60_000);
  }
  await client.fill(languageSelector, 'zh-CN');
  await client.waitFor(selected('zh-CN'), 60_000);
  assert.equal(await client.evaluate('window.__TAURI_INTERNALS__.invoke("get_interface_language")'), 'zh-CN');
  await client.click('.xy-shell-nav-item[data-view="providers"]');
  await client.waitFor('document.documentElement.lang === "zh-CN" && Boolean(document.querySelector(".provider-workspace"))', 60_000);
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
  if (['windows-x64', 'linux-x64', 'macos-universal'].includes(platform)) {
    const matches = (await find(root, name => name === 'manifest.json')).filter(path => basename(resolve(path, '..')) === platform);
    assert.equal(matches.length, 1, `Expected one ${platform} package manifest`);
    const directory = resolve(matches[0], '..');
    const manifest = JSON.parse(await readFile(matches[0], 'utf8'));
    assert.equal(manifest.version, version);
    assert(Array.isArray(manifest.files) && manifest.files.length > 0);
    const names = new Set();
    for (const entry of manifest.files) {
      const path = join(directory, safeFilename(entry.name));
      assert(!names.has(entry.name.toLowerCase()), 'Duplicate manifest filename');
      names.add(entry.name.toLowerCase());
      assert(Number.isSafeInteger(entry.bytes) && entry.bytes > 0, 'Invalid manifest file size');
      assert.match(entry.sha256 ?? '', /^[0-9a-f]{64}$/, 'Invalid manifest SHA256');
      assert.equal((await stat(path)).size, entry.bytes);
      assert.equal(sha256(await readFile(path)), entry.sha256);
    }
    const result = { directory, version, platform, files: manifest.files.map(file => ({ ...file, path: join(directory, file.name) })) };
    if (platform === 'windows-x64') {
      const nsis = result.files.filter(file => file.format === 'nsis');
      assert.equal(nsis.length, 1);
      const installedSha256ByFormat = {};
      for (const file of result.files.filter(file => ['nsis', 'msi'].includes(file.format))) {
        assert(!Object.hasOwn(installedSha256ByFormat, file.format), 'Duplicate Windows installer format');
        const original = JSON.parse((await readFile(join(directory, 'native-' + file.format + '-payload-check.json'), 'utf8')).replace(/^\uFEFF/, ''));
        assert.match(original.installedSha256, /^[0-9a-f]{64}$/);
        installedSha256ByFormat[file.format] = original.installedSha256;
      }
      Object.assign(result, { installer: nsis[0].path, installerSha256: nsis[0].sha256, installedSha256: installedSha256ByFormat.nsis, installedSha256ByFormat });
    }
    return result;
  }
  throw new Error('Unsupported native acceptance package: ' + platform);
}
