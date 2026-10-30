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
test('artifact extraction rejects traversal, links, duplicate paths and Windows aliases', async () => {
  const root = await mkdtemp(join(tmpdir(), 'xy-native-zip-'));
  const cli = fileURLToPath(new URL('./native-acceptance/extract-artifact.mjs', import.meta.url));
  const python = `import json,stat,sys,zipfile
with zipfile.ZipFile(sys.argv[1], 'w') as z:
 for name,mode in json.loads(sys.argv[2]):
  e=zipfile.ZipInfo();e.filename=name;e.external_attr=mode<<16;z.writestr(e,'fixture')`;
  const cases = [
    [['../outside.txt', 0]], [['C:/outside.txt', 0]], [['folder\\outside.txt', 0]],
    [['link', 0o120777]], [['duplicate', 0], ['DUPLICATE', 0]], [['file:stream', 0]],
    [['trailing.', 0]], [['trailing ', 0]], [['CON.txt', 0]], [['nested/LPT1', 0]],
    [['bad?name', 0]], [['bad\0name', 0]], [['a/./b', 0]], [['a//b', 0]],
    [['file', 0], ['file/child', 0]], [['/absolute', 0]],
  ];
  for (const [index, entries] of cases.entries()) {
    const archive = join(root, `bad-${index}.zip`), target = join(root, `bad-${index}`);
    const zip = spawnSync('python', ['-c', python, archive, JSON.stringify(entries)], { encoding: 'utf8' }); assert.equal(zip.status, 0, zip.stderr);
    const result = spawnSync(process.execPath, [cli, archive, target], { encoding: 'utf8' }); assert.notEqual(result.status, 0, `Unsafe case ${index}: ${JSON.stringify(entries)}\n${result.stdout}\n${result.stderr}`);
  }
  const archive = join(root, 'good.zip'), target = join(root, 'good');
  assert.equal(spawnSync('python', ['-c', python, archive, JSON.stringify([['nested/file.txt', 0]])]).status, 0);
  const result = spawnSync(process.execPath, [cli, archive, target], { encoding: 'utf8' }); assert.equal(result.status, 0, result.stderr);
  assert.equal(await readFile(join(target, 'nested/file.txt'), 'utf8'), 'fixture');
  assert.notEqual(spawnSync(process.execPath, [cli, archive, target]).status, 0, 'Existing extraction targets must not be overwritten');
});

test('Windows diagnostics retain the original failure and await detached NSIS removal', async () => {
  const script = await readFile(new URL('./native-acceptance/windows.ps1', import.meta.url), 'utf8');
  assert(script.includes('$report.error = $_.ToString()'));
  assert(script.includes('$report.cleanupError = $_.ToString()'));
  assert(script.includes('if (-not $primaryFailure) { $primaryFailure = $_ }'));
  assert(script.includes('$attempt -lt 30 -and (Test-Path -LiteralPath $exe)'));
  assert(script.indexOf('windows-diagnostics.ps1') < script.indexOf("Invoke-Installer (Join-Path $install 'uninstall.exe')"));
});

test('WebView debug policy is per-app, disposable and restored even after failure', async () => {
  const script = await readFile(new URL('./native-acceptance/windows.ps1', import.meta.url), 'utf8');
  assert(script.includes("@($config.identifier, 'xy-switch.exe')"));
  assert(script.includes('$debugPolicyBackups.Add($backup)'));
  assert(script.includes('$key.SetValue($backup.name, $backup.value, $backup.kind)'));
  assert(script.includes('Remove-ItemProperty -LiteralPath $debugPolicyPath -Name $backup.name'));
  assert(script.includes('$report.debugPolicyRestored = $true'));
  assert(script.includes('$report.passed -and $report.uninstalled -and $report.debugPolicyRestored'));
  assert(script.indexOf('requires a disposable GitHub-hosted Windows runner') < script.indexOf('New-Item -Path $debugPolicyPath'));
});
