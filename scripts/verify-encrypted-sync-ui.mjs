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
const c = await connect(undefined, { fresh: true }), reports = [], only = process.env.QA_ONLY?.trim() ? process.env.QA_ONLY.split(',') : null;
const page = 'document.querySelector(".encrypted-sync-settings")';
const click = label => c.button(label, page);
const text = () => c.evaluate(page + '.innerText');
const calls = command => c.evaluate('window.__XY_QA__.calls.filter(c=>c.command===' + JSON.stringify(command) + ')');
const button = label => '[...' + page + '.querySelectorAll("button")].find(b=>b.textContent.trim()===' + JSON.stringify(label) + ')';
const disabled = async (label, expected = true) => {
  const actual = await c.evaluate(button(label) + '?.matches(":disabled")');
  assert.notEqual(actual, undefined, 'Missing button: ' + label); assert.equal(actual, expected, label);
};
async function field(label) {
  const id = await c.evaluate('[...' + page + '.querySelectorAll("label")].find(e=>e.textContent.trim()===' + JSON.stringify(label) + ')?.htmlFor');
  assert(id, 'Missing field: ' + label);
  assert(await c.evaluate(`(() => {const element=document.getElementById(${JSON.stringify(id)}),dialog=${page}.querySelector('[role="dialog"]');return element.getClientRects().length>0&&!element.closest('details:not([open])')&&(!dialog||dialog.contains(element));})()`), 'Field is not accessible: ' + label);
  return '[id=' + JSON.stringify(id) + ']';
}
const fill = async (label, value) => c.fill(await field(label), value);
const confirm = (word, value = word) => fill('输入确认文字 · ' + word, value);
async function review() { await c.click('.encrypted-sync-settings [role="dialog"] .gov-check input'); }
async function activePreview() {
  const entries = await c.evaluate('Object.entries(window.__XY_QA__.sync.tickets)');
  assert.equal(entries.length, 1, 'Exactly one preview ticket must be active');
  const [ticketId, preview] = entries[0]; return { ticketId, ...preview };
}
async function expandDiff(selector = '.sync-review-modal .gov-diff') {
  const entry = 'document.querySelector(' + JSON.stringify(selector) + ')';
  await c.waitFor(entry + '?.getClientRects().length > 0');
  const preview = await activePreview(), identity = await c.evaluate(entry + '.querySelector(".gov-diff__identity").textContent');
  const english = await c.evaluate('localStorage.getItem("xy-switch:language")==="en"');
  const change = preview.action === 'import' ? preview.content.find(row => row.key === identity) : null;
  if (preview.action === 'import') assert(change, 'Diff must identify a preview object by its complete key');
  else if (preview.action === 'export') assert.equal(identity, english ? 'Export configuration' : '导出配置');
  else {
    const selectedBackupId = (await calls('preview_sync_rollback')).at(-1)?.args.id;
    assert(selectedBackupId, 'Rollback must identify the backup requested for preview');
    assert.equal(identity, selectedBackupId);
    assert.equal(await c.evaluate('document.querySelector(".sync-review-modal .gov-confirm-subject code").textContent'), selectedBackupId);
  }
  const before = change ? change.local : preview.action === 'export' ? {} : preview.content.before;
  const after = change ? change.incoming : preview.action === 'export' ? preview.content : preview.content.after;
  assert.equal(await c.evaluate(entry + '.querySelectorAll(".gov-diff__complete details").length'), 2);
  for (const [index, content] of [before, after].entries()) {
    const complete = selector + ' .gov-diff__complete details:nth-child(' + (index + 1) + ')';
    if (!await c.evaluate('document.querySelector(' + JSON.stringify(complete) + ').open')) await c.click(complete + ' > summary');
    assert(await c.evaluate('document.querySelector(' + JSON.stringify(complete + ' > pre') + ').getClientRects().length > 0'), 'Complete diff content must be readable');
    assert.equal(await c.evaluate('document.querySelector(' + JSON.stringify(complete + ' > pre') + ').textContent'), JSON.stringify(content, null, 2), 'Complete diff side must match the ticket');
  }
  const headings = await c.evaluate('[...' + entry + '.querySelectorAll(".gov-diff-lines thead th")].slice(0,2).map(element=>element.textContent)');
  if (headings.length || JSON.stringify(before) !== JSON.stringify(after)) assert.deepEqual(headings, change ? ['本地内容', '传入内容'] : english ? ['Before', 'After'] : ['修改前', '修改后']);
  assert.equal(await c.evaluate(entry + '.querySelectorAll(".gov-diff__counts dd").length'), 2);
}
const objectKeys = () => c.evaluate('[...document.querySelectorAll(".sync-review__objects > li code")].map(element=>element.textContent)');
async function selectObject(key) {
  if (await c.evaluate('document.querySelector(".sync-review__mobile-select select")?.getClientRects().length > 0')) await fill('所选对象', key);
  else await c.clickExpression('[...document.querySelectorAll(".sync-review__objects button")].find(element=>element.querySelector("code").textContent===' + JSON.stringify(key) + ')');
  await c.waitFor('document.querySelector(".sync-review__detail .gov-diff__identity")?.textContent===' + JSON.stringify(key));
  assert.equal(await c.evaluate('document.querySelector(".sync-review__objects button[aria-pressed=true]")?.getAttribute("aria-controls")'), await c.evaluate('document.querySelector(".sync-review__detail").id'));
}
async function reviewCleared() {
  assert.equal(await c.evaluate('document.querySelector(".encrypted-sync-settings [role=dialog] .gov-check input").checked'), false);
  assert.equal(await c.evaluate('document.querySelector(' + JSON.stringify(await field('输入确认文字 · IMPORT')) + ').value'), '');
  await disabled('确认合并所选变更');
}
const summary = () => c.evaluate('Object.fromEntries([...' + page + '.querySelectorAll(".sync-summary dt")].map(label=>[label.textContent.trim(),label.nextElementSibling.innerText.trim()]))');
async function category(name) {
  const selector = '#settings-task-' + name;
  if (!await c.evaluate('Boolean(document.querySelector(' + JSON.stringify(selector) + '))')) {
    await c.click('.settings-page > .page-heading button');
    await discardTaskDraft();
  }
  await c.waitFor('Boolean(document.querySelector(' + JSON.stringify(selector) + '))');
  await c.click(selector);
  await c.waitFor('Boolean(document.querySelector(' + JSON.stringify('#settings-panel-' + name) + '))');
}
async function discardTaskDraft() {
  if (await c.evaluate('Boolean(document.querySelector(".task-leave-dialog"))')) {
    const english = await c.evaluate('document.documentElement.lang === "en"');
    await c.button(english ? 'Discard and leave' : '丢弃并离开', 'document.querySelector(".task-leave-dialog")');
    await c.waitFor('!document.querySelector(".task-leave-dialog")');
  }
}
async function syncView(name) {
  await c.click('#encrypted-sync-tab-' + name);
  await discardTaskDraft();
  await c.waitFor('Boolean(document.querySelector(' + JSON.stringify('#encrypted-sync-panel-' + name) + '))');
}
async function noOverflow() { const m = await c.evaluate('({width:innerWidth,root:document.documentElement.scrollWidth,body:document.body.scrollWidth})'); assert(m.root <= m.width && m.body <= m.width, JSON.stringify(m)); }
async function noSecrets() {
  const values = await c.evaluate('JSON.stringify({storage:Object.entries(localStorage),calls:window.__XY_QA__.calls})');
  assert(!values.includes(password), 'Passphrase leaked into storage or QA calls');
  assert.equal((await calls('set_provider_api_key')).length, 0);
}
async function reset({ os = 'windows', running = false, language = 'zh-CN', width = 1280, theme = 'light' } = {}) {
  await c.evaluate('localStorage.setItem("xy-sync-qa:os",' + JSON.stringify(os) + ');localStorage.setItem("xy-sync-qa:running",' + JSON.stringify(String(running)) + ');localStorage.setItem("xy-sync-qa:language",' + JSON.stringify(language) + ');localStorage.setItem("xy-switch:language",' + JSON.stringify(language) + ');localStorage.setItem("xy-switch:theme",' + JSON.stringify(theme) + ');localStorage.setItem(' + JSON.stringify(storageKey) + ',' + JSON.stringify(legacy) + ');');
  await c.viewport(width, width < 500 ? 844 : 960); await c.navigate(base + '/', { readySelector: ['android', 'ios'].includes(os) ? '.workspace-route-notice' : 'h1' });
  await c.workspace('settings');
  await category('sync'); await c.waitFor('Boolean(' + page + ')');
}
async function importPreview() {
  await fill('加密口令', password); await click('选择加密文件并预览');
  await c.waitFor('document.querySelector(".sync-review-modal--import .sync-review__detail")?.getClientRects().length > 0');
  assert((await objectKeys()).length > 0, 'Import review must list its objects');
  assert(await c.evaluate('[...document.querySelectorAll(".encrypted-sync-settings input[type=password]")].every(element=>element.value==="")'));
  assert.deepEqual((await calls('preview_sync_import')).at(-1).args, { passphrase: '[redacted]' });
}
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
  } finally {
    await writeFile(join(output, name + '-calls.json'), JSON.stringify(await c.evaluate('window.__XY_QA__.calls'), null, 2).replaceAll(password, '[redacted]'));
    await writeFile(join(output, 'results.json'), JSON.stringify(reports, null, 2));
  }
}
let fixtureId;
try {
  ({ identifier: fixtureId } = await c.call('Page.addScriptToEvaluateOnNewDocument', { source })); await c.navigate(base + '/');
  await run('default-no-automatic-sync', async () => {
    await reset(); assert.match(await text(), /不是自动云同步/); assert.match(await text(), /不传播删除/);
    assert.equal((await calls('preview_sync_export')).length, 0); assert.equal((await calls('preview_sync_import')).length, 0);
    assert.equal((await calls('list_sync_backups')).length, 0);
    await disabled('选择加密文件并预览'); await noOverflow(); await saveScreenshot('sync-default');
  });
  await run('changing-sync-view-clears-passphrase', async () => {
    await reset(); await fill('加密口令', password); await disabled('选择加密文件并预览', false);
    await syncView('backups'); assert.equal(await c.evaluate(page + '.querySelectorAll("input[type=password]").length'), 0);
    await syncView('exchange'); assert.equal(await c.evaluate(page + '.querySelector("input[type=password]").value'), '');
    await disabled('选择加密文件并预览'); assert.equal((await calls('preview_sync_import')).length, 0); assert.equal((await calls('preview_sync_export')).length, 0);
  });
  await run('export-review-passphrase-confirmation-and-clearing', async () => {
    await reset(); await click('预览加密导出'); await c.waitFor('Boolean(document.querySelector(".gov-diff"))'); await expandDiff();
    const preview = await activePreview();
    assert.equal((await calls('load_session_favorites')).length, 1);
    assert.equal(await c.evaluate('localStorage.getItem(' + JSON.stringify(storageKey) + ')'), legacy);
    assert.match(await text(), /qa-session/); assert.match(await text(), /QA encrypted provider/);
    await disabled('确认并另存加密文件'); await fill('加密口令', 'short'); await fill('再次输入加密口令', 'short');
    await review(); await confirm('EXPORT'); await disabled('确认并另存加密文件');
    await fill('加密口令', password); await fill('再次输入加密口令', password + '-different'); await disabled('确认并另存加密文件');
    await fill('再次输入加密口令', password); await disabled('确认并另存加密文件', false);
    await confirm('EXPORT', 'export'); await disabled('确认并另存加密文件');
    assert.equal((await calls('save_sync_export')).length, 0); await confirm('EXPORT');
    await c.evaluate('document.querySelector(".sync-review-modal .gov-diff").scrollIntoView({block:"start"})'); await saveScreenshot('sync-export-diff');
    await c.evaluate('window.__XY_QA__.hold.push("save_sync_export")'); await click('确认并另存加密文件');
    assert(await c.evaluate('[...document.querySelectorAll(".encrypted-sync-settings input[type=password]")].every(e=>e.value==="")'));
    assert.deepEqual((await calls('save_sync_export'))[0].args, { ticketId: preview.ticketId, passphrase: '[redacted]', confirmation: 'EXPORT' });
    await c.evaluate('window.__XY_QA__.release("save_sync_export")'); await c.waitFor(page + '.innerText.includes("加密文件已保存")');
    assert.equal(await c.evaluate('Object.keys(window.__XY_QA__.sync.tickets).length'), 0);
    assert.equal((await calls('apply_sync_import')).length, 0);
  });
  await run('explicit-conflicts-merge-backup-and-rollback', async () => {
    await reset(); await importPreview(); await disabled('确认合并所选变更');
    const preview = await activePreview();
    assert.deepEqual(await objectKeys(), preview.content.map(change => change.key));
    assert.deepEqual(await summary(), { '变更': '4', '接受传入项': '0', '保留本地 / 跳过': '0', '尚未选择': '4' });
    for (const change of preview.content) {
      await selectObject(change.key);
      if (change.status === 'unchanged') {
        assert.equal(await c.evaluate('document.querySelectorAll(".sync-choice select").length'), 0);
        assert.match(await c.evaluate('document.querySelector(".sync-review__detail").innerText'), /此对象与本地一致，不会写入/);
      } else {
        assert.equal(await c.evaluate('document.querySelectorAll(".sync-choice select").length'), 1);
        assert.equal(await c.evaluate('document.querySelector(".sync-choice select").value'), '');
        assert.equal(await c.evaluate('document.querySelector(".sync-choice select").getAttribute("aria-label")'), '处理方式 · ' + change.key);
      }
      await expandDiff();
    }
    await selectObject(preview.content[0].key);
    assert.match(await c.evaluate('document.querySelector(".sync-review__detail .gov-warning").innerText'), /保留旧供应商及凭据，另建禁用副本/);
    await review(); await confirm('IMPORT'); await disabled('确认合并所选变更');
    await click('全部保留本地'); await reviewCleared();
    assert.deepEqual(await summary(), { '变更': '4', '接受传入项': '0', '保留本地 / 跳过': '4', '尚未选择': '0' });
    await review(); await confirm('IMPORT'); await disabled('确认合并所选变更');
    await click('仅接受新增项'); await reviewCleared();
    assert.deepEqual(await summary(), { '变更': '4', '接受传入项': '2', '保留本地 / 跳过': '2', '尚未选择': '0' });
    await review(); await disabled('确认合并所选变更'); await confirm('IMPORT', 'import'); await disabled('确认合并所选变更');
    await confirm('IMPORT'); await disabled('确认合并所选变更', false);
    await fill('处理方式', 'incoming'); await reviewCleared();
    assert.deepEqual(await summary(), { '变更': '4', '接受传入项': '3', '保留本地 / 跳过': '1', '尚未选择': '0' });
    await review(); await confirm('IMPORT'); await fill('处理方式', 'local'); await reviewCleared();
    await review(); await confirm('IMPORT'); await disabled('确认合并所选变更', false);
    assert.equal((await calls('apply_sync_import')).length, 0);
    await expandDiff(); await c.evaluate('document.querySelector(".sync-review__detail").scrollIntoView({block:"start"})');
    await saveScreenshot('sync-conflicts'); await click('确认合并所选变更');
    await c.waitFor(page + '.innerText.includes("已合并 2 项")');
    const applied = (await calls('apply_sync_import'))[0].args;
    assert.equal(applied.confirmation, 'IMPORT'); assert.equal(applied.choices.length, 4);
    assert.equal(applied.choices.filter(v => v.useIncoming).length, 2);
    assert.deepEqual(applied, { ticketId: preview.ticketId, choices: preview.content.filter(change => change.status !== 'unchanged').map(change => ({ key: change.key, useIncoming: change.status === 'add' })), confirmation: 'IMPORT' });
    assert.equal(await c.evaluate('window.__XY_QA__.sync.backups.length'), 1);
    assert(await c.evaluate('window.__XY_QA__.sync.favorites.some(f=>f.sessionId==="qa-imported")'));
    await syncView('backups'); await click('预览同步回滚'); await expandDiff();
    const rollback = await activePreview();
    assert.deepEqual((await calls('preview_sync_rollback'))[0].args, { id: await c.evaluate('window.__XY_QA__.sync.backups[0].id') });
    await c.evaluate('document.querySelector(".sync-review-modal .gov-diff").scrollIntoView({block:"start"})'); await saveScreenshot('sync-rollback-diff');
    await disabled('确认回滚同步'); await review(); await confirm('ROLLBACK SYNC', 'ROLLBACK'); await disabled('确认回滚同步');
    assert.equal((await calls('apply_sync_rollback')).length, 0);
    await confirm('ROLLBACK SYNC'); await click('确认回滚同步'); await c.waitFor(page + '.innerText.includes("同步变更已回滚")');
    assert.deepEqual((await calls('apply_sync_rollback'))[0].args, { ticketId: rollback.ticketId, confirmation: 'ROLLBACK SYNC' });
    assert.equal(await c.evaluate('window.__XY_QA__.sync.backups[0].used'), true);
    assert.equal(await c.evaluate('window.__XY_QA__.sync.favorites.some(f=>f.sessionId==="qa-imported")'), false);
    assert.equal(await c.evaluate('Object.keys(window.__XY_QA__.sync.tickets).length'), 0);
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
    await syncView('backups'); await c.waitFor(page + '.innerText.includes("预览同步回滚")');
    assert.equal(await c.evaluate('Object.keys(window.__XY_QA__.sync.tickets).length'), 0);
  });
  await run('running-proxy-prevents-apply', async () => {
    await reset({ running: true }); await importPreview(); await click('仅接受新增项'); await review(); await confirm('IMPORT');
    await disabled('确认合并所选变更'); assert.equal((await calls('apply_sync_import')).length, 0);
    assert.match(await text(), /请先停止代理，并在设置中恢复接管/);
    await click('取消并清除预览'); assert.equal(await c.evaluate('Object.keys(window.__XY_QA__.sync.tickets).length'), 0);
  });
  await run('wrong-password-and-stale-preview-fail-closed', async () => {
    await reset(); await fill('加密口令', 'incorrect fixture passphrase'); await click('选择加密文件并预览');
    await c.waitFor(page + '.innerText.includes("Wrong passphrase")');
    assert.equal(await c.evaluate('document.querySelector(".encrypted-sync-settings input[type=password]").value'), '');
    assert.equal(await c.evaluate('Object.keys(window.__XY_QA__.sync.tickets).length'), 0);
    assert.equal((await calls('apply_sync_import')).length, 0);
    await importPreview(); await click('仅接受新增项'); await review(); await confirm('IMPORT');
    await c.evaluate('window.__XY_QA__.fail.apply_sync_import="Local data changed after preview; preview again"');
    await click('确认合并所选变更'); await c.waitFor(page + '.innerText.includes("Local data changed after preview")');
    assert.equal(await c.evaluate('window.__XY_QA__.sync.backups.length'), 0);
    assert.equal(await c.evaluate('Object.keys(window.__XY_QA__.sync.tickets).length'), 0);
    assert.equal(await c.evaluate('document.querySelector(".sync-review-modal")'), null);
    assert.equal(await c.evaluate('document.querySelector(".encrypted-sync-settings input[type=password]").value'), '');
    assert.equal(await c.evaluate('window.__XY_QA__.sync.favorites.some(f=>f.sessionId==="qa-imported")'), false);
  });
  await run('migration-failure-blocks-file-exchange', async () => {
    await reset(); await c.evaluate('window.__XY_QA__.fail.load_session_favorites="Invalid legacy favorites data"');
    await click('预览加密导出'); await c.waitFor(page + '.innerText.includes("Local favorites could not be synchronized")');
    assert.equal((await calls('preview_sync_export')).length, 0);
    assert.equal(await c.evaluate('localStorage.getItem(' + JSON.stringify(storageKey) + ')'), legacy);
  });
  await run('native-favorites-use-identifiers-and-retain-legacy', async () => {
    await reset(); await c.workspace('sessions'); await c.waitFor('document.querySelectorAll(".session-index__row").length === 1');
    await c.click('.session-index__item'); await c.waitFor('document.querySelector(".session-favorite")?.getAttribute("aria-pressed")==="true"');
    await c.click('.session-favorite'); await c.waitFor('window.__XY_QA__.calls.some(c=>c.command==="set_session_favorite")');
    const call = (await calls('set_session_favorite'))[0];
    assert.deepEqual(call.args, { favorite: { providerId: 'codex', sessionId: 'qa-session' }, enabled: false });
    assert.equal(await c.evaluate('localStorage.getItem(' + JSON.stringify(storageKey) + ')'), legacy);
    await c.waitFor('document.querySelector(".session-favorite")?.getAttribute("aria-pressed")==="false"');
    assert.equal(await c.evaluate('document.querySelector(".sessions-workspace").dataset.reading'), 'true');
    await c.click('.session-reader__back'); await c.waitFor('document.querySelector(".sessions-workspace")?.dataset.reading==="false"');
    await c.waitFor('document.activeElement?.matches(".session-index__item[aria-current=true]")');
    await c.workspace('settings'); await category('sync');
    await click('预览加密导出'); await c.waitFor('Boolean(document.querySelector(".gov-diff"))'); await expandDiff();
    assert.equal(await c.evaluate('window.__XY_QA__.sync.favorites.length'), 0);
    assert.deepEqual((await activePreview()).content.favorites, []);
    await click('取消并清除预览');
  });
  await run('pending-and-expired-preview-tickets-are-cleared', async () => {
    await reset(); await c.evaluate('window.__XY_QA__.hold.push("preview_sync_import")');
    await fill('加密口令', password); await click('选择加密文件并预览');
    assert(await c.evaluate('document.querySelector(".settings-page > .page-heading button").disabled'));
    await c.click('.xy-shell-nav-item[data-view="governance"]');
    await c.waitFor('Boolean(document.querySelector(".task-navigation-wait"))');
    assert.equal(await c.evaluate('document.querySelector(".xy-shell-root").dataset.workspaceView'), 'settings');
    await c.evaluate('window.__XY_QA__.release("preview_sync_import")');
    await c.waitFor('Boolean(document.querySelector(".sync-review-modal--import"))');
    await click('取消并清除预览');
    await c.waitFor('window.__XY_QA__.calls.some(c=>c.command==="cancel_sync_preview")');
    assert.equal(await c.evaluate('Object.keys(window.__XY_QA__.sync.tickets).length'), 0);
    assert.equal((await calls('apply_sync_import')).length, 0);
    await category('data');
    await category('sync'); await c.evaluate('window.__XY_QA__.sync.ttl=-1'); await click('预览加密导出');
    await c.waitFor(page + '.innerText.includes("同步预览已过期")');
    assert.equal(await c.evaluate('Object.keys(window.__XY_QA__.sync.tickets).length'), 0);
    assert.equal(await c.evaluate('document.querySelector(".sync-review-modal")'), null);
    assert.equal((await calls('save_sync_export')).length, 0); assert.equal((await calls('apply_sync_import')).length, 0);
  });
  await run('pagination-and-narrow-layout', async () => {
    await reset({ width: 390, theme: 'dark' }); await c.evaluate('window.__XY_QA__.sync.large=true'); await importPreview();
    const preview = await activePreview(), allKeys = preview.content.map(change => change.key), visited = [];
    assert.equal(allKeys.length, 57);
    await expandDiff(); await c.evaluate('document.querySelector(".sync-review__detail").scrollIntoView({block:"start"})'); await saveScreenshot('sync-narrow-dark');
    for (let index = 0; index < 8; index++) {
      const expected = allKeys.slice(index * 8, (index + 1) * 8), keys = await objectKeys();
      assert.deepEqual(keys, expected); visited.push(...keys);
      assert.equal(await c.evaluate('document.querySelector(".sync-review__pagination .gov-number").textContent'), (index + 1) + ' / 8');
      await disabled('上一页', index === 0); await disabled('下一页', index === 7);
      assert(await c.evaluate('document.querySelector(".sync-review__mobile-select select").getClientRects().length > 0'));
      assert.deepEqual(await c.evaluate('[...document.querySelector(".sync-review__mobile-select select").options].map(option=>option.value)'), expected);
      await selectObject(keys.at(-1)); await fill('处理方式', 'incoming'); await noOverflow();
      if (index < 7) await click('下一页');
    }
    assert.deepEqual(visited, allKeys); assert.equal(new Set(visited).size, 57);
    assert.deepEqual(await summary(), { '变更': '56', '接受传入项': '8', '保留本地 / 跳过': '0', '尚未选择': '48' });
    await expandDiff(); await c.evaluate('document.querySelector(".sync-review").scrollIntoView({block:"start"})'); await saveScreenshot('sync-narrow-last-page');
    await review(); await confirm('IMPORT'); await disabled('确认合并所选变更');
    for (let index = 7; index > 0; index--) await click('上一页');
    await selectObject(allKeys[7]); assert.equal(await c.evaluate('document.querySelector(".sync-choice select").value'), 'incoming');
    await click('仅接受新增项'); await reviewCleared();
    assert.deepEqual(await summary(), { '变更': '56', '接受传入项': '54', '保留本地 / 跳过': '2', '尚未选择': '0' });
    await review(); await disabled('确认合并所选变更'); await confirm('IMPORT'); await disabled('确认合并所选变更', false);
    assert.equal((await calls('apply_sync_import')).length, 0);
    await click('取消并清除预览'); assert.equal(await c.evaluate('Object.keys(window.__XY_QA__.sync.tickets).length'), 0);
    await writeFile(join(output, 'sync-pagination.json'), JSON.stringify({ pageSize: 8, pages: 8, visited, retainedChoice: allKeys[7] }, null, 2));
  });
  await run('english-and-mobile-browser-read-only', async () => {
    await reset({ language: 'en' }); assert.match(await text(), /Manual encrypted file exchange/);
    await click('Preview encrypted export'); await c.waitFor('Boolean(document.querySelector(".gov-diff"))'); await expandDiff(); await noOverflow(); await saveScreenshot('sync-english');
    await click('Cancel and clear preview');
    await c.waitFor('!document.querySelector(".sync-review-modal") && Object.keys(window.__XY_QA__.sync.tickets).length === 0');
    for (const os of ['android', 'ios', 'browser']) {
      await reset({ os, width: 390 }); assert.match(await text(), /仅限桌面应用/);
      assert.equal(await c.evaluate(page + '.querySelectorAll("input,button").length'), 0);
      assert.equal((await calls('list_sync_backups')).length, 0); assert.equal((await calls('preview_sync_export')).length, 0); await noOverflow();
    }
    await saveScreenshot('sync-mobile-readonly');
  });
} finally {
  if (fixtureId) await c.call('Page.removeScriptToEvaluateOnNewDocument', { identifier: fixtureId });
  await writeFile(join(output, 'network.json'), JSON.stringify({ requests: c.networkRequests, webSockets: c.webSocketUrls }, null, 2));
  await c.close();
}
console.log(JSON.stringify({ output, passed: reports.filter(r => r.passed).length, total: reports.length }));
if (reports.some(r => !r.passed)) process.exitCode = 1;
