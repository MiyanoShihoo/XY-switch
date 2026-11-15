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
window.__XY_QA__.sessions = ${JSON.stringify(sessions)};
Object.defineProperty(navigator, "clipboard", { configurable: true, value: { writeText: async text => { window.__XY_QA__.clipboard = text; } } });`;
const c = await connect(undefined, { fresh: true });
const experienceSessions = Array.from({ length: 18 }, (_, index) => {
  const providerId = ['codex', 'claude', 'gemini'][index % 3];
  const sessionId = 'experience-session-' + String(index + 1).padStart(2, '0');
  return { providerId, sessionId, title: 'QA discussion ' + (index + 1), summary: 'Original transcript and tool records',
    projectDir: ['D:/Projects/xy-switch', 'D:/Projects/planning', 'D:/Projects/research'][index % 3],
    sourcePath: 'QA:/sessions/' + providerId + '/' + sessionId + '.jsonl', resumeCommand: providerId + ' --resume ' + sessionId,
    createdAt: 1792195200 - index * 100, lastActiveAt: 1792195600 - index * 100 };
});
const experienceMessages = [
  { role: 'user', content: '请核对原始会话与工具记录。' },
  { role: 'assistant', content: '  保留开头空格、换行与代码。\nconst result = true;\n' },
  { role: 'assistant', content: '{"path":"QA:/workspace/source","limit":4}', tool: { eventKind: 'call', status: 'requested', name: 'read_file', callId: 'qa-call-1', approvalRequestId: 'qa-approval-1' } },
  { role: 'user', content: '工具返回的内容是否完整？' },
  { role: 'tool', content: '  first line\nsecond line\n', tool: { eventKind: 'result', status: 'completed', name: 'read_file', callId: 'qa-call-1' } },
  { role: 'assistant', content: '来源位置 D:/Projects/original-store/source.ts 保持原样。' },
  { role: 'user', content: '请跳转到这一段讨论并保留键盘焦点。' },
  { role: 'tool', content: '{"approved":true}', tool: { eventKind: 'approval', status: 'approved', name: 'read_file', callId: 'qa-call-1', approvalRequestId: 'qa-approval-1' } },
  { role: 'user', content: '最后确认复制恢复命令只复制、不执行。' },
  { role: 'assistant', content: '原始消息、工具调用与讨论顺序均保留。' },
].map((message, index) => ({ ...message, ts: 1792195200 + index * 60 }));
const experienceFixture = process.env.QA_EXPERIENCE_FIXTURE
  ? await readFile(resolve(process.env.QA_EXPERIENCE_FIXTURE), 'utf8')
  : 'window.__XY_QA__.sessions = ' + JSON.stringify(experienceSessions) + '; window.__XY_QA__.sessionMessages = ' + JSON.stringify(experienceMessages) + ';';
const reports = [], cleanupFailures = [], only = process.env.QA_ONLY?.split(',');
let fixtureId;
const reader = '.sessions-page [role="dialog"]';
const readerScope = 'document.querySelector(' + JSON.stringify(reader) + ')';
const confirmationScope = 'document.querySelector(".sessions-page [role=alertdialog]")';
const row = title => '[...document.querySelectorAll(".session-index__row")].find(element => element.querySelector("strong")?.textContent === ' + JSON.stringify(title) + ')';
const rows = () => c.evaluate('[...document.querySelectorAll(".session-index__item strong")].map(element => element.textContent)');
const detailTitle = (page = c) => page.evaluate(readerScope + '?.querySelector(".xy-ui-modal__title")?.textContent ?? null');
const keys = () => c.evaluate('(JSON.parse(localStorage.getItem(' + JSON.stringify(key) + '))?.keys ?? []).sort()');
const isFavorite = title => c.evaluate('Boolean(' + row(title) + '?.querySelector(".session-index__title svg"))');
const visibleText = () => c.evaluate('document.body.innerText');
const focusSearch = () => c.evaluate('document.querySelector(".session-browser__heading input").focus()');
const favoritesFilter = '.session-favorites-filter';
const favoriteRows = 'document.querySelectorAll(".session-index__title svg").length';

async function workspace(view, page = c) {
  await page.workspace(view);
}

async function closeReader(page = c) {
  if (await page.evaluate('Boolean(' + readerScope + ')')) await page.click(reader + ' .xy-ui-modal__header button');
  await page.waitFor('!' + readerScope);
  if (await page.evaluate('Boolean(document.querySelector(".session-reading-pane .session-reader__back")?.getClientRects().length)')) {
    await page.click('.session-reading-pane .session-reader__back');
    await page.waitFor('document.querySelector(".sessions-page").dataset.reading === "false"');
  }
}
async function closeDetail(page = c) {
  await closeReader(page);
  if (await page.evaluate('Boolean(document.querySelector(".session-preview__navigation button")?.getClientRects().length)')) {
    await page.click('.session-preview__navigation button');
    await page.waitFor('document.querySelector(".session-lookup")?.dataset.previewOpen === "false"');
  }
}
async function selectPreview(title, page = c) {
  await closeDetail(page);
  await page.clickExpression(row(title) + '.querySelector(".session-index__item")');
  await page.waitFor('document.querySelector(".session-preview h2")?.textContent === ' + JSON.stringify(title));
  assert.equal(await page.evaluate('Boolean(' + readerScope + ') || Boolean(document.querySelector(".session-reading-pane"))'), false, 'selecting a session opens its preview');
}
async function readPreview(page = c) {
  await page.click('.session-preview__read-actions button:last-child');
  await page.waitFor('Boolean(document.querySelector(".session-reading-pane .session-reader"))');
}
async function selectInline(title, page = c) {
  await selectPreview(title, page);
  await readPreview(page);
  await page.waitFor('document.querySelector(".session-reading-pane .session-reader__identity h2")?.textContent === ' + JSON.stringify(title));
  assert.equal(await page.evaluate('Boolean(' + readerScope + ')'), false, 'full reading opens the inline reader');
}
async function select(title, page = c) {
  if (await detailTitle(page) !== title) {
    await selectInline(title, page);
    await page.click('.session-reading-pane .session-reader__expand');
  }
  await page.waitFor(readerScope + '?.querySelector(".xy-ui-modal__title")?.textContent === ' + JSON.stringify(title));
}
async function star(title, page = c, keepOpen = false) {
  if (keepOpen) await select(title, page);
  else await selectPreview(title, page);
  const scope = keepOpen ? reader : '.session-preview';
  const wasFavorite = await page.evaluate('document.querySelector(' + JSON.stringify(scope + ' .session-favorite') + ').getAttribute("aria-pressed") === "true"');
  await page.click(scope + ' .session-favorite');
  await page.waitFor('Boolean(' + row(title) + '?.querySelector(".session-index__title svg")) === ' + !wasFavorite);
  if (!keepOpen) await closeDetail(page);
}
async function tabTo(selector, page = c) {
  const focused = 'document.activeElement === document.querySelector(' + JSON.stringify(selector) + ')';
  for (let step = 0; step < 30; step++) {
    if (await page.evaluate(focused)) return;
    await page.key('Tab');
  }
  assert.fail('Control is not keyboard reachable: ' + selector);
}
async function readerPanel(panel, scope = reader, page = c) {
  const target = scope + (panel === 'discussion' ? ' .session-discussion' : ' .session-reader__resume-panel');
  const visible = 'Boolean(document.querySelector(' + JSON.stringify(target) + ')?.getClientRects().length)';
  if (!await page.evaluate(visible)) await page.click(scope + ' .session-reader__panel-toggles button:' + (panel === 'discussion' ? 'first' : 'last') + '-child');
  await page.waitFor(visible);
}
async function closeReaderPanel(scope = reader, page = c) {
  const toggle = scope + ' .session-reader__panel-toggles button[aria-expanded="true"]';
  if (await page.evaluate('Boolean(document.querySelector(' + JSON.stringify(toggle) + ')?.getClientRects().length)')) {
    await page.click(toggle);
    await page.waitFor('document.querySelector(' + JSON.stringify(scope + ' .session-reader__workspace') + ').dataset.panel === "closed"');
  }
}
async function disclose(selector, page = c) {
  if (!await page.evaluate('document.querySelector(' + JSON.stringify(selector) + ')?.open')) await page.click(selector + ' > summary');
  await page.waitFor('document.querySelector(' + JSON.stringify(selector) + ')?.open === true');
}
async function refreshIndex() {
  await closeDetail();
  await c.button('刷新索引', 'document.querySelector(".session-browser__heading")');
  await c.waitFor('!document.querySelector(".session-browser__heading button").disabled');
}
async function prepareLongSession() {
  const title = 'QA session with a long title '.repeat(24).trim();
  await c.evaluate('window.__XY_QA__.sessions[0].title = ' + JSON.stringify(title) + '; window.__XY_QA__.sessions[0].projectDir = "QA:/" + "long-folder-name/".repeat(30); window.__XY_QA__.sessions[0].resumeCommand = null; window.__XY_QA__.sessionMessages = [{ role: "assistant", content: "Long synthetic transcript ".repeat(400) }]; window.__XY_QA__.sessionMessageWarnings = ["QA trailing message could not be read"]');
  await refreshIndex();
  return title;
}
async function beginDelete() {
  await readerPanel('resume');
  await disclose(reader + ' .session-source');
  await c.button('删除会话', readerScope);
  await c.waitFor('Boolean(' + confirmationScope + ')');
}
async function reset({ raw = null, width = 1280, height = width < 500 ? 844 : 820, language = 'zh-CN', theme = 'light', catalogSize = 4 } = {}) {
  await c.evaluate('localStorage.removeItem(' + JSON.stringify(key) + '); localStorage.setItem("xy-session-qa:language", ' + JSON.stringify(language) + '); localStorage.setItem("xy-switch:theme", ' + JSON.stringify(theme) + '); ' + (raw === null ? '' : 'localStorage.setItem(' + JSON.stringify(key) + ', ' + JSON.stringify(raw) + ');'));
  await c.viewport(width, height);
  await c.navigate(base + '/');
  await workspace('sessions');
  await c.waitFor('document.querySelectorAll(".session-index__row").length === ' + catalogSize);
  assert.equal(await c.evaluate('document.querySelector(".sessions-page h1").textContent'), language === 'en' ? 'Sessions' : '会话');
}
async function noOverflow() {
  const metrics = await c.evaluate('({ width: innerWidth, root: document.documentElement.scrollWidth, body: document.body.scrollWidth })');
  assert(metrics.root <= metrics.width && metrics.body <= metrics.width, JSON.stringify(metrics));
}
async function readerGeometry(name, expanded = false) {
  const metrics = await c.evaluate(`(() => {
    const scope = document.querySelector(${JSON.stringify(expanded ? reader : '.session-reading-pane')});
    const bounds = element => { const rect = element.getBoundingClientRect(); return { top: rect.top, bottom: rect.bottom, height: rect.height, width: rect.width }; };
    const shown = element => Boolean(element?.getClientRects().length) && getComputedStyle(element).visibility !== 'hidden';
    const messages = scope.querySelector('.session-messages');
    const source = scope.querySelector('.session-source');
    return {
      width: innerWidth, height: innerHeight,
      firstMessage: bounds(messages.querySelector('.session-message')),
      messages: bounds(messages),
      side: bounds(scope.querySelector('.session-reader__side')),
      panel: scope.querySelector('.session-reader__workspace').dataset.panel,
      title: bounds(scope.querySelector('.session-reader__identity h2, .xy-ui-modal__title')),
      main: bounds(document.querySelector('.xy-shell-main')),
      pageHeadingVisible: shown(document.querySelector('.session-browser__heading')),
      directoryVisible: shown(document.querySelector('.session-directory')),
      backVisible: shown(scope.querySelector('.session-reader__back')),
      sourceExpanded: source.open,
      transcriptOverflowY: getComputedStyle(messages).overflowY,
      sourceOverflowY: getComputedStyle(source.querySelector('.workspace-disclosure__body')).overflowY,
      scrollRegions: [...document.querySelectorAll('.xy-shell-main, .session-reading-pane, .session-reader, .session-reader *, .session-reader-modal .xy-ui-modal__body, .session-reader-modal .xy-ui-modal__header > div')]
        .filter(element => shown(element) && /^(auto|scroll)$/.test(getComputedStyle(element).overflowY) && element.scrollHeight > element.clientHeight + 1)
        .map(element => ({ className: element.className, ...bounds(element), clientHeight: element.clientHeight, scrollHeight: element.scrollHeight, scrollTop: element.scrollTop }))
    };
  })()`);
  await writeFile(join(output, name + '-geometry.json'), JSON.stringify(metrics, null, 2));
  return metrics;
}
async function modalFits(role = 'dialog') {
  const scope = role === 'dialog' ? readerScope : confirmationScope;
  const metrics = await c.evaluate('(() => { const panel = ' + scope + '; const rect = panel.getBoundingClientRect(); const body = panel.querySelector(".xy-ui-modal__body"); const buttons = [...panel.querySelectorAll(".xy-ui-modal__footer button, .xy-ui-modal__header button")].map(element => { const bounds = element.getBoundingClientRect(); return { label: element.textContent, top: bounds.top, bottom: bounds.bottom, left: bounds.left, right: bounds.right }; }); return { width: innerWidth, height: innerHeight, top: rect.top, bottom: rect.bottom, bodyWidth: body.clientWidth, bodyScrollWidth: body.scrollWidth, buttons }; })()');
  assert(metrics.top >= -1 && metrics.bottom <= metrics.height + 1, 'the dialog fits the window: ' + JSON.stringify(metrics));
  assert(metrics.bodyScrollWidth <= metrics.bodyWidth + 1, 'reader content wraps: ' + JSON.stringify(metrics));
  assert(metrics.buttons.length > 0, 'the dialog has reachable actions');
  assert(metrics.buttons.every(button => button.top >= 0 && button.bottom <= metrics.height + 1 && button.left >= 0 && button.right <= metrics.width + 1), 'dialog actions remain reachable: ' + JSON.stringify(metrics));
}
async function withExperience(work) {
  const { identifier } = await c.call('Page.addScriptToEvaluateOnNewDocument', { source: experienceFixture });
  try { await work(); } finally { await c.call('Page.removeScriptToEvaluateOnNewDocument', { identifier }); }
}
async function experienceGeometry(name) {
  const metrics = await c.evaluate(`(() => {
    const reader = document.querySelector('.session-reading-pane .session-reader');
    const rect = element => { const box = element.getBoundingClientRect(); return { x: box.x, y: box.y, width: box.width, height: box.height, bottom: box.bottom }; };
    const visible = element => Boolean(element?.getClientRects().length);
    const messages = reader.querySelector('.session-messages');
    const discussion = reader.querySelector('.session-discussion');
    const context = reader.querySelector('.session-resume-context');
    const code = context.querySelector('.session-resume-context__path');
    return { viewport: innerWidth, container: getComputedStyle(document.querySelector('.sessions-page')).containerName,
      reader: rect(reader), messages: rect(messages), firstMessage: rect(messages.querySelector('.session-message')),
      firstBody: rect(messages.querySelector('pre')), fontSize: getComputedStyle(messages.querySelector('pre')).fontSize,
      side: rect(reader.querySelector('.session-reader__side')), discussion: rect(discussion),
      discussionToggle: visible(reader.querySelector('.session-reader__panel-toggles button')),
      gap: getComputedStyle(reader.querySelector('.session-reader__workspace')).columnGap,
      directoryVisible: visible(document.querySelector('.session-directory')), sourceExpanded: reader.querySelector('.session-source').open,
      resume: rect(reader.querySelector('.session-reader__resume')), context: rect(context), contextText: context.innerText,
      path: code.textContent, pathBounds: rect(code), pathWidth: code.clientWidth, pathScrollWidth: code.scrollWidth,
      mainWidth: document.querySelector('.xy-shell-main').clientWidth, mainScrollWidth: document.querySelector('.xy-shell-main').scrollWidth };
  })()`);
  await writeFile(join(output, name + '-geometry.json'), JSON.stringify(metrics, null, 2));
  return metrics;
}
async function completeTranscript() {
  assert.deepEqual(await c.evaluate('[...document.querySelectorAll(".session-reading-pane .session-message pre")].map(element => element.textContent)'), await c.evaluate('window.__XY_QA__.sessionMessages.map(message => message.content)'), 'all loaded message bodies, including tool events, stay complete');
}
async function closeQaPage(page, phase) {
  try { await page.close(); } catch (error) {
    cleanupFailures.push({ phase, error: String(error), stack: error.stack });
    console.error('FAIL ' + phase + ': ' + error);
    await writeFile(join(output, 'cleanup-failures.json'), JSON.stringify(cleanupFailures, null, 2));
  }
}
async function layoutDiagnostics() {
  return c.evaluate(`(() => {
    const selectors = ['.xy-shell-main', '.session-browser__heading', '.session-toolbar', '.session-lookup', '.session-results', '.session-library', '.session-index__item[aria-current=true]', '.session-preview', '.session-preview__read-actions', '.session-reading-pane', '.session-messages', '.session-reader__side'];
    return { width: innerWidth, height: innerHeight, active: document.activeElement?.outerHTML.slice(0, 600), boxes: selectors.map(selector => {
      const element = document.querySelector(selector); if (!element) return { selector, missing: true };
      const rect = element.getBoundingClientRect();
      const hit = document.elementFromPoint(rect.left + rect.width / 2, rect.top + rect.height / 2);
      return { selector, x: rect.x, y: rect.y, width: rect.width, height: rect.height, clientHeight: element.clientHeight, scrollHeight: element.scrollHeight, scrollTop: element.scrollTop, overflowY: getComputedStyle(element).overflowY, centerHit: hit?.className };
    }) };
  })()`);
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
    reports.push({ name, passed: false, error: String(error), stack: error.stack });
    await c.screenshot(join(output, name + '-failure.png'));
    await writeFile(join(output, name + '-failure.json'), JSON.stringify({ snapshot: await c.snapshot(), layout: await layoutDiagnostics(), exceptions: c.exceptions.slice(exceptionStart) }, null, 2));
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
    assert.equal(await detailTitle(), null, 'the reader opens only on request');
    await c.waitFor('document.querySelectorAll(".session-preview .session-excerpt").length === 2');
    assert.deepEqual(await c.evaluate('window.__XY_QA__.calls.filter(call => call.command === "get_session_detail").map(call => call.args)'), [{ providerId: 'codex', sessionId: 'qa-recent' }], 'only the selected preview loads a transcript');
    assert.equal(await c.evaluate('Boolean(document.querySelector(".session-toolbar__client select")?.getClientRects().length)'), true, 'client filter is directly available');
    assert.equal(await c.evaluate('[...document.querySelectorAll(".session-index__row")].every(element => element.querySelectorAll("button").length === 1)'), true);
    await star('Older Codex session', c, true);
    assert.equal((await rows())[0], 'Older Codex session');
    assert.equal(await detailTitle(), 'Older Codex session', 'favoriting keeps the open reader');
    assert.equal(await c.evaluate(readerScope + '.querySelector(".session-source").open'), false);
    await c.waitFor(readerScope + '.querySelectorAll(".session-message").length === 2');
    await readerPanel('resume');
    await c.button('复制恢复命令', readerScope);
    await c.waitFor('window.__XY_QA__.clipboard === "codex --resume qa-shared"');
    await c.waitFor(readerScope + '.innerText.includes("恢复命令已复制")');
    await closeDetail();
    assert.deepEqual(await keys(), [oldKey]);
    await star('Claude planning notes');
    assert.deepEqual(await keys(), [claudeKey, oldKey].sort(), 'client-scoped identical IDs');
    await star('Older Codex session');
    assert.deepEqual(await keys(), [claudeKey]);
    await star('Older Codex session');
    await workspace('providers'); await workspace('sessions');
    await c.waitFor(favoriteRows + ' === 2');
    assert.equal(await detailTitle(), null);
    await c.navigate(base + '/'); await workspace('sessions');
    await c.waitFor(favoriteRows + ' === 2');
    assert.equal(await c.evaluate('document.querySelectorAll(".session-index button button").length'), 0);
    assert.equal(await detailTitle(), null);
    assert(!(await c.evaluate('localStorage.getItem(' + JSON.stringify(key) + ')')).match(/QA:|title|summary|sourcePath|projectDir/));
    await c.screenshot(join(output, 'favorites-desktop-zh-light.png'));
  });
  await run('filters-and-keyboard', async () => {
    await reset({ raw: saved([oldKey, claudeKey, ghostKey]) });
    assert.equal(await c.evaluate('document.querySelector(".session-favorites-filter > span[aria-hidden]:last-child").textContent'), '2');
    await c.click(favoritesFilter);
    assert.equal((await rows()).length, 2);
    await c.fill('.session-toolbar__client select', 'codex');
    await c.fill('.session-toolbar__project select', JSON.stringify('QA:/workspace/favorites'));
    await c.fill('.session-browser__heading input', '  FAVORITES  ');
    assert.deepEqual(await rows(), ['Older Codex session']);
    await c.fill('.session-browser__heading input', 'no matching session');
    assert((await visibleText()).includes('没有匹配的会话'));
    await focusSearch(); await c.key('Escape');
    assert.deepEqual(await rows(), ['Older Codex session']);
    assert.equal(await c.evaluate('document.querySelector(".session-favorites-filter").getAttribute("aria-pressed")'), 'true');
    assert.equal(await c.evaluate('document.querySelector(".session-toolbar__client select").value'), 'codex');
    assert.equal(await c.evaluate('document.querySelector(".session-toolbar__project select").value'), JSON.stringify('QA:/workspace/favorites'));
    await c.button('清除筛选');
    assert.equal((await rows()).length, 4);
    assert.deepEqual(await c.evaluate('[...document.querySelectorAll(".session-toolbar select")].map(element => element.value)'), ['all', 'all']);
    assert.equal(await c.evaluate('document.querySelector(".session-favorites-filter").getAttribute("aria-pressed")'), 'false');
    assert.equal(await c.evaluate('document.activeElement.getAttribute("aria-label")'), '搜索会话');
    await c.fill('.session-browser__heading input', 'composing');
    await c.evaluate('document.activeElement.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", isComposing: true, bubbles: true }))');
    assert.equal(await c.evaluate('document.activeElement.value'), 'composing');
    await c.key('Escape');
    assert.equal(await c.evaluate('document.activeElement.value'), '');
    await c.evaluate(row('Older Codex session') + '.querySelector(".session-index__item").focus()');
    await c.key('Enter');
    await c.waitFor('document.querySelector(".session-preview h2")?.textContent === "Older Codex session"');
    assert.equal(await c.evaluate('Boolean(document.querySelector(".session-reading-pane, [role=dialog]"))'), false, 'keyboard selection opens the preview');
    await tabTo('.session-preview__read-actions button:last-child');
    await c.key('Enter');
    await c.waitFor('document.activeElement === document.querySelector(".session-reading-pane .session-reader__identity h2")');
    await tabTo('.session-reading-pane .session-reader__expand');
    await c.key('Enter');
    await c.waitFor('Boolean(' + readerScope + ')');
    assert.equal(await detailTitle(), 'Older Codex session');
    await tabTo(reader + ' .session-favorite');
    assert.equal(await c.evaluate('document.activeElement.classList.contains("session-favorite")'), true, 'reader actions are keyboard reachable');
    await c.key('Enter');
    assert.equal(await isFavorite('Older Codex session'), false);
    assert.equal(await c.evaluate('document.activeElement.classList.contains("session-favorite")'), true, 'keyboard focus survives row reordering');
    assert.equal(await detailTitle(), 'Older Codex session');
    await c.key('Escape');
    await c.waitFor('!' + readerScope);
    assert.equal(await c.evaluate('document.activeElement.classList.contains("session-reader__expand")'), true, 'closing expanded reading restores the expand action');
    await c.viewport(640, 820);
    await c.waitFor('Boolean(document.querySelector(".session-reader__back")?.getClientRects().length)');
    await closeDetail();
    assert.equal(await c.evaluate('document.activeElement.getAttribute("aria-label")'), '预览会话 Older Codex session', 'returning to the directory restores the selected row');
  });
  await run('cross-window-selection', async () => {
    await reset();
    await select('Recent Codex session');
    const peer = await connect(undefined, { fresh: true });
    try {
      await peer.call('Page.addScriptToEvaluateOnNewDocument', { source });
      await peer.navigate(base + '/'); await workspace('sessions', peer);
      await peer.waitFor('document.querySelectorAll(".session-index__row").length === 4');
      await star('Older Codex session', peer);
      await c.waitFor(favoriteRows + ' === 1');
      assert.equal((await rows())[0], 'Older Codex session');
      assert.equal(await detailTitle(), 'Recent Codex session', 'another window must not change the current detail');
      await star('Claude planning notes', peer);
      await c.waitFor(favoriteRows + ' === 2');
      assert.deepEqual(await keys(), [oldKey, claudeKey].sort());
      assert.equal(await detailTitle(), 'Recent Codex session');
      await c.call('Page.bringToFront'); await c.viewport(820, 820);
      assert.equal(await detailTitle(), 'Recent Codex session', 'resizing keeps the focused reader');
      await closeDetail();
      await c.click(favoritesFilter); await select('Older Codex session');
      await star('Older Codex session', peer);
      await c.waitFor('!' + readerScope);
      assert.equal(await c.evaluate('document.activeElement === document.querySelector(".session-index__item[aria-current=true]")'), true);
      await peer.evaluate('localStorage.clear()');
      await c.waitFor(favoriteRows + ' === 0');
      const systemLanguage = await c.evaluate('(navigator.languages?.[0] ?? navigator.language).toLowerCase().startsWith("zh") ? "zh-CN" : "en"');
      await c.waitFor('document.documentElement.lang === ' + JSON.stringify(systemLanguage));
      const emptyMessage = systemLanguage === 'zh-CN' ? '还没有收藏的会话' : 'No favorite sessions yet';
      await c.waitFor('document.body.innerText.includes(' + JSON.stringify(emptyMessage) + ')');
      assert.equal(await c.evaluate('localStorage.getItem(' + JSON.stringify(key) + ')'), null);
      assert.deepEqual(await keys(), []);
      assert.equal(await c.evaluate('document.querySelector(".session-favorites-filter").getAttribute("aria-pressed")'), 'true');
      assert.equal(peer.exceptions.length, 0);
    } finally { await closeQaPage(peer, 'peer-page-cleanup'); }
  });
  await run('storage-failure-and-retry', async () => {
    await reset({ raw: saved([ghostKey]) });
    await c.evaluate(`window.__restoreStorage = (() => { const original = Storage.prototype.setItem; window.__saveFavorites = value => original.call(localStorage, ${JSON.stringify(key)}, value); Storage.prototype.setItem = function(k, v) { if (k === ${JSON.stringify(key)}) throw new DOMException('QA storage full', 'QuotaExceededError'); return original.call(this, k, v); }; return () => { Storage.prototype.setItem = original; }; })()`);
    try {
      await star('Older Codex session', c, true);
      assert.equal(await isFavorite('Older Codex session'), true);
      assert.deepEqual(await keys(), [ghostKey]);
      assert((await c.evaluate(readerScope + '.innerText')).includes('本地收藏暂不可用'), 'save failures are visible inside the focused reader');
      await c.evaluate(`window.__saveFavorites(${JSON.stringify(saved([ghostKey, claudeKey]))}); window.__restoreStorage()`);
      await c.button('重试', readerScope);
      assert.deepEqual(await keys(), [ghostKey, claudeKey, oldKey].sort());
      assert(!(await visibleText()).includes('本地收藏暂不可用'));
    } finally { await c.evaluate('window.__restoreStorage?.()'); }
    await c.evaluate(`window.__restoreStorage = (() => { const original = Storage.prototype.getItem; Storage.prototype.getItem = function(k) { if (k === ${JSON.stringify(key)}) throw new DOMException('QA read blocked', 'SecurityError'); return original.call(this, k); }; return () => { Storage.prototype.getItem = original; }; })()`);
    try {
      await star('Older Codex session', c, true);
      assert.equal(await isFavorite('Older Codex session'), false);
      assert((await c.evaluate(readerScope + '.innerText')).includes('本地收藏暂不可用'), 'save failures are visible inside the focused reader');
      await c.evaluate('window.__restoreStorage()'); await c.button('重试', readerScope);
      assert.deepEqual(await keys(), [ghostKey, claudeKey].sort());
    } finally { await c.evaluate('window.__restoreStorage?.()'); }
  });
  await run('invalid-storage-and-missing-catalog', async () => {
    const future = '{"version":2,"keys":["future-format"]}';
    await reset({ raw: future });
    assert((await visibleText()).includes('收藏记录格式无法识别'));
    await star('Older Codex session', c, true); await c.button('重试', readerScope);
    assert.equal(await c.evaluate(`localStorage.getItem(${JSON.stringify(key)})`), future);
    assert.equal(await isFavorite('Older Codex session'), true);
    await c.evaluate(`localStorage.setItem(${JSON.stringify(key)}, ${JSON.stringify(saved([claudeKey, ghostKey]))})`);
    await c.button('重试', readerScope);
    assert.deepEqual(await keys(), [oldKey, claudeKey, ghostKey].sort());
    await c.evaluate('window.__XY_QA__.sessions = window.__XY_QA__.sessions.filter(s => s.sessionId !== "qa-shared" || s.providerId !== "codex")');
    await refreshIndex();
    await c.waitFor('document.querySelectorAll(".session-index__row").length === 3');
    assert.deepEqual(await keys(), [oldKey, claudeKey, ghostKey].sort(), 'a partial catalog does not erase favorites');
    await c.evaluate(`window.__XY_QA__.sessions = ${JSON.stringify(sessions)}`); await refreshIndex();
    await c.waitFor(favoriteRows + ' === 2');
  });
  await run('deletion-preserves-until-success', async () => {
    await reset({ raw: saved([oldKey, claudeKey]) });
    await select('Older Codex session');
    await c.evaluate('window.__XY_QA__.fail.delete_sessions = "QA session file changed"');
    await beginDelete();
    await c.button('取消', confirmationScope);
    await c.waitFor('Boolean(' + readerScope + ')');
    assert.equal(await detailTitle(), 'Older Codex session');
    assert.equal(await c.evaluate('window.__XY_QA__.calls.filter(call => call.command === "delete_sessions").length'), 0, 'cancelling does not delete');
    await beginDelete(); await c.button('确认删除', confirmationScope);
    await c.waitFor('document.body.innerText.includes("QA session file changed")');
    assert.deepEqual(await keys(), [oldKey, claudeKey].sort());
    assert.equal((await rows()).length, 4);
    await c.evaluate('delete window.__XY_QA__.fail.delete_sessions');
    for (const returned of [0, -1, null, 'unexpected']) {
      const calls = await c.evaluate('window.__XY_QA__.calls.filter(call => call.command === "delete_sessions").length');
      await c.evaluate('window.__XY_QA__.deleteCount = ' + JSON.stringify(returned));
      await c.button('确认删除', confirmationScope);
      await c.waitFor('window.__XY_QA__.calls.filter(call => call.command === "delete_sessions").length === ' + (calls + 1));
      await c.waitFor(confirmationScope + '?.innerText.includes("后端未删除任何会话")');
      assert.deepEqual(await keys(), [oldKey, claudeKey].sort(), 'invalid deletion responses preserve favorites');
      assert.equal((await rows()).length, 4, 'invalid deletion responses preserve the catalog');
    }
    await c.evaluate('window.__XY_QA__.deleteCount = 1'); await c.button('确认删除', confirmationScope);
    await c.waitFor('!document.querySelector("[role=alertdialog]") && document.querySelectorAll(".session-index__row").length === 3');
    assert.deepEqual(await keys(), [claudeKey]);
    assert.equal(await isFavorite('Claude planning notes'), true);
    assert.deepEqual(await c.evaluate('window.__XY_QA__.calls.filter(call => call.command === "delete_sessions").at(-1).args'), { ids: [{ providerId: 'codex', sessionId: 'qa-shared' }] });
  });
  await run('compact-drawer-and-empty-state', async () => {
    await reset({ width: 640, raw: saved([oldKey, claudeKey]) });
    await star('Recent Codex session');
    assert.equal(await c.evaluate('Boolean(document.querySelector("[role=dialog]"))'), false, 'closing a reader returns to the list');
    await select('Older Codex session');
    await c.waitFor('Boolean(document.querySelector("[role=dialog]"))');
    assert.equal(await detailTitle(), 'Older Codex session');
    await closeDetail();
    assert.equal(await c.evaluate('document.activeElement.getAttribute("aria-label")'), '预览会话 Older Codex session');
    await c.click(favoritesFilter); await select('Older Codex session'); await c.click(reader + ' .session-favorite');
    await c.waitFor('!document.querySelector("[role=dialog]")');
    assert.equal(await c.evaluate('document.activeElement === document.querySelector(".session-index__item[aria-current=true]")'), true, 'a removed session returns focus to the selected result');
    assert.deepEqual(await rows(), ['Recent Codex session', 'Claude planning notes']);
    await star('Recent Codex session'); await star('Claude planning notes');
    assert((await visibleText()).includes('还没有收藏的会话'));
    assert.equal(await c.evaluate('document.activeElement === document.querySelector(".session-browser__heading input")'), true);
    await c.button('清除筛选');
    assert.equal((await rows()).length, 4);
    assert.equal(await c.evaluate('document.activeElement.getAttribute("aria-label")'), '搜索会话');
  });
  await run('focused-reader-boundaries', async () => {
    await reset({ width: 390, height: 844 });
    await selectInline('Recent Codex session');
    await c.waitFor('document.querySelector(".session-reading-pane").querySelectorAll(".session-message").length === 2');
    const narrow = await readerGeometry('reader-inline-390-light');
    assert.equal(narrow.pageHeadingVisible, false, 'directory refresh is absent from narrow reading');
    assert.equal(narrow.directoryVisible, false);
    assert.equal(narrow.sourceExpanded, false);
    assert(narrow.firstMessage.top > narrow.title.bottom && narrow.firstMessage.bottom <= narrow.height, 'the first message remains visible below the reading controls: ' + JSON.stringify(narrow));
    assert.equal(narrow.transcriptOverflowY, 'auto', 'the narrow transcript has its own reading scroll');
    assert.equal(narrow.panel, 'closed', 'narrow reading starts with the auxiliary panel closed');
    assert(narrow.messages.height > 0);
    await noOverflow();
    await c.screenshot(join(output, 'reader-inline-390-light.png'));
    assert((await c.evaluate('document.querySelector(".session-reading-pane .session-messages").innerText')).includes('已保留全部审计证据。\nconst result = true;'), 'message notes and code remain complete');
    await c.fill('.session-reading-pane .session-find input', '审计');
    await c.waitFor('document.querySelectorAll(".session-reading-pane .session-message[data-search-current=true] mark").length === 1');
    await c.click('.session-reading-pane .session-find input');
    await c.key('Escape');
    assert.equal(await c.evaluate('document.querySelector(".session-reading-pane .session-find input").value'), '');
    await c.click('.session-reading-pane .session-favorite');
    assert.deepEqual(await keys(), [pair('codex', 'qa-recent')]);
    await readerPanel('resume', '.session-reading-pane');
    await c.button('复制恢复命令', 'document.querySelector(".session-reading-pane")');
    await c.waitFor('window.__XY_QA__.clipboard === "codex --resume qa-recent"');
    await disclose('.session-reading-pane .session-source');
    const sourceText = await c.evaluate('document.querySelector(".session-reading-pane .session-source").innerText');
    for (const value of ['Recent Codex session', 'QA:/workspace/recent', 'qa-recent', 'QA:/sessions/codex/qa-recent.jsonl', 'codex --resume qa-recent', '恢复命令只复制，不执行']) assert(sourceText.includes(value), 'complete source information: ' + value);
    const disclosed = await readerGeometry('reader-inline-390-source');
    assert.equal(disclosed.sourceOverflowY, 'visible');
    assert(disclosed.side.height > 0 && disclosed.messages.height > 0, 'source management leaves both reader regions usable');
    assert(disclosed.scrollRegions.some(region => region.className === 'session-reader__side'), 'expanded source information scrolls in the auxiliary region');
    assert(disclosed.scrollRegions.every(region => ['session-reader__side', 'session-messages'].includes(region.className)), 'source management does not add a page scroll region');
    await c.button('删除会话', 'document.querySelector(".session-reading-pane")');
    await c.waitFor('Boolean(' + confirmationScope + ')');
    for (const warning of ['Recent Codex session', '不可撤销', '请先停止该会话', '不会修改客户端的索引或数据库']) assert((await c.evaluate(confirmationScope + '.innerText')).includes(warning));
    await modalFits('alertdialog');
    await c.screenshot(join(output, 'reader-inline-390-delete-confirmation.png'));
    await c.button('取消', confirmationScope);
    await c.waitFor('!' + confirmationScope);
    assert.equal(await c.evaluate('document.activeElement.textContent.trim()'), '删除会话', 'cancelling inline deletion returns to its trigger');
    assert.equal(await c.evaluate('window.__XY_QA__.calls.filter(call => call.command === "delete_sessions").length'), 0);
    assert.deepEqual(await keys(), [pair('codex', 'qa-recent')]);
    await closeDetail();
    assert.equal(await c.evaluate('document.activeElement.getAttribute("aria-label")'), '预览会话 Recent Codex session');
    assert.equal(await c.evaluate('Boolean(document.querySelector(".session-browser__heading").getClientRects().length)'), true, 'directory refresh returns with the directory');
  });
  await run('short-window-directory-access', async () => {
    await reset({ width: 390, height: 360 });
    const longTitle = await prepareLongSession();
    assert(await c.evaluate('document.querySelector(".session-library").clientHeight > 0'), 'the short-window directory must leave visible space for session rows');
    await selectInline(longTitle);
    await c.waitFor('document.querySelector(".session-reading-pane").querySelectorAll(".session-message").length === 1');
    const opened = await readerGeometry('reader-from-short-directory');
    assert(opened.title.top >= opened.main.top - 1 && opened.title.bottom <= opened.main.bottom + 1, 'opening a session from a scrolled directory reveals the reading title');
    await c.evaluate('document.querySelector(".session-reading-pane .session-messages").scrollTop = document.querySelector(".session-reading-pane .session-messages").scrollHeight');
    await closeDetail();
    const returnedFocus = await c.evaluate('(() => { const rect = document.activeElement.getBoundingClientRect(); const main = document.querySelector(".xy-shell-main").getBoundingClientRect(); return { title: document.activeElement.getAttribute("aria-label"), visible: rect.height > main.height ? rect.top < main.bottom && rect.bottom > main.top : rect.top >= main.top - 1 && rect.bottom <= main.bottom + 1 }; })()');
    assert.equal(returnedFocus.title, '预览会话 ' + longTitle);
    assert.equal(returnedFocus.visible, true, 'returning from scrolled content reveals the focused session row');
  });
  await run('short-window-reader-content', async () => {
    await reset({ width: 390, height: 844 });
    const longTitle = await prepareLongSession();
    await selectInline(longTitle);
    await c.waitFor('document.querySelector(".session-reading-pane").querySelectorAll(".session-message").length === 1');
    await c.viewport(390, 360);
    const longInline = await readerGeometry('reader-inline-390x360-long-content');
    assert(longInline.title.height <= 52, 'long titles occupy at most two heading lines');
    assert(longInline.title.top >= longInline.main.top - 1 && longInline.title.bottom <= longInline.main.bottom + 1, 'short inline reading keeps its title inside the reading pane');
    assert.deepEqual(longInline.scrollRegions.map(region => region.className), ['session-messages'], 'long inline content scrolls in its transcript region');
    await noOverflow();
    await c.screenshot(join(output, 'reader-inline-390x360-long-content.png'));
    const transcriptScrolled = await c.evaluate('(() => { const messages = document.querySelector(".session-reading-pane .session-messages"); messages.scrollTop = messages.scrollHeight; return messages.scrollTop > 0; })()');
    assert.equal(transcriptScrolled, true);
    await c.click('.session-reading-pane .session-reader__expand');
    await c.waitFor('Boolean(' + readerScope + ')');
    await c.waitFor(readerScope + '.querySelectorAll(".session-message").length === 1');
    await noOverflow(); await modalFits();
    await readerPanel('resume');
    assert.equal(await c.evaluate(readerScope + '.querySelector(".session-reader__resume").disabled'), true, 'no resume command is not presented as a working action');
    await closeReaderPanel();
    assert.equal(await c.evaluate(readerScope + '.querySelector(".session-read-warnings").open'), false);
    await disclose(reader + ' .session-read-warnings');
    assert((await c.evaluate(readerScope + '.innerText')).includes('QA trailing message could not be read'));
    const scroll = await c.evaluate('(() => { const messages = ' + readerScope + '.querySelector(".session-messages"); messages.scrollTop = messages.scrollHeight; return { moved: messages.scrollTop > 0, scrollable: messages.scrollHeight > messages.clientHeight }; })()');
    assert.deepEqual(scroll, { moved: true, scrollable: true });
    const longExpanded = await readerGeometry('reader-expanded-390x360-long-content', true);
    assert(longExpanded.messages.height > 0 && longExpanded.scrollRegions.some(region => region.className === 'session-messages'), 'expanded narrow content keeps a usable transcript scroll region');
    assert(longExpanded.scrollRegions.every(region => ['session-messages', 'xy-ui-modal__body'].includes(region.className)), 'expanded reading scroll stays inside the dialog');
    await modalFits();
    await c.screenshot(join(output, 'reader-390x360-long-content.png'));
    await beginDelete();
    await modalFits('alertdialog');
    await c.button('取消', confirmationScope);
    await c.waitFor('Boolean(' + readerScope + ')');
    assert.equal(await detailTitle(), longTitle);
    await closeDetail();
  });
  if (experienceFixture) {
    await run('experience-reader-layout', () => withExperience(async () => {
      await reset({ width: 1440, height: 960, catalogSize: 18 });
      assert.equal(await c.evaluate('document.querySelectorAll(".session-toolbar__project option").length'), 4);
      await c.waitFor('document.querySelectorAll(".session-preview .session-excerpt").length === 2');
      assert.equal(await c.evaluate('window.__XY_QA__.calls.filter(call => call.command === "get_session_detail").length'), 1, 'the directory loads only its selected preview');
      const session = await c.evaluate('window.__XY_QA__.sessions[0]');
      await selectInline(session.title);
      await c.waitFor('document.querySelectorAll(".session-reading-pane .session-message").length === 10');
      await completeTranscript();
      const desktop = await experienceGeometry('experience-reader-1440');
      assert(desktop.container.split(' ').includes('workspace'), 'the shared workspace container name survives');
      assert.equal(desktop.directoryVisible, false);
      assert.equal(desktop.sourceExpanded, false);
      assert(desktop.firstMessage.y <= 250 && desktop.firstBody.y <= 250, 'the first body starts by y=250: ' + JSON.stringify(desktop));
      assert.equal(desktop.fontSize, '15px');
      assert(desktop.firstMessage.width <= 900 && desktop.firstMessage.width >= 600, 'the actual message column stays within its reading width');
      assert.equal(desktop.side.width, 272);
      assert.equal(desktop.gap, '28px');
      assert.equal(desktop.discussionToggle, false);
      assert.equal(desktop.path, session.projectDir);
      for (const text of ['先切换工作目录', '再在终端使用恢复命令', '恢复命令只复制，不执行。']) assert(desktop.contextText.includes(text));
      assert(desktop.resume.y >= desktop.context.y && desktop.resume.bottom <= desktop.context.bottom + 1, 'the resume context is beside its action');
      await noOverflow();
      await c.screenshot(join(output, 'experience-reader-1440.png'));
      await c.viewport(1024, 768);
      await c.evaluate('document.querySelector(".xy-shell-main").scrollTop = 0');
      const medium = await experienceGeometry('experience-reader-1024');
      assert.equal(medium.discussionToggle, medium.reader.width < 940, 'discussion controls follow the available reader width');
      await noOverflow();
      await c.screenshot(join(output, 'experience-reader-1024.png'));
      const compactViewport = Math.round(1024 + 939 - medium.reader.width);
      for (const width of [939, 940]) {
        await c.viewport(Math.round(1024 + width - medium.reader.width), 768);
        await c.waitFor('Math.abs(document.querySelector(".session-reading-pane .session-reader").getBoundingClientRect().width - ' + width + ') < 0.1');
        const boundary = await experienceGeometry('experience-reader-net-' + width);
        assert.equal(boundary.discussionToggle, width < 940);
        if (width === 939) assert.equal(boundary.messages.width, boundary.reader.width, 'the compact reader uses its whole available width');
        if (width === 940) { assert.equal(boundary.side.width, 272); assert.equal(boundary.gap, '28px'); }
      }
      await c.viewport(compactViewport, 768);
      await c.waitFor('Math.abs(document.querySelector(".session-reading-pane .session-reader").getBoundingClientRect().width - 939) < 0.1');
      await readerPanel('discussion', '.session-reading-pane');
      await c.waitFor('document.querySelector(".session-reading-pane .session-reader__workspace").dataset.panel === "discussion"');
      assert.equal(await c.evaluate('document.querySelectorAll(".session-reading-pane .session-discussion__items li").length'), 4, 'only loaded user messages form the discussion');
      await c.click('.session-reading-pane .session-discussion__items li:nth-child(3) button');
      await c.waitFor('document.activeElement === document.querySelectorAll(".session-reading-pane .session-message")[6]');
      const discussionFocus = await c.evaluate('(() => { const bounds = document.activeElement.getBoundingClientRect(); const messages = document.querySelector(".session-reading-pane .session-messages").getBoundingClientRect(); return { index: document.activeElement.dataset.messageIndex, top: bounds.top, bottom: bounds.bottom, transcriptTop: messages.top, transcriptBottom: messages.bottom, panel: document.querySelector(".session-reading-pane .session-reader__workspace").dataset.panel }; })()');
      assert.equal(discussionFocus.panel, 'closed');
      assert(discussionFocus.top >= discussionFocus.transcriptTop && discussionFocus.bottom <= discussionFocus.transcriptBottom + 1, 'discussion navigation reveals and focuses the original message');
      await writeFile(join(output, 'experience-discussion-focus.json'), JSON.stringify(discussionFocus, null, 2));
      await c.fill('.session-reading-pane .session-find input', 'original-store');
      await c.waitFor('document.querySelectorAll(".session-reading-pane .session-message")[5].dataset.searchCurrent === "true"');
      assert.equal(await c.evaluate('document.querySelectorAll(".session-reading-pane .session-message mark").length'), 1);
      await completeTranscript();
      if (!process.env.QA_EXPERIENCE_FIXTURE) {
        assert.deepEqual(await c.evaluate('[...document.querySelectorAll(".session-reading-pane .session-tool")].map(element => ({ event: element.dataset.event, status: element.dataset.status, name: element.querySelector(".session-tool__name").textContent }))'), [
          { event: 'call', status: 'requested', name: 'read_file' },
          { event: 'result', status: 'completed', name: 'read_file' },
          { event: 'approval', status: 'approved', name: 'read_file' },
        ]);
        assert.equal(await c.evaluate('[...document.querySelectorAll(".session-reading-pane .session-tool__id a")].every(element => element.textContent === "qa-call-1" && document.getElementById(element.hash.slice(1))?.dataset.messageIndex === "2")'), true, 'tool results and approvals keep their original call links');
        await c.fill('.session-reading-pane .session-find input', '原始');
        await c.waitFor('document.querySelector(".session-reading-pane [data-search-current=true]")?.dataset.messageIndex === "0"');
        await c.click('.session-reading-pane .session-find input');
        await c.key('Enter');
        await c.waitFor('document.querySelector(".session-reading-pane [data-search-current=true]")?.dataset.messageIndex === "9"');
        await c.key('Enter', 8);
        await c.waitFor('document.querySelector(".session-reading-pane [data-search-current=true]")?.dataset.messageIndex === "0"');
        await c.button('下一条匹配消息', 'document.querySelector(".session-reading-pane")');
        await c.waitFor('document.querySelector(".session-reading-pane [data-search-current=true]")?.dataset.messageIndex === "9"');
        await c.button('上一条匹配消息', 'document.querySelector(".session-reading-pane")');
        await c.waitFor('document.querySelector(".session-reading-pane [data-search-current=true]")?.dataset.messageIndex === "0"');
        await c.fill('.session-reading-pane .session-find input', 'qa-no-matching-message');
        assert.equal(await c.evaluate('document.querySelectorAll(".session-reading-pane .session-message mark").length'), 0);
        assert((await c.evaluate('document.querySelector(".session-reading-pane .session-find__count").textContent')).includes('0 / 0'));
        await c.button('清除正文查找', 'document.querySelector(".session-reading-pane")');
        await completeTranscript();
      }
      await closeDetail();
      const paths = ['D:/Clients/original-store/production/working copies/release validation/customer-facing applications/shared-project', 'E:/Archives/original-store/staging/working copies/release validation/customer-facing applications/shared-project'];
      const commands = paths.map((path, index) => '  ' + ['codex resume', 'claude --resume'][index] + ' "qa session ' + index + '"\n--directory "' + path + '"\n');
      await c.evaluate('window.__XY_QA__.sessions.slice(0, 2).forEach((session, index) => { session.title = "QA complete directory " + index; session.projectDir = ' + JSON.stringify(paths) + '[index]; session.resumeCommand = ' + JSON.stringify(commands) + '[index]; })');
      await refreshIndex();
      const projectPaths = await c.evaluate('[...document.querySelectorAll(".session-toolbar__project option")].filter(element => element.value !== "all").map(element => JSON.parse(element.value))');
      assert(paths.every(path => projectPaths.includes(path)), 'same-name projects retain separate complete paths');
      for (const [index, path] of paths.entries()) {
        await c.fill('.session-toolbar__project select', JSON.stringify(path));
        assert.deepEqual(await rows(), ['QA complete directory ' + index]);
      }
      await c.button('清除筛选');
      for (const width of [1024, 320]) for (const index of [0, 1]) {
        await c.viewport(width, width === 320 ? 900 : 768);
        await selectInline('QA complete directory ' + index);
        await c.waitFor('document.querySelectorAll(".session-reading-pane .session-message").length === 10');
        await readerPanel('resume', '.session-reading-pane');
        const context = await experienceGeometry('experience-context-' + width + '-' + index);
        assert.equal(context.path, paths[index]);
        assert.equal(context.sourceExpanded, false);
        assert.equal(await c.evaluate('document.querySelector(".session-reading-pane .session-resume-context__command").textContent'), commands[index]);
        assert(context.contextText.includes('先切换工作目录') && context.contextText.includes('再在终端使用恢复命令'));
        assert(context.pathScrollWidth <= context.pathWidth + 1 && context.mainScrollWidth <= context.mainWidth + 1, 'complete paths wrap without horizontal scroll');
        if (width === 320) assert(context.pathBounds.height > 24, 'long paths naturally wrap at 320');
        await noOverflow();
        if (width === 320 && index === 0) await c.screenshot(join(output, 'experience-reader-320-long-path.png'));
        const calls = await c.evaluate('window.__XY_QA__.calls.length');
        await c.click('.session-reading-pane .session-resume-context button');
        await c.waitFor('window.__XY_QA__.clipboard === ' + JSON.stringify(paths[index]));
        await c.click('.session-reading-pane .session-reader__resume');
        await c.waitFor('window.__XY_QA__.clipboard === ' + JSON.stringify(commands[index]));
        assert.equal(await c.evaluate('window.__XY_QA__.calls.length'), calls, 'copying directory and verbatim command does not invoke execution');
        await completeTranscript();
      }
    }));
    await run('experience-directory-return', () => withExperience(async () => {
      const favorites = Array.from({ length: 18 }, (_, index) => pair(['codex', 'claude', 'gemini'][index % 3], 'experience-session-' + String(index + 1).padStart(2, '0')));
      await reset({ raw: saved(favorites), width: 1024, height: 480, catalogSize: 18 });
      const query = '  D:/Projects/xy-switch  ';
      const project = JSON.stringify('D:/Projects/xy-switch');
      await c.fill('.session-browser__heading input', query);
      await c.fill('.session-toolbar__project select', project);
      await c.fill('.session-toolbar__client select', 'codex');
      await c.click('.session-favorites-filter');
      await c.waitFor('document.querySelectorAll(".session-index__row").length === 6');
      const before = await c.evaluate('(async () => { window.__sessionReturnTrigger = [...document.querySelectorAll(".session-index__item")].at(-1); window.__sessionReturnTrigger.scrollIntoView({ block: "center", behavior: "instant" }); await new Promise(resolve => requestAnimationFrame(resolve)); return { scrollTop: document.querySelector(".session-library").scrollTop, label: window.__sessionReturnTrigger.getAttribute("aria-label") }; })()');
      assert(before.scrollTop > 0, 'the return scenario starts in a scrolled directory');
      await c.clickExpression('window.__sessionReturnTrigger');
      await c.evaluate('void (window.__sessionReadTrigger = document.querySelector(".session-preview__read-actions button:last-child"))');
      await readPreview();
      await c.waitFor('document.querySelectorAll(".session-reading-pane .session-message").length === 10');
      await c.waitFor('document.activeElement === document.querySelector(".session-reading-pane .session-reader__identity h2")');
      for (const width of [1440, 320, 1024]) {
        await c.viewport(width, 480);
        assert.equal(await c.evaluate('Boolean(document.querySelector(".session-directory").getClientRects().length)'), false);
      }
      await c.evaluate('document.querySelector(".session-reading-pane .session-messages").scrollTop = document.querySelector(".session-reading-pane .session-messages").scrollHeight');
      await closeReader();
      await c.waitFor('document.activeElement === window.__sessionReadTrigger');
      const after = await c.evaluate('(() => { const target = window.__sessionReturnTrigger; const bounds = document.activeElement.getBoundingClientRect(); const preview = document.querySelector(".session-preview").getBoundingClientRect(); return { scrollTop: document.querySelector(".session-library").scrollTop, label: target.getAttribute("aria-label"), query: document.querySelector(".session-browser__heading input").value, project: document.querySelector(".session-toolbar__project select").value, client: document.querySelector(".session-toolbar__client select").value, favoritesOnly: document.querySelector(".session-favorites-filter").getAttribute("aria-pressed"), selected: target.getAttribute("aria-current"), visible: bounds.top >= preview.top && bounds.bottom <= preview.bottom, sameTrigger: document.activeElement === window.__sessionReadTrigger }; })()');
      assert.equal(after.scrollTop, before.scrollTop);
      assert.equal(after.label, before.label);
      assert.equal(after.query, query);
      assert.equal(after.project, project);
      assert.equal(after.client, 'codex');
      assert.equal(after.favoritesOnly, 'true');
      assert.equal(after.selected, 'true');
      assert(after.sameTrigger && after.visible, 'returning from full reading restores its visible preview action');
      assert.deepEqual(await keys(), [...favorites].sort());
      await writeFile(join(output, 'experience-directory-return.json'), JSON.stringify({ before, after }, null, 2));
      await c.viewport(320, 480);
      await closeDetail();
      await c.waitFor('document.activeElement === window.__sessionReturnTrigger');
      assert.equal(await c.evaluate('Boolean(document.querySelector(".session-library").getClientRects().length)'), true, 'returning from the narrow preview restores the selected result');
      await c.viewport(1024, 480);
      await c.clickExpression('window.__sessionReturnTrigger');
      await readPreview();
      await c.waitFor('Boolean(document.querySelector(".session-reading-pane .session-favorite"))');
      await c.click('.session-reading-pane .session-favorite');
      await c.waitFor('document.activeElement === document.querySelector(".session-index__item[aria-current=true]")');
      const removed = await c.evaluate('(() => { const bounds = document.activeElement.getBoundingClientRect(); const directory = document.querySelector(".session-library").getBoundingClientRect(); return { triggerRemoved: !window.__sessionReturnTrigger.isConnected, resultFocused: document.activeElement === document.querySelector(".session-index__item[aria-current=true]"), visible: bounds.top >= directory.top && bounds.bottom <= directory.bottom + 1, query: document.querySelector(".session-browser__heading input").value, project: document.querySelector(".session-toolbar__project select").value, client: document.querySelector(".session-toolbar__client select").value, favoritesOnly: document.querySelector(".session-favorites-filter").getAttribute("aria-pressed"), rows: document.querySelectorAll(".session-index__row").length }; })()');
      assert(removed.triggerRemoved && removed.resultFocused && removed.visible);
      assert.equal(removed.query, query);
      assert.equal(removed.project, project);
      assert.equal(removed.client, 'codex');
      assert.equal(removed.favoritesOnly, 'true');
      assert.equal(removed.rows, 5);
      await writeFile(join(output, 'experience-directory-removal-focus.json'), JSON.stringify(removed, null, 2));
    }));
  }
  await run('bilingual-responsive-layout', async () => {
    for (const language of ['zh-CN', 'en']) for (const theme of ['light', 'dark']) for (const width of [1280, 820, 390]) {
      await reset({ language, theme, width, height: width === 390 ? 480 : 820, raw: saved([oldKey, claudeKey]) });
      await c.click(favoritesFilter);
      await noOverflow();
      assert.equal(await c.evaluate('Boolean(document.querySelector(".session-toolbar__client select")?.getClientRects().length)'), true, 'client filter is directly available');
      if (width === 1280) {
        const centers = await c.evaluate('[document.querySelector(".session-toolbar__client select"), document.querySelector(".session-toolbar__project select"), document.querySelector(".session-favorites-filter")].map(element => { const rect = element.getBoundingClientRect(); return rect.top + rect.height / 2; })');
        assert(Math.max(...centers) - Math.min(...centers) <= 2, 'project, client and favorites controls share the filter row');
      }
      assert.equal(await c.evaluate('Boolean(document.querySelector(".session-toolbar__project select")?.getClientRects().length)'), true, 'project filter is directly available');
      const snapshot = await c.snapshot();
      const label = language === 'en' ? 'Favorites only' : '仅看收藏';
      assert(snapshot.controls.some(control => control.label === label));
      assert((await rows()).includes('Older Codex session'), 'user titles are not translated');
      await c.screenshot(join(output, 'favorites-' + width + '-' + language + '-' + theme + '.png'));
      await selectInline('Older Codex session');
      await c.waitFor('document.querySelector(".session-reading-pane").querySelectorAll(".session-message").length === 2');
      await noOverflow();
      const inlineName = 'reader-inline-' + width + '-' + language + '-' + theme;
      const geometry = await readerGeometry(inlineName);
      assert.equal(geometry.backVisible, !geometry.directoryVisible, 'return to directory is offered only when the directory is hidden');
      if (width === 390) {
        assert.equal(geometry.pageHeadingVisible, false);
        assert(geometry.firstMessage.top <= 320 && geometry.firstMessage.top < geometry.main.bottom, 'narrow reading starts within the first screen: ' + JSON.stringify(geometry));
        assert.equal(geometry.transcriptOverflowY, 'auto');
        assert(geometry.messages.height > 0, 'narrow reading leaves space for the transcript');
        assert(geometry.scrollRegions.every(region => region.className === 'session-messages'), 'narrow reading scroll stays in the transcript');
      }
      assert.equal(geometry.directoryVisible, false, 'reading uses an independent workspace at every width');
      await c.screenshot(join(output, inlineName + '.png'));
      await c.click('.session-reader__expand');
      await c.waitFor('Boolean(' + readerScope + ')');
      await noOverflow(); await modalFits();
      await c.screenshot(join(output, 'reader-' + width + '-' + language + '-' + theme + '.png'));
      await closeDetail();
    }
  });
} finally {
  if (fixtureId) await c.call('Page.removeScriptToEvaluateOnNewDocument', { identifier: fixtureId }).catch(() => {});
  await closeQaPage(c, 'primary-page-cleanup');
  await writeFile(join(output, 'run-summary.json'), JSON.stringify({ reports, cleanupFailures, passed: reports.length > 0 && reports.every(report => report.passed) && cleanupFailures.length === 0 }, null, 2));
}
console.log(`Session QA: ${reports.filter(report => report.passed).length}/${reports.length} business scenarios passed; ${cleanupFailures.length} cleanup failures. Artifacts: ${output}`);
if (!reports.length || reports.some(report => !report.passed) || cleanupFailures.length) process.exitCode = 1;
