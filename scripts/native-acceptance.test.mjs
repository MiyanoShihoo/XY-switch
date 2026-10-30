import assert from 'node:assert/strict';
import test from 'node:test';
import { createCipheriv, pbkdf2Sync, randomBytes } from 'node:crypto';
import { mkdtemp, mkdir, readFile, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { decryptSync, packageInput, requireDisposableRunner, safeFilename, sha256 } from './native-acceptance/lib.mjs';

const secret = 'synthetic native test passphrase';
function encrypt(payload) {
  const header = Buffer.concat([Buffer.from('XYSYNC01'), randomBytes(32), randomBytes(12)]);
  const key = pbkdf2Sync(secret, header.subarray(8, 40), 600_000, 32, 'sha256');
  const cipher = createCipheriv('aes-256-gcm', key, header.subarray(40)); cipher.setAAD(header);
  return Buffer.concat([header, cipher.update(JSON.stringify(payload)), cipher.final(), cipher.getAuthTag()]);
}
test('native harness refuses ordinary workstations and self-hosted CI', () => {
  assert.throws(() => requireDisposableRunner('win32', {}));
  assert.throws(() => requireDisposableRunner('win32', { GITHUB_ACTIONS: 'true', RUNNER_ENVIRONMENT: 'self-hosted' }));
  assert.throws(() => requireDisposableRunner('linux', { GITHUB_ACTIONS: 'true', RUNNER_ENVIRONMENT: 'github-hosted' }));
  requireDisposableRunner('win32', { GITHUB_ACTIONS: 'true', RUNNER_ENVIRONMENT: 'github-hosted' });
});
test('independent Node decryption authenticates the production envelope header and tag', () => {
  const payload = { schema: 1, providers: [], favorites: [], prices: [] }, data = encrypt(payload);
  assert.deepEqual(decryptSync(data, secret), payload);
  assert.throws(() => decryptSync(data, secret + '-wrong'));
  for (const offset of [8, 40, 52, data.length - 1]) {
    const tampered = Buffer.from(data); tampered[offset] ^= 1;
    assert.throws(() => decryptSync(tampered, secret));
  }
});
test('envelope size and magic are checked before expensive KDF', () => {
  for (const data of [Buffer.alloc(67), Buffer.alloc(68), Buffer.alloc(8 * 1024 * 1024 + 69)]) assert.throws(() => decryptSync(data, secret));
});
test('cross-platform package filenames cannot escape the artifact directory', () => {
  for (const name of ['../x', '..\\x', 'C:\\x', '/x', '..', 'x:y', 'a\0b', '']) assert.throws(() => safeFilename(name));
  assert.equal(safeFilename('XY-Switch-1.2.0.exe'), 'XY-Switch-1.2.0.exe');
});
test('simulator package requires an exact checksum, not just an archive filename', async () => {
  const root = await mkdtemp(join(tmpdir(), 'xy-native-package-'));
  const path = join(root, '1.2.0', 'ios-arm64-simulator'); await mkdir(path, { recursive: true });
  const name = 'XY-Switch-1.2.0-ios-arm64-simulator.zip', data = Buffer.from('synthetic archive');
  await writeFile(join(path, name), data); await writeFile(join(path, 'SHA256SUMS.txt'), `${sha256(data)}  ${name}\n`);
  assert.equal((await packageInput(root, 'ios-arm64-simulator', '1.2.0')).archiveSha256, sha256(data));
  await writeFile(join(path, name), Buffer.from('tampered'));
  await assert.rejects(packageInput(root, 'ios-arm64-simulator', '1.2.0'));
});
test('Windows installer verification binds the package and previously installed payload', async () => {
  const root = await mkdtemp(join(tmpdir(), 'xy-native-windows-')), path = join(root, 'windows-x64'); await mkdir(path);
  const data = Buffer.from('synthetic installer'), name = 'setup.exe';
  await writeFile(join(path, name), data);
  await writeFile(join(path, 'manifest.json'), JSON.stringify({ version: '1.2.0', files: [{ name, format: 'nsis', bytes: data.length, sha256: sha256(data) }] }));
  await writeFile(join(path, 'native-nsis-payload-check.json'), '\uFEFF' + JSON.stringify({ installedSha256: 'a'.repeat(64) }));
  assert.equal((await packageInput(root, 'windows-x64', '1.2.0')).installedSha256, 'a'.repeat(64));
  await assert.rejects(packageInput(root, 'windows-x64', '1.2.1'));
  await writeFile(join(path, name), Buffer.from('modified installer')); await assert.rejects(packageInput(root, 'windows-x64', '1.2.0'));
});
test('standalone XCTest generation does not alter or rebuild the tested application', async () => {
  const root = await mkdtemp(join(tmpdir(), 'xy-native-xcode-'));
  const cli = fileURLToPath(new URL('./native-acceptance/ios-project.mjs', import.meta.url));
  const result = spawnSync(process.execPath, [cli, root], { encoding: 'utf8' }); assert.equal(result.status, 0, result.stderr);
  const project = await readFile(join(root, 'NativeAcceptance.xcodeproj/project.pbxproj'), 'utf8');
  assert(project.includes('com.apple.product-type.bundle.ui-testing')); assert(project.includes('CODE_SIGNING_ALLOWED = NO'));
  const swift = await readFile(join(root, 'NativeAcceptanceTests.swift'), 'utf8');
  assert(swift.includes('XCUIApplication(bundleIdentifier: "com.xyswitch.desktop")'));
  assert(swift.includes('app.terminate(); app.launch()')); assert(swift.includes('"175"'));
  assert(!project.includes('src-tauri'));
});
test('iOS acceptance cannot succeed solely on a nonblank loading screenshot', async () => {
  const script = await readFile(new URL('./smoke-ios-simulator.sh', import.meta.url), 'utf8');
  assert(script.includes('xcodebuild test')); assert(script.includes('s.passedTests>=1')); assert(script.includes("preferences['requestTimeoutSecs'] == 175"));
  assert(!script.includes('rendered=true')); assert(script.indexOf('xcodebuild test') < script.indexOf('verify-native-screenshot.mjs'));
});
