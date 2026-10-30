import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { randomBytes, randomUUID } from 'node:crypto';
import { copyFile, mkdir, readFile, writeFile } from 'node:fs/promises';
import { join, resolve } from 'node:path';
import { parseArgs } from 'node:util';
import { connect } from '../workspace-qa/cdp.mjs';
import { decryptSync, requireDisposableRunner, sha256, waitForNativePage } from './lib.mjs';

requireDisposableRunner();
assert.equal(process.platform, 'win32');
const { values: args } = parseArgs({ options: Object.fromEntries(['endpoint', 'pid', 'output', 'files', 'database'].map(key => [key, { type: 'string' }])) });
for (const value of Object.values(args)) assert(value);
assert.match(args.pid, /^\d+$/);
const output = resolve(args.output), file = join(resolve(args.files), 'native-exchange.xysync');
await mkdir(output, { recursive: true });
const passphrase = randomBytes(32).toString('base64url'), wrongPassphrase = randomBytes(32).toString('base64url');
const scrub = value => String(value).replaceAll(passphrase, '[redacted]').replaceAll(wrongPassphrase, '[redacted]');
const reports = [];
let c, activeStep = 'connect';
const panel = 'document.querySelector(".encrypted-sync-settings")';
const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));
process.env.APP_URL = 'http://tauri.localhost';
const invoke = (command, payload = {}) => c.evaluate(`window.__TAURI_INTERNALS__.invoke(${JSON.stringify(command)},${JSON.stringify(payload)})`);
const button = label => c.button(label, panel);
const buttonExpr = label => `[...${panel}.querySelectorAll("button")].find(e=>e.textContent.trim()===${JSON.stringify(label)})`;
async function disabled(label, expected = true) { assert.equal(await c.evaluate(`Boolean(${buttonExpr(label)}?.disabled)`), expected, label); }
async function fill(label, value) {
  const id = await c.evaluate(`[...${panel}.querySelectorAll("label")].find(e=>e.textContent.trim()===${JSON.stringify(label)})?.htmlFor`);
  assert(id, `Missing native field: ${label}`);
  await c.fill(`[id=${JSON.stringify(id)}]`, value);
}
const confirm = word => fill('输入确认文字 · ' + word, word);
const message = value => c.waitFor(`${panel}?.innerText.includes(${JSON.stringify(value)})`, 30_000);
async function noSecrets() {
  assert.equal(await c.evaluate('[...document.querySelectorAll("input[type=password]")].every(e=>e.value==="")'), true, 'UI must clear passphrase inputs');
  const storage = await c.evaluate('JSON.stringify({local:Object.entries(localStorage),session:Object.entries(sessionStorage)})');
  for (const secret of [passphrase, wrongPassphrase]) assert(!storage.includes(secret), 'Passphrase persisted in WebView storage');
  for (const suffix of ['', '-wal', '-journal']) {
    const data = await readFile(args.database + suffix).catch(error => { if (error.code === 'ENOENT') return Buffer.alloc(0); throw error; });
    for (const secret of [passphrase, wrongPassphrase]) assert(!data.includes(Buffer.from(secret)), 'Passphrase persisted in SQLite');
  }
}
async function shot(name) { await c.screenshot(join(output, name + '.png')); }
async function step(name, work) {
  activeStep = name; console.log('RUN ' + name);
  const started = Date.now();
  await work(); await noSecrets();
  reports.push({ name, passed: true, elapsedMs: Date.now() - started }); console.log('PASS ' + name);
}
async function dialog(operation, name, clickLabel) {
  const argv = ['-NoProfile', '-NonInteractive', '-STA', '-ExecutionPolicy', 'Bypass', '-File', resolve('scripts/native-acceptance/windows-dialog.ps1'), '-AppProcessId', args.pid, '-Operation', operation, '-OutputPrefix', join(output, name)];
  if (operation !== 'cancel') argv.push('-SelectedPath', file);
  const worker = spawn('powershell.exe', argv, { windowsHide: true, stdio: ['ignore', 'pipe', 'pipe'] });
  let diagnostic = '';
  worker.stdout.on('data', chunk => { diagnostic += chunk; }); worker.stderr.on('data', chunk => { diagnostic += chunk; });
  const done = new Promise((resolve, reject) => {
    const timer = setTimeout(() => { worker.kill(); reject(new Error('Native dialog worker timed out')); }, 120_000);
    worker.on('error', error => { clearTimeout(timer); reject(error); });
    worker.on('close', code => { clearTimeout(timer); code === 0 ? resolve() : reject(new Error(`Native dialog worker exited ${code}: ${diagnostic}`)); });
  });
  done.catch(() => {});
  try { await button(clickLabel); await done; }
  finally { if (worker.exitCode === null) worker.kill(); }
  const evidence = JSON.parse((await readFile(join(output, name + '.json'), 'utf8')).replace(/^\uFEFF/, ''));
  assert.equal(evidence.processId, Number(args.pid)); assert.equal(evidence.class, '#32770'); assert.equal(evidence.completed, true);
  assert.equal(evidence.screenshotCaptured, true); assert(evidence.dialogHandle && evidence.buttonHandle);
  assert(['uia-invoke-pattern', 'win32-bm-click'].includes(evidence.actionMethod));
  if (operation !== 'cancel') { assert.equal(evidence.fileNameVerified, true); assert(evidence.editHandle); }
}
const sourceProviders = [0, 1].map(index => ({
  id: randomUUID(), name: `Native source ${index}`, providerType: 'custom', appGroup: 'Codex',
  endpoint: `https://source-${index}.sync-fixture.invalid/v1`, model: `qa-native-model-${index}`,
  enabled: false, sortOrder: 200 + index,
  advanced: { headerOverrides: { 'X-Native-Fixture': 'LOCAL-ONLY-SOURCE-HEADER' }, bodyOverrides: { fixtureNote: 'LOCAL-ONLY-SOURCE-BODY' } },
}));
const favorite = { providerId: 'codex', sessionId: 'qa-native-sync-session' };
const price = { id: 'qa-native-model-0', inputPerMillion: 1.25, outputPerMillion: 4.5, cacheReadPerMillion: 0.125, cacheCreationPerMillion: null };
const keepPrice = { ...price, id: 'qa-native-keep-local', inputPerMillion: 7 };
const priceFields = value => Object.fromEntries(Object.keys(price).map(key => [key, value[key]]));
async function state() {
  const [providers, prices, favorites, backups] = await Promise.all([
    invoke('list_providers'), invoke('list_model_pricing'), invoke('load_session_favorites', { legacy: null, legacyAvailable: true }), invoke('list_sync_backups'),
  ]);
  return {
    providers: providers.filter(p => p.name.startsWith('Native ')).sort((a, b) => a.id.localeCompare(b.id)),
    prices: prices.filter(p => p.id.startsWith('qa-native-')).sort((a, b) => a.id.localeCompare(b.id)),
    favorites, backups,
  };
}
let before, after, exported;
try {
  for (let attempt = 0; attempt < 60 && !c; attempt++) {
    try { c = await connect(args.endpoint); } catch (error) { if (attempt === 59) throw error; await sleep(500); }
  }
  const connection = await waitForNativePage(c);
  await writeFile(join(output, 'native-page-connection.json'), JSON.stringify({ ...connection, processId: Number(args.pid) }, null, 2));
  assert.equal(await c.evaluate('location.origin'), 'http://tauri.localhost');
  assert.equal(await c.evaluate('Boolean(window.__TAURI_INTERNALS__?.invoke) && !window.__XY_QA__'), true);
  const platform = await invoke('app_platform');
  assert.equal(platform.os, 'windows'); assert.equal(platform.mobile, false);
  await invoke('set_interface_language', { preference: 'zh-CN', resolvedLanguage: 'zh-CN' });
  await c.evaluate('localStorage.setItem("xy-switch:language","zh-CN")');
  await c.call('Page.reload');
  await c.waitFor('document.documentElement.lang === "zh-CN" && Boolean(document.querySelector(".provider-workspace"))', 60_000);
  await step('production-ipc-and-synthetic-fixture', async () => {
    await invoke('load_session_favorites', { legacy: null, legacyAvailable: true });
    for (const provider of sourceProviders) await invoke('save_provider', { provider });
    await invoke('set_session_favorite', { favorite, enabled: true });
    for (const pricing of [price, keepPrice]) await invoke('save_model_pricing', { pricing });
    await c.click('.xy-shell-nav-item[title="设置"]'); await c.click('#settings-tab-sync');
    await c.waitFor(`Boolean(${buttonExpr('预览加密导出')}) && !${buttonExpr('预览加密导出')}.disabled`);
  });
  await step('native-save-picker-and-independent-decryption', async () => {
    await button('预览加密导出'); await message('加密导出预览');
    await disabled('确认并另存加密文件');
    const previewText = await c.evaluate(panel + '.textContent');
    assert(previewText.includes('qa-native-model-0')); assert(!previewText.includes('LOCAL-ONLY-'));
    await shot('export-preview');
    await fill('加密口令', passphrase); await fill('再次输入加密口令', passphrase);
    await confirm('EXPORT'); await disabled('确认并另存加密文件');
    await c.click('.encrypted-sync-settings .gov-check input'); await disabled('确认并另存加密文件', false);
    await dialog('save', 'native-save-dialog', '确认并另存加密文件');
    await message('加密文件已保存');
    const data = await readFile(file); exported = decryptSync(data, passphrase);
    assert.deepEqual(Object.keys(exported).sort(), ['favorites', 'prices', 'providers', 'schema']); assert.equal(exported.schema, 1);
    assert(!data.includes(Buffer.from('qa-native-model-0')) && !data.includes(Buffer.from('Native source')));
    assert.deepEqual(exported.favorites, [favorite]);
    assert.deepEqual(exported.prices.find(p => p.id === price.id), price);
    for (const provider of sourceProviders) {
      const incoming = exported.providers.find(p => p.name === provider.name); assert(incoming);
      assert.notEqual(incoming.id, provider.id, 'Sync identity must not reuse a credential ID');
      assert.deepEqual(Object.keys(incoming).sort(), ['appGroup', 'endpoint', 'id', 'isFullUrl', 'model', 'modelMappings', 'name', 'providerType', 'upstreamFormat']);
    }
    assert(!JSON.stringify(exported).includes('LOCAL-ONLY-'));
    assert.throws(() => decryptSync(data, wrongPassphrase));
    const corrupted = Buffer.from(data); corrupted[corrupted.length - 1] ^= 1; assert.throws(() => decryptSync(corrupted, passphrase));
    await copyFile(file, join(output, 'native-exchange.xysync'));
    await writeFile(join(output, 'encryption-verification.json'), JSON.stringify({ format: 'XYSYNC01', algorithm: 'AES-256-GCM', kdf: 'PBKDF2-HMAC-SHA256', iterations: 600_000, bytes: data.length, sha256: sha256(data), independentlyDecrypted: true, wrongPassphraseRejected: true, tamperedTagRejected: true, whitelistVerified: true, distinctSyncIdentities: true, providers: exported.providers.length, favorites: exported.favorites.length, prices: exported.prices.length, passphrasePersisted: false }, null, 2));
  });
  await step('destination-local-conflicts-and-picker-cancel', async () => {
    for (const [index, source] of sourceProviders.entries()) {
      const provider = { ...source, name: `Native local ${index}`, model: `qa-native-local-${index}`, advanced: { ...source.advanced, headerOverrides: { 'X-Native-Fixture': 'LOCAL-ONLY-DESTINATION-HEADER' } } };
      if (index === 1) provider.endpoint = 'https://destination.sync-fixture.invalid/v1';
      await invoke('save_provider', { provider });
    }
    await invoke('set_session_favorite', { favorite, enabled: false });
    await invoke('save_model_pricing', { pricing: { ...price, inputPerMillion: 3 } });
    await invoke('save_model_pricing', { pricing: { ...keepPrice, inputPerMillion: 9 } });
    before = await state(); assert.equal(before.backups.length, 0);
    await fill('加密口令', passphrase);
    await dialog('cancel', 'native-cancel-dialog', '选择加密文件并预览'); await message('已取消文件选择');
    assert.deepEqual(await state(), before);
  });
  await step('native-open-picker-wrong-passphrase-no-write', async () => {
    await fill('加密口令', wrongPassphrase);
    await dialog('open', 'native-wrong-passphrase-dialog', '选择加密文件并预览');
    await message('Wrong passphrase or damaged encrypted file');
    assert.deepEqual(await state(), before); assert.equal(await c.evaluate('document.querySelectorAll(".sync-change").length'), 0);
    await shot('wrong-passphrase');
  });
  await step('native-import-preview-explicit-choices-and-backup', async () => {
    await fill('加密口令', passphrase);
    await dialog('open', 'native-import-dialog', '选择加密文件并预览');
    await c.waitFor('document.querySelectorAll(".sync-change select").length === 5', 30_000);
    assert.deepEqual(await state(), before, 'A preview must not apply imported data');
    await confirm('IMPORT'); await c.click('.encrypted-sync-settings .gov-check input');
    await disabled('确认合并所选变更');
    const selects = await c.evaluate('[...document.querySelectorAll(".sync-change select")].map(e=>({id:e.id,label:e.closest(".sync-change").textContent}))');
    for (let index = 0; index < selects.length; index++) {
      const row = selects[index];
      await c.fill(`[id=${JSON.stringify(row.id)}]`, row.label.includes('qa-native-keep-local') ? 'local' : 'incoming');
      if (index < selects.length - 1) await disabled('确认合并所选变更');
    }
    await disabled('确认合并所选变更', false); await shot('import-choices');
    await button('确认合并所选变更'); await message('已合并 4 项');
    after = await state(); assert.equal(after.backups.length, 1); assert.equal(after.backups[0].used, false);
    const merged = after.providers.find(p => p.id === sourceProviders[0].id);
    assert.equal(merged.name, sourceProviders[0].name); assert.equal(merged.model, sourceProviders[0].model);
    assert.deepEqual(merged.advanced, before.providers.find(p => p.id === merged.id).advanced);
    const retained = after.providers.find(p => p.id === sourceProviders[1].id);
    assert.deepEqual(retained, before.providers.find(p => p.id === retained.id));
    const copy = after.providers.find(p => !sourceProviders.some(source => source.id === p.id));
    assert(copy && !copy.enabled); assert.equal(copy.endpoint, sourceProviders[1].endpoint);
    assert(!JSON.stringify(copy.advanced).includes('LOCAL-ONLY-')); assert.notEqual(copy.id, exported.providers.find(p => p.name === sourceProviders[1].name).id);
    assert.deepEqual(after.favorites, [favorite]);
    assert.deepEqual(priceFields(after.prices.find(p => p.id === price.id)), price);
    assert.equal(after.prices.find(p => p.id === keepPrice.id).inputPerMillion, 9);
    await shot('import-complete');
  });
  await step('native-rollback-preview-and-persisted-restoration', async () => {
    await button('预览同步回滚'); await message('ROLLBACK SYNC');
    assert.deepEqual(await state(), after); await disabled('确认回滚同步');
    await confirm('ROLLBACK SYNC'); await c.click('.encrypted-sync-settings .gov-check input');
    await disabled('确认回滚同步', false); await shot('rollback-preview');
    await button('确认回滚同步'); await message('同步变更已回滚');
    const restored = await state();
    assert.deepEqual(restored.providers.filter(p => sourceProviders.some(s => s.id === p.id)), before.providers);
    assert.deepEqual(restored.prices, before.prices); assert.deepEqual(restored.favorites, before.favorites);
    const retainedCopies = restored.providers.filter(p => !sourceProviders.some(s => s.id === p.id));
    assert.equal(retainedCopies.length, 1); assert.equal(retainedCopies[0].enabled, false);
    assert.equal(restored.backups[0].used, true);
    await writeFile(join(output, 'native-state-verification.json'), JSON.stringify({ before, afterImport: after, afterRollback: restored }, null, 2));
    await shot('rollback-complete');
  });
  assert.equal(c.exceptions.length, 0, 'Unhandled exceptions in the real native WebView');
} catch (error) {
  reports.push({ name: activeStep, passed: false, error: scrub(error.stack ?? error) });
  if (c) {
    await shot('failure').catch(() => {});
    const snapshot = await c.evaluate('({url:location.href,origin:location.origin,readyState:document.readyState,text:document.body?.innerText,controls:[...document.querySelectorAll("button,input,select")].map(e=>({tag:e.tagName,label:e.getAttribute("aria-label"),text:e.textContent.slice(0,160),id:e.id,disabled:e.disabled}))})').catch(() => null);
    await writeFile(join(output, 'failure.json'), scrub(JSON.stringify({ snapshot, exceptions: c.exceptions }, null, 2)));
  }
  console.error(scrub(error.stack ?? error)); process.exitCode = 1;
} finally {
  await writeFile(join(output, 'results.json'), scrub(JSON.stringify({ passed: process.exitCode !== 1, sourceCommit: process.env.NATIVE_SOURCE_COMMIT, harnessCommit: process.env.GITHUB_SHA, sourceRun: process.env.NATIVE_SOURCE_RUN, platform: 'windows', reports, limitations: ['Synthetic records; no real provider credentials or paid API calls', 'Not Linux/macOS native UI acceptance', 'Test-only CDP on a disposable child; released packages are unchanged'] }, null, 2)));
  if (c) await c.close();
}
