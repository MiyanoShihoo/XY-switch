import assert from 'node:assert/strict';
import { readFile, writeFile, mkdir } from 'node:fs/promises';
import { resolve, join } from 'node:path';
import { connect } from './workspace-qa/cdp.mjs';

const base = process.env.APP_URL ?? 'http://127.0.0.1:1431';
if (!['127.0.0.1', 'localhost', '[::1]'].includes(new URL(base).hostname)) throw Error('Sync QA must use a local renderer');
const output = resolve(process.env.QA_OUTPUT ?? 'artifacts/encrypted-sync-qa/ui-' + new Date().toISOString().replace(/[:.]/g, '-'));
await mkdir(output, { recursive: true });
const password = 'fixture long sync passphrase', storageKey = 'xy-switch.session-favorites.v1';
const legacy = JSON.stringify({ version: 1, keys: [JSON.stringify(['codex', 'qa-session'])] });
const source = "localStorage.setItem('xy-qa:native-language',localStorage.getItem('xy-sync-qa:language')||'zh-CN');\n"
  + await readFile(new URL('./workspace-qa/fixture.js', import.meta.url), 'utf8') + '\n'
  + await readFile(new URL('./workspace-qa/encrypted-sync-fixture.js', import.meta.url), 'utf8');
const c = await connect(undefined, { fresh: true }), reports = [], only = process.env.QA_ONLY?.split(',');
const page = 'document.querySelector(".encrypted-sync-settings")';
const click = label => c.button(label, page);
const text = () => c.evaluate(page + '.innerText');
const calls = command => c.evaluate('window.__XY_QA__.calls.filter(c=>c.command===' + JSON.stringify(command) + ')');
const button = label => '[...' + page + '.querySelectorAll("button")].find(b=>b.textContent.trim()===' + JSON.stringify(label) + ')';
const disabled = async (label, expected = true) => assert.equal(await c.evaluate('Boolean(' + button(label) + '?.disabled)'), expected, label);
async function field(label) {
  const id = await c.evaluate('[...' + page + '.querySelectorAll("label")].find(e=>e.textContent.trim()===' + JSON.stringify(label) + ')?.htmlFor');
  assert(id, 'Missing field: ' + label); return '[id=' + JSON.stringify(id) + ']';
}
const fill = async (label, value) => c.fill(await field(label), value);
const confirm = (word, value = word) => fill('输入确认文字 · ' + word, value);
async function review() { await c.click('.encrypted-sync-settings .gov-check input'); }
async function noOverflow() { const m = await c.evaluate('({width:innerWidth,root:document.documentElement.scrollWidth,body:document.body.scrollWidth})'); assert(m.root <= m.width && m.body <= m.width, JSON.stringify(m)); }
async function noSecrets() {
  const values = await c.evaluate('JSON.stringify({storage:Object.entries(localStorage),calls:window.__XY_QA__.calls})');
  assert(!values.includes(password), 'Passphrase leaked into storage or QA calls');
  assert.equal((await calls('set_provider_api_key')).length, 0);
}
async function reset({ os = 'windows', running = false, language = 'zh-CN', width = 1280, theme = 'light' } = {}) {
  await c.evaluate('localStorage.setItem("xy-sync-qa:os",' + JSON.stringify(os) + ');localStorage.setItem("xy-sync-qa:running",' + JSON.stringify(String(running)) + ');localStorage.setItem("xy-sync-qa:language",' + JSON.stringify(language) + ');localStorage.setItem("xy-switch:language",' + JSON.stringify(language) + ');localStorage.setItem("xy-switch:theme",' + JSON.stringify(theme) + ');localStorage.setItem(' + JSON.stringify(storageKey) + ',' + JSON.stringify(legacy) + ');');
  await c.viewport(width, width < 500 ? 844 : 960); await c.navigate(base + '/');
  const label = language === 'en' ? 'Settings' : '设置';
  const snapshot = await c.snapshot(); assert(snapshot.controls.some(e => e.text.trim() === label));
  await c.click('.xy-shell-nav-item[title=' + JSON.stringify(label) + ']');
  await c.click('#settings-tab-sync'); await c.waitFor('Boolean(' + page + ')');
}
async function importPreview() { await fill('加密口令', password); await click('选择加密文件并预览'); await c.waitFor('document.querySelectorAll(".sync-change").length > 0'); }
async function saveScreenshot(name) { await c.screenshot(join(output, name + '.png')); }
async function run(name, work) {
  if (only && !only.includes(name)) return;
  const start = c.exceptions.length; console.log('RUN ' + name);
  try { await work(); await noSecrets(); assert.equal(c.exceptions.length, start, 'Unexpected browser exception'); reports.push({ name, passed: true }); console.log('PASS ' + name); }
  catch (error) {
    reports.push({ name, passed: false, error: String(error).replaceAll(password, '[redacted]') });
    await saveScreenshot(name + '-failure');
    const snapshot = await c.evaluate('({text:document.body.innerText,controls:[...document.querySelectorAll("button,input,select")].map(e=>({tag:e.tagName,label:e.getAttribute("aria-label"),text:e.textContent.slice(0,120),id:e.id,value:e.type==="password"?"[redacted]":e.value,disabled:e.disabled}))})');
    await writeFile(join(output, name + '-failure.json'), JSON.stringify({ snapshot, exceptions: c.exceptions.slice(start) }, null, 2).replaceAll(password, '[redacted]'));
    console.error('FAIL ' + name + ': ' + String(error).replaceAll(password, '[redacted]'));
  } finally { await writeFile(join(output, 'results.json'), JSON.stringify(reports, null, 2)); }
}
let fixtureId;
try {
  ({ identifier: fixtureId } = await c.call('Page.addScriptToEvaluateOnNewDocument', { source })); await c.navigate(base + '/');
  await run('default-no-automatic-sync', async () => {
    await reset(); assert.match(await text(), /不是自动云同步/); assert.match(await text(), /不传播删除/);
    assert.equal((await calls('preview_sync_export')).length, 0); assert.equal((await calls('preview_sync_import')).length, 0);
    await disabled('选择加密文件并预览'); await noOverflow(); await saveScreenshot('sync-default');
  });
  await run('export-review-passphrase-confirmation-and-clearing', async () => {
    await reset(); await click('预览加密导出'); await c.waitFor('Boolean(document.querySelector(".gov-diff"))');
    assert.equal((await calls('load_session_favorites')).length, 1);
    assert.equal(await c.evaluate('localStorage.getItem(' + JSON.stringify(storageKey) + ')'), legacy);
    assert.match(await text(), /qa-session/); assert.match(await text(), /QA encrypted provider/);
    await disabled('确认并另存加密文件'); await fill('加密口令', 'short'); await fill('再次输入加密口令', 'short');
    await review(); await confirm('EXPORT'); await disabled('确认并另存加密文件');
    await fill('加密口令', password); await fill('再次输入加密口令', password + '-different'); await disabled('确认并另存加密文件');
    await fill('再次输入加密口令', password); await disabled('确认并另存加密文件', false);
    await c.evaluate('window.__XY_QA__.hold.push("save_sync_export")'); await click('确认并另存加密文件');
    assert(await c.evaluate('[...document.querySelectorAll(".encrypted-sync-settings input[type=password]")].every(e=>e.value==="")'));
    assert.equal((await calls('save_sync_export'))[0].args.passphrase, '[redacted]');
    await c.evaluate('window.__XY_QA__.release("save_sync_export")'); await c.waitFor(page + '.innerText.includes("加密文件已保存")');
    assert.equal(await c.evaluate('Object.keys(window.__XY_QA__.sync.tickets).length'), 0);
    assert.equal((await calls('apply_sync_import')).length, 0);
  });
  await run('explicit-conflicts-merge-backup-and-rollback', async () => {
    await reset(); await importPreview(); await disabled('确认合并所选变更');
    assert(await c.evaluate('[...document.querySelectorAll(".sync-change select")].every(e=>e.value==="")'));
    await review(); await confirm('IMPORT'); await disabled('确认合并所选变更');
    await click('仅接受新增项'); assert.equal(await c.evaluate('document.querySelector(".gov-check input").checked'), false);
    await review(); await disabled('确认合并所选变更', false);
    await c.click('.sync-change summary'); assert.match(await text(), /保留旧供应商及凭据/);
    await saveScreenshot('sync-conflicts'); await click('确认合并所选变更');
    await c.waitFor(page + '.innerText.includes("已合并 2 项")');
    const applied = (await calls('apply_sync_import'))[0].args;
    assert.equal(applied.confirmation, 'IMPORT'); assert.equal(applied.choices.length, 4);
    assert.equal(applied.choices.filter(v => v.useIncoming).length, 2);
    assert.equal(await c.evaluate('window.__XY_QA__.sync.backups.length'), 1);
    assert(await c.evaluate('window.__XY_QA__.sync.favorites.some(f=>f.sessionId==="qa-imported")'));
    await click('预览同步回滚'); await disabled('确认回滚同步'); await review(); await confirm('ROLLBACK SYNC', 'ROLLBACK'); await disabled('确认回滚同步');
    await confirm('ROLLBACK SYNC'); await click('确认回滚同步'); await c.waitFor(page + '.innerText.includes("同步变更已回滚")');
    assert.equal(await c.evaluate('window.__XY_QA__.sync.backups[0].used'), true);
    assert.equal(await c.evaluate('window.__XY_QA__.sync.favorites.some(f=>f.sessionId==="qa-imported")'), false);
  });
  await run('saved-import-survives-refresh-failure', async () => {
    await reset(); await importPreview(); await click('仅接受新增项'); await review(); await confirm('IMPORT');
    await c.evaluate('window.__XY_QA__.fail.load_session_favorites="database unavailable"');
    await click('确认合并所选变更');
    await c.waitFor(page + '.innerText.includes("数据已保存，但界面刷新失败")');
    assert.match(await text(), /已合并 2 项/);
    assert.equal((await calls('apply_sync_import')).length, 1);
    assert.equal(await c.evaluate('window.__XY_QA__.sync.backups.length'), 1);
    assert(await c.evaluate('window.__XY_QA__.sync.favorites.some(f=>f.sessionId==="qa-imported")'));
    assert.match(await text(), /预览同步回滚/);
    assert.equal(await c.evaluate('Object.keys(window.__XY_QA__.sync.tickets).length'), 0);
  });
  await run('running-proxy-prevents-apply', async () => {
    await reset({ running: true }); await importPreview(); await click('仅接受新增项'); await review(); await confirm('IMPORT');
    await disabled('确认合并所选变更'); assert.equal((await calls('apply_sync_import')).length, 0);
    await click('取消并清除预览'); assert.equal(await c.evaluate('Object.keys(window.__XY_QA__.sync.tickets).length'), 0);
  });
  await run('wrong-password-and-stale-preview-fail-closed', async () => {
    await reset(); await fill('加密口令', 'incorrect fixture passphrase'); await click('选择加密文件并预览');
    await c.waitFor(page + '.innerText.includes("Wrong passphrase")');
    assert.equal(await c.evaluate('document.querySelector(".encrypted-sync-settings input[type=password]").value'), '');
    assert.equal(await c.evaluate('Object.keys(window.__XY_QA__.sync.tickets).length'), 0);
    await importPreview(); await click('仅接受新增项'); await review(); await confirm('IMPORT');
    await c.evaluate('window.__XY_QA__.fail.apply_sync_import="Local data changed after preview; preview again"');
    await click('确认合并所选变更'); await c.waitFor(page + '.innerText.includes("Local data changed after preview")');
    assert.equal(await c.evaluate('window.__XY_QA__.sync.backups.length'), 0);
    assert.equal(await c.evaluate('Object.keys(window.__XY_QA__.sync.tickets).length'), 0);
  });
  await run('migration-failure-blocks-file-exchange', async () => {
    await reset(); await c.evaluate('window.__XY_QA__.fail.load_session_favorites="Invalid legacy favorites data"');
    await click('预览加密导出'); await c.waitFor(page + '.innerText.includes("Local favorites could not be synchronized")');
    assert.equal((await calls('preview_sync_export')).length, 0);
    assert.equal(await c.evaluate('localStorage.getItem(' + JSON.stringify(storageKey) + ')'), legacy);
  });
  await run('native-favorites-use-identifiers-and-retain-legacy', async () => {
    await reset(); await c.click('.xy-shell-nav-item[title="会话"]'); await c.waitFor('document.querySelectorAll(".session-index__row").length === 1');
    await c.waitFor('document.querySelector(".session-favorite")?.getAttribute("aria-pressed")==="true"');
    await c.click('.session-favorite'); await c.waitFor('window.__XY_QA__.calls.some(c=>c.command==="set_session_favorite")');
    const call = (await calls('set_session_favorite'))[0];
    assert.deepEqual(call.args, { favorite: { providerId: 'codex', sessionId: 'qa-session' }, enabled: false });
    assert.equal(await c.evaluate('localStorage.getItem(' + JSON.stringify(storageKey) + ')'), legacy);
    await c.click('.xy-shell-nav-item[title="设置"]'); await c.click('#settings-tab-sync');
    await click('预览加密导出'); await c.waitFor('Boolean(document.querySelector(".gov-diff"))');
    assert.equal(await c.evaluate('window.__XY_QA__.sync.favorites.length'), 0);
    await click('取消并清除预览');
  });
  await run('late-and-expired-preview-tickets-are-cleared', async () => {
    await reset(); await c.evaluate('window.__XY_QA__.hold.push("preview_sync_import")');
    await fill('加密口令', password); await click('选择加密文件并预览'); await c.click('#settings-tab-data');
    await c.evaluate('window.__XY_QA__.release("preview_sync_import")');
    await c.waitFor('window.__XY_QA__.calls.some(c=>c.command==="cancel_sync_preview")');
    assert.equal(await c.evaluate('Object.keys(window.__XY_QA__.sync.tickets).length'), 0);
    await c.click('#settings-tab-sync'); await c.evaluate('window.__XY_QA__.sync.ttl=-1'); await click('预览加密导出');
    await c.waitFor(page + '.innerText.includes("同步预览已过期")');
    assert.equal(await c.evaluate('Object.keys(window.__XY_QA__.sync.tickets).length'), 0);
  });
  await run('pagination-and-narrow-layout', async () => {
    await reset({ width: 390, theme: 'dark' }); await c.evaluate('window.__XY_QA__.sync.large=true'); await importPreview();
    assert.equal(await c.evaluate('document.querySelectorAll(".sync-change").length'), 50); await noOverflow();
    await click('下一页'); assert.equal(await c.evaluate('document.querySelectorAll(".sync-change").length'), 7);
    await noOverflow(); await saveScreenshot('sync-narrow-dark');
  });
  await run('english-and-mobile-browser-read-only', async () => {
    await reset({ language: 'en' }); assert.match(await text(), /Manual encrypted file exchange/);
    await click('Preview encrypted export'); await c.waitFor('Boolean(document.querySelector(".gov-diff"))'); await noOverflow(); await saveScreenshot('sync-english');
    for (const os of ['android', 'ios', 'browser']) {
      await reset({ os, width: 390 }); assert.match(await text(), /仅限桌面应用/);
      assert.equal(await c.evaluate(page + '.querySelectorAll("input,button").length'), 0);
      assert.equal((await calls('list_sync_backups')).length, 0); assert.equal((await calls('preview_sync_export')).length, 0); await noOverflow();
    }
    await saveScreenshot('sync-mobile-readonly');
  });
} finally {
  if (fixtureId) await c.call('Page.removeScriptToEvaluateOnNewDocument', { identifier: fixtureId });
  await c.close();
}
console.log(JSON.stringify({ output, passed: reports.filter(r => r.passed).length, total: reports.length }));
if (reports.some(r => !r.passed)) process.exitCode = 1;
