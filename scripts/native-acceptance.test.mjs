import assert from 'node:assert/strict';
import test from 'node:test';
import { createCipheriv, pbkdf2Sync, randomBytes } from 'node:crypto';
import { mkdtemp, mkdir, readFile, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { decryptSync, packageInput, requireDisposableRunner, safeFilename, sha256, waitForNativePage } from './native-acceptance/lib.mjs';

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
  assert(script.includes("$debugPolicyPath = 'HKLM:"));
  assert(script.includes('WindowsBuiltInRole]::Administrator'));
  assert(script.includes('if (-not $debugPolicyExisted) { New-Item -Path $debugPolicyPath'));
  assert(!/Name ['"]\*['"]/.test(script));
  assert(script.includes('$debugPolicyBackups.Add($backup)'));
  assert(script.includes('$key.SetValue($backup.name, $backup.value, $backup.kind)'));
  assert(script.includes('Remove-ItemProperty -LiteralPath $debugPolicyPath -Name $backup.name'));
  assert(script.includes('$report.debugPolicyRestored = $true'));
  assert(script.includes('$report.passed -and $report.uninstalled -and $report.debugPolicyRestored'));
  assert(script.indexOf('requires a disposable GitHub-hosted Windows runner') < script.indexOf('New-Item -Path $debugPolicyPath'));
});


const readyPage = { url: 'http://tauri.localhost/', origin: 'http://tauri.localhost', readyState: 'complete', productionIpc: true, mockIpc: false, workspace: true };
const blankPage = { ...readyPage, url: 'about:blank', origin: 'null', productionIpc: false, workspace: false };
test('native page readiness waits for real navigation and the rendered workspace', async () => {
  const states = [blankPage, { ...readyPage, workspace: false }, readyPage];
  let calls = 0;
  const client = { evaluate: async () => states[calls++] };
  assert.deepEqual(await waitForNativePage(client, { timeoutMs: 1000, intervalMs: 1 }), readyPage);
  assert.equal(calls, 3);
});
test('native page readiness tolerates only navigation context replacement', async () => {
  let calls = 0;
  const client = { evaluate: async () => { if (!calls++) throw new Error('Execution context was destroyed.'); return readyPage; } };
  assert.deepEqual(await waitForNativePage(client, { timeoutMs: 1000, intervalMs: 1 }), readyPage);
  await assert.rejects(waitForNativePage({ evaluate: async () => { throw new Error('Target closed'); } }), /Target closed/);
});
test('native page readiness rejects mock bridges and unrelated origins', async () => {
  await assert.rejects(waitForNativePage({ evaluate: async () => ({ ...readyPage, mockIpc: true }) }), /must not attach to mock IPC/);
  await assert.rejects(waitForNativePage({ evaluate: async () => ({ ...readyPage, origin: 'https://example.invalid' }) }), /Unexpected native page origin/);
});
test('native page readiness times out rather than navigating or fabricating IPC', async () => {
  await assert.rejects(waitForNativePage({ evaluate: async () => blankPage }, { timeoutMs: 10, intervalMs: 1 }), /did not become ready/);
  const script = await readFile(new URL('./native-acceptance/windows-sync.mjs', import.meta.url), 'utf8');
  assert(script.indexOf('await waitForNativePage(c)') < script.indexOf("invoke('app_platform')"));
});

test('native dialog waits for its PID-bound controls and retains failure evidence', async () => {
  const script = await readFile(new URL('./native-acceptance/windows-dialog.ps1', import.meta.url), 'utf8');
  const loop = script.slice(script.indexOf('while ([DateTime]::UtcNow -lt $deadline)'), script.indexOf('$report.controlsWaitMs'));
  for (const required of ['$uia::RootElement.FindFirst($children, $condition)', '$dialog.FindAll', '$report.controlAttempts++', '$editReady -and $buttonReady', 'TryGetCurrentPattern', 'Start-Sleep -Milliseconds 150']) assert(loop.includes(required), required);
  assert(script.includes('$uia::ProcessIdProperty, $AppProcessId'));
  assert(script.includes("$uia::ClassNameProperty, '#32770'"));
  assert(script.includes("if (-not $ready) { throw 'Native filename/action controls did not become ready"));
  assert(script.indexOf('if (-not $ready)') < script.indexOf(').SetValue($SelectedPath)'));
  assert(script.includes('if ($dialog -and -not $report.screenshotCaptured)'));
  assert(script.includes('$report.error = $_.Exception.Message'));
  assert(!/SendKeys|SendInput|SetCursorPos/.test(script));
});

test('native dialog handles UIA Pane proxies through exact Win32 control identities', async () => {
  const script = await readFile(new URL('./native-acceptance/windows-dialog.ps1', import.meta.url), 'utf8');
  const native = await readFile(new URL('./native-acceptance/windows-dialog-native.cs', import.meta.url), 'utf8');
  assert(script.includes("$_.Current.ClassName -eq 'Edit' -and $_.Current.AutomationId -eq '1001'"));
  assert(script.includes("$_.Current.ClassName -eq 'Button' -and $_.Current.AutomationId -eq $buttonId"));
  assert(!script.includes('$_.Current.ControlType -eq'));
  for (const required of ['ProcessId(dialog) == processId', 'ClassName(dialog) == "#32770"', 'IsChild(dialog, control)', 'ProcessId(control) == processId', 'ClassName(control) == className', 'GetDlgCtrlID(control) == controlId', 'IsWindowVisible(control)', 'IsWindowEnabled(control)', 'SendMessageTimeoutW', '5000', 'controlId != 1 && controlId != 2']) assert(native.includes(required), required);
  assert(script.includes('::ReadFileName($dialogHandle, $editHandle, $AppProcessId) -cne $SelectedPath'));
  assert(script.includes('$report.fileNameVerified = $true'));
  assert(script.includes("$report.actionMethod = 'win32-bm-click'"));
  assert(!/SendKeys|SendInput|SetCursorPos/.test(native));
});

test('native filename entry notifies the shell via actual edit replacement', async () => {
  const native = await readFile(new URL('./native-acceptance/windows-dialog-native.cs', import.meta.url), 'utf8');
  const script = await readFile(new URL('./native-acceptance/windows-dialog.ps1', import.meta.url), 'utf8');
  assert(native.includes('EditMessage(edit, 0x00B1, UIntPtr.Zero, new IntPtr(-1), 2, 5000'));
  assert(native.includes('SetTextMessage(edit, 0x00C2, new UIntPtr(1), path, 2, 5000'));
  assert(!native.includes('SetTextMessage(edit, 0x000C'));
  assert(script.includes("$report.editMethod = 'win32-em-replacesel'"));
  assert(script.indexOf('::ReadFileName') < script.indexOf('::ClickAction'));
});
