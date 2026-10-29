import assert from 'node:assert/strict';
import { readFile, mkdir, writeFile } from 'node:fs/promises';
import { resolve, join } from 'node:path';
import { connect } from './workspace-qa/cdp.mjs';

const base = process.env.APP_URL ?? 'http://127.0.0.1:1431';
if (!['127.0.0.1', 'localhost', '[::1]'].includes(new URL(base).hostname)) throw new Error('Session QA must use a local renderer');
const output = resolve(process.env.QA_OUTPUT ?? `artifacts/session-favorites-qa/${new Date().toISOString().replace(/[:.]/g, '-')}`);
await mkdir(output, { recursive: true });
const key = 'xy-switch.session-favorites.v1';
const pair = (providerId, sessionId) => JSON.stringify([providerId, sessionId]);
const oldKey = pair('codex', 'qa-shared'), claudeKey = pair('claude', 'qa-shared');
const ghostKey = pair('gemini', 'qa-not-in-catalog');
const saved = keys => JSON.stringify({ version: 1, keys: [...keys].sort() });
const sessions = [
  { providerId: 'codex', sessionId: 'qa-recent', title: 'Recent Codex session', summary: 'Rendering and keyboard checks', projectDir: 'QA:/workspace/recent', createdAt: 1792195200, lastActiveAt: 1792195600 },
  { providerId: 'codex', sessionId: 'qa-shared', title: 'Older Codex session', summary: 'Favorites regression checklist', projectDir: 'QA:/workspace/favorites', createdAt: 1792195000, lastActiveAt: 1792195300 },
  { providerId: 'claude', sessionId: 'qa-shared', title: 'Claude planning notes', summary: 'Layout planning', projectDir: 'QA:/workspace/planning', createdAt: 1792195100, lastActiveAt: 1792195500000 },
  { providerId: 'gemini', sessionId: 'qa-gemini', title: 'Gemini QA checklist', summary: 'Cross-client smoke checks', projectDir: 'QA:/workspace/gemini', createdAt: 1792195100, lastActiveAt: 1792195400 },
].map(session => ({ ...session, sourcePath: `QA:/sessions/${session.providerId}/${session.sessionId}.jsonl`, resumeCommand: `${session.providerId} --resume ${session.sessionId}` }));
const fixture = await readFile(new URL('./workspace-qa/fixture.js', import.meta.url), 'utf8');
const source = `(() => { const language = localStorage.getItem('xy-session-qa:language') || 'zh-CN'; localStorage.setItem('xy-qa:native-language', language); localStorage.setItem('xy-switch:language', language); })();\n${fixture}\nwindow.__XY_QA__.platform.os = "browser";
window.__XY_QA__.sessions = ${JSON.stringify(sessions)};`;
const c = await connect(undefined, { fresh: true });
const reports = [], only = process.env.QA_ONLY?.split(',');
let fixtureId;
const row = title => `[...document.querySelectorAll('.session-index__row')].find(e => e.querySelector('strong')?.textContent === ${JSON.stringify(title)})`;
const star = (title, page = c) => page.clickExpression(`${row(title)}.querySelector('.session-favorite')`);
const select = title => c.clickExpression(`${row(title)}.querySelector('.session-index__item')`);
const rows = () => c.evaluate(`[...document.querySelectorAll('.session-index__item strong')].map(e => e.textContent)`);
const detailTitle = () => c.evaluate(`document.querySelector('.session-detail h2')?.textContent`);
const keys = () => c.evaluate(`JSON.parse(localStorage.getItem(${JSON.stringify(key)}) || '{"keys":[]}').keys.sort()`);
const isFavorite = title => c.evaluate(`${row(title)}?.querySelector('.session-favorite')?.getAttribute('aria-pressed') === 'true'`);
const nav = (label, page = c) => page.click(`.xy-shell-nav-item[title=${JSON.stringify(label)}]`);
const visibleText = () => c.evaluate('document.body.innerText');
const focusSearch = () => c.evaluate(`document.querySelector('input[aria-label="搜索会话"]').focus()`);
const favoritesFilter = '.session-favorites-filter';

async function reset({ raw = null, width = 1280, language = 'zh-CN', theme = 'light' } = {}) {
  await c.evaluate(`localStorage.removeItem(${JSON.stringify(key)}); localStorage.setItem('xy-session-qa:language', ${JSON.stringify(language)}); localStorage.setItem('xy-switch:theme', ${JSON.stringify(theme)}); ${raw === null ? '' : `localStorage.setItem(${JSON.stringify(key)}, ${JSON.stringify(raw)});`}`);
  await c.viewport(width, width < 500 ? 844 : 820);
  await c.navigate(base + '/');
  const snapshot = await c.snapshot();
  const label = language === 'en' ? 'Sessions' : '会话';
  assert(snapshot.controls.some(control => control.label === label || control.text.trim() === label), `missing ${label} navigation`);
  await nav(label);
  await c.waitFor('document.querySelectorAll(".session-index__row").length === 4');
}
async function noOverflow() {
  const metrics = await c.evaluate('({ width: innerWidth, root: document.documentElement.scrollWidth, body: document.body.scrollWidth })');
  assert(metrics.root <= metrics.width && metrics.body <= metrics.width, JSON.stringify(metrics));
}
async function run(name, work) {
  if (only && !only.includes(name)) return;
  console.log('RUN ' + name);
  const exceptionStart = c.exceptions.length;
  try {
    await work();
    assert.equal(c.exceptions.length, exceptionStart, 'unexpected browser exception');
    reports.push({ name, passed: true });
    console.log('PASS ' + name);
  } catch (error) {
    reports.push({ name, passed: false, error: String(error) });
    await c.screenshot(join(output, name + '-failure.png'));
    await writeFile(join(output, name + '-failure.json'), JSON.stringify({ snapshot: await c.snapshot(), exceptions: c.exceptions.slice(exceptionStart) }, null, 2));
    console.error('FAIL ' + name + ': ' + error);
  } finally {
    await writeFile(join(output, 'results.json'), JSON.stringify(reports, null, 2));
  }
}

try {
  ({ identifier: fixtureId } = await c.call('Page.addScriptToEvaluateOnNewDocument', { source }));
  await c.navigate(base + '/');
  await run('favorites-lifecycle', async () => {
    await reset();
    assert.deepEqual(await rows(), ['Recent Codex session', 'Claude planning notes', 'Gemini QA checklist', 'Older Codex session']);
    assert.equal(await detailTitle(), 'Recent Codex session');
    await star('Older Codex session');
    assert.equal((await rows())[0], 'Older Codex session');
    assert.equal(await detailTitle(), 'Recent Codex session', 'a star must not select its row');
    assert.deepEqual(await keys(), [oldKey]);
    await star('Claude planning notes');
    assert.deepEqual(await keys(), [claudeKey, oldKey].sort(), 'client-scoped identical IDs');
    await star('Older Codex session');
    assert.deepEqual(await keys(), [claudeKey]);
    await star('Older Codex session');
    await nav('供应商'); await nav('会话');
    await c.waitFor('document.querySelectorAll(".session-favorite[aria-pressed=true]").length === 2');
    await c.navigate(base + '/'); await nav('会话');
    await c.waitFor('document.querySelectorAll(".session-favorite[aria-pressed=true]").length === 2');
    assert.equal(await c.evaluate('document.querySelectorAll(".session-index button button").length'), 0);
    assert.equal(await c.evaluate('document.querySelectorAll(".session-index [aria-current=true]").length'), 1);
    assert(!(await c.evaluate(`localStorage.getItem(${JSON.stringify(key)})`)).match(/QA:|title|summary|sourcePath|projectDir/));
    await c.screenshot(join(output, 'favorites-desktop-zh-light.png'));
  });
  await run('filters-and-keyboard', async () => {
    await reset({ raw: saved([oldKey, claudeKey, ghostKey]) });
    assert.equal(await c.evaluate(`document.querySelector('${favoritesFilter}').textContent.trim()`), '仅看收藏 (2)');
    await c.click(favoritesFilter);
    assert.equal((await rows()).length, 2);
    await c.fill('select[aria-label="筛选会话客户端"]', 'codex');
    await c.fill('input[aria-label="搜索会话"]', '  FAVORITES  ');
    assert.deepEqual(await rows(), ['Older Codex session']);
    await c.fill('input[aria-label="搜索会话"]', 'no matching session');
    assert((await visibleText()).includes('没有匹配的会话'));
    await focusSearch(); await c.key('Escape');
    assert.deepEqual(await rows(), ['Older Codex session']);
    assert.equal(await c.evaluate(`document.querySelector('${favoritesFilter}').getAttribute('aria-pressed')`), 'true');
    assert.equal(await c.evaluate('document.querySelector("select[aria-label=筛选会话客户端]").value'), 'codex');
    await c.button('清除筛选');
    assert.equal((await rows()).length, 4);
    assert.equal(await c.evaluate('document.activeElement.getAttribute("aria-label")'), '搜索会话');
    await c.fill('input[aria-label="搜索会话"]', 'composing');
    await c.evaluate(`document.activeElement.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', isComposing: true, bubbles: true }))`);
    assert.equal(await c.evaluate('document.activeElement.value'), 'composing');
    await c.key('Escape');
    assert.equal(await c.evaluate('document.activeElement.value'), '');
    await c.evaluate(`${row('Older Codex session')}.querySelector('.session-index__item').focus()`);
    await c.key('Tab');
    assert.equal(await c.evaluate('document.activeElement.classList.contains("session-favorite")'), true);
    await c.key('Enter');
    assert.equal(await isFavorite('Older Codex session'), false);
    assert.equal(await c.evaluate('document.activeElement.classList.contains("session-favorite")'), true, 'keyboard focus survives row reordering');
    assert.equal(await detailTitle(), 'Claude planning notes');
  });
  await run('cross-window-selection', async () => {
    await reset();
    const peer = await connect(undefined, { fresh: true });
    try {
      await peer.call('Page.addScriptToEvaluateOnNewDocument', { source });
      await peer.navigate(base + '/'); await peer.snapshot(); await nav('会话', peer);
      await peer.waitFor('document.querySelectorAll(".session-index__row").length === 4');
      await star('Older Codex session', peer);
      await c.waitFor('document.querySelectorAll(".session-favorite[aria-pressed=true]").length === 1');
      assert.equal((await rows())[0], 'Older Codex session');
      assert.equal(await detailTitle(), 'Recent Codex session', 'another window must not change the current detail');
      await star('Claude planning notes');
      await peer.waitFor('document.querySelectorAll(".session-favorite[aria-pressed=true]").length === 2');
      assert.deepEqual(await keys(), [oldKey, claudeKey].sort());
      assert.equal(await detailTitle(), 'Recent Codex session');
      await c.call('Page.bringToFront'); await c.viewport(820, 820);
      await c.waitFor('!document.querySelector(".session-detail")');
      await c.click(favoritesFilter); await select('Older Codex session');
      await c.waitFor('Boolean(document.querySelector("[role=dialog]"))');
      await star('Older Codex session', peer);
      await c.waitFor('!document.querySelector("[role=dialog]")');
      assert.equal(await c.evaluate('document.activeElement.classList.contains("session-favorites-filter")'), true);
      await peer.evaluate('localStorage.clear()');
      await c.waitFor('document.querySelectorAll(".session-favorite[aria-pressed=true]").length === 0');
      assert((await visibleText()).includes('还没有收藏的会话'));
      assert.equal(peer.exceptions.length, 0);
    } finally { await peer.close(); }
  });
  await run('storage-failure-and-retry', async () => {
    await reset({ raw: saved([ghostKey]) });
    await c.evaluate(`window.__restoreStorage = (() => { const original = Storage.prototype.setItem; window.__saveFavorites = value => original.call(localStorage, ${JSON.stringify(key)}, value); Storage.prototype.setItem = function(k, v) { if (k === ${JSON.stringify(key)}) throw new DOMException('QA storage full', 'QuotaExceededError'); return original.call(this, k, v); }; return () => { Storage.prototype.setItem = original; }; })()`);
    try {
      await star('Older Codex session');
      assert.equal(await isFavorite('Older Codex session'), true);
      assert.deepEqual(await keys(), [ghostKey]);
      assert((await visibleText()).includes('本地收藏暂不可用'));
      await c.evaluate(`window.__saveFavorites(${JSON.stringify(saved([ghostKey, claudeKey]))}); window.__restoreStorage()`);
      await c.button('重试');
      assert.deepEqual(await keys(), [ghostKey, claudeKey, oldKey].sort());
      assert(!(await visibleText()).includes('本地收藏暂不可用'));
    } finally { await c.evaluate('window.__restoreStorage?.()'); }
    await c.evaluate(`window.__restoreStorage = (() => { const original = Storage.prototype.getItem; Storage.prototype.getItem = function(k) { if (k === ${JSON.stringify(key)}) throw new DOMException('QA read blocked', 'SecurityError'); return original.call(this, k); }; return () => { Storage.prototype.getItem = original; }; })()`);
    try {
      await star('Older Codex session');
      assert.equal(await isFavorite('Older Codex session'), false);
      assert((await visibleText()).includes('本地收藏暂不可用'));
      await c.evaluate('window.__restoreStorage()'); await c.button('重试');
      assert.deepEqual(await keys(), [ghostKey, claudeKey].sort());
    } finally { await c.evaluate('window.__restoreStorage?.()'); }
  });
  await run('invalid-storage-and-missing-catalog', async () => {
    const future = '{"version":2,"keys":["future-format"]}';
    await reset({ raw: future });
    assert((await visibleText()).includes('收藏记录格式无法识别'));
    await star('Older Codex session'); await c.button('重试');
    assert.equal(await c.evaluate(`localStorage.getItem(${JSON.stringify(key)})`), future);
    assert.equal(await isFavorite('Older Codex session'), true);
    await c.evaluate(`localStorage.setItem(${JSON.stringify(key)}, ${JSON.stringify(saved([claudeKey, ghostKey]))})`);
    await c.button('重试');
    assert.deepEqual(await keys(), [oldKey, claudeKey, ghostKey].sort());
    await c.evaluate('window.__XY_QA__.sessions = window.__XY_QA__.sessions.filter(s => s.sessionId !== "qa-shared" || s.providerId !== "codex")');
    await c.button('刷新');
    await c.waitFor('document.querySelectorAll(".session-index__row").length === 3');
    assert.deepEqual(await keys(), [oldKey, claudeKey, ghostKey].sort(), 'a partial catalog does not erase favorites');
    await c.evaluate(`window.__XY_QA__.sessions = ${JSON.stringify(sessions)}`); await c.button('刷新');
    await c.waitFor('document.querySelectorAll(".session-favorite[aria-pressed=true]").length === 2');
  });
  await run('deletion-preserves-until-success', async () => {
    await reset({ raw: saved([oldKey, claudeKey]) });
    await select('Older Codex session');
    await c.evaluate('window.__XY_QA__.fail.delete_sessions = "QA session file changed"');
    await c.button('删除会话'); await c.button('确认删除');
    await c.waitFor('document.body.innerText.includes("QA session file changed")');
    assert.deepEqual(await keys(), [oldKey, claudeKey].sort());
    assert.equal((await rows()).length, 4);
    await c.evaluate('delete window.__XY_QA__.fail.delete_sessions; window.__XY_QA__.deleteCount = 0');
    await c.button('确认删除');
    await c.waitFor('document.body.innerText.includes("后端未删除任何会话")');
    assert.deepEqual(await keys(), [oldKey, claudeKey].sort());
    await c.evaluate('window.__XY_QA__.deleteCount = 1'); await c.button('确认删除');
    await c.waitFor('!document.querySelector("[role=alertdialog]") && document.querySelectorAll(".session-index__row").length === 3');
    assert.deepEqual(await keys(), [claudeKey]);
    assert.equal(await isFavorite('Claude planning notes'), true);
    assert.deepEqual(await c.evaluate('window.__XY_QA__.calls.filter(call => call.command === "delete_sessions").at(-1).args'), { ids: [{ providerId: 'codex', sessionId: 'qa-shared' }] });
  });
  await run('compact-drawer-and-empty-state', async () => {
    await reset({ width: 820, raw: saved([oldKey, claudeKey]) });
    await star('Recent Codex session');
    assert.equal(await c.evaluate('Boolean(document.querySelector("[role=dialog]"))'), false, 'the row star must not open the drawer');
    await select('Older Codex session');
    await c.waitFor('Boolean(document.querySelector("[role=dialog]"))');
    assert.equal(await detailTitle(), 'Older Codex session');
    await c.key('Escape');
    assert.equal(await c.evaluate('document.activeElement.getAttribute("aria-label")'), '查看会话 Older Codex session');
    await c.click(favoritesFilter); await select('Older Codex session'); await c.button('已收藏');
    await c.waitFor('!document.querySelector("[role=dialog]")');
    assert.equal(await c.evaluate('document.activeElement.classList.contains("session-favorites-filter")'), true, 'removed drawer trigger has a usable focus fallback');
    assert.deepEqual(await rows(), ['Recent Codex session', 'Claude planning notes']);
    await star('Recent Codex session'); await star('Claude planning notes');
    assert((await visibleText()).includes('还没有收藏的会话'));
    assert.equal(await c.evaluate('document.activeElement.classList.contains("session-favorites-filter")'), true);
    await c.button('查看全部会话');
    assert.equal((await rows()).length, 4);
    assert.equal(await c.evaluate('document.activeElement.getAttribute("aria-label")'), '搜索会话');
  });
  await run('bilingual-responsive-layout', async () => {
    for (const language of ['zh-CN', 'en']) for (const theme of ['light', 'dark']) for (const width of [1280, 820, 390]) {
      await reset({ language, theme, width, raw: saved([oldKey, claudeKey]) });
      await c.click(favoritesFilter);
      await noOverflow();
      if (width === 1280) {
        const tops = await c.evaluate("[...document.querySelectorAll('.session-toolbar select, .session-toolbar input')].map(e => Math.round(e.getBoundingClientRect().top))");
        assert.equal(tops[0], tops[1], 'desktop filters fit on one row');
      }
      const snapshot = await c.snapshot();
      const label = language === 'en' ? 'Favorites only' : '仅看收藏';
      assert(snapshot.controls.some(control => control.label === label));
      assert((await rows()).includes('Older Codex session'), 'user titles are not translated');
      if (width === 390) {
        await select('Older Codex session');
        await c.waitFor('Boolean(document.querySelector("[role=dialog]"))');
        await noOverflow();
      }
      await c.screenshot(join(output, `favorites-${width}-${language}-${theme}.png`));
    }
  });
} finally {
  if (fixtureId) await c.call('Page.removeScriptToEvaluateOnNewDocument', { identifier: fixtureId }).catch(() => {});
  await c.close();
}
console.log(`Session QA: ${reports.filter(report => report.passed).length}/${reports.length} passed. Artifacts: ${output}`);
if (!reports.length || reports.some(report => !report.passed)) process.exitCode = 1;
