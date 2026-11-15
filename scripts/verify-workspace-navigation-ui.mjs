import assert from 'node:assert/strict';
import { mkdir, readFile, writeFile } from 'node:fs/promises';
import { join, resolve } from 'node:path';
import { connect } from './workspace-qa/cdp.mjs';

const base = process.env.APP_URL ?? 'http://127.0.0.1:1431';
const origin = new URL(base).origin;
assert(['127.0.0.1', 'localhost', '[::1]'].includes(new URL(base).hostname), 'Use a local renderer only');
const output = resolve(process.env.QA_OUTPUT ?? 'artifacts/workspace-navigation-qa');
await mkdir(output, { recursive: true });
const c = await connect(undefined, { fresh: true });
const reports = [], screenshots = [], contrasts = [], layouts = [], navigationVisibility = [], providerRecovery = [], providerNavigation = [];
const only = process.env.QA_ONLY?.split(',');
let fixtureId;
const pause = ms => new Promise(resolve => setTimeout(resolve, ms));
const fixture = script => c.evaluate('(()=>{const q=window.__XY_QA__;' + script + '})()');
const countCalls = command => c.evaluate('window.__XY_QA__.calls.filter(c=>c.command===' + JSON.stringify(command) + ').length');
const noAutomaticRequests = async () => {
  for (const command of ['set_current_provider', 'test_provider_health', 'test_all_provider_health', 'discover_provider_models', 'probe_provider_response_model', 'confirm_model_verification_cost']) {
    assert.equal(await countCalls(command), 0, 'viewing must not invoke ' + command);
  }
};
const networkAllowed = value => {
  const url = new URL(value);
  return ['data:', 'blob:', 'about:'].includes(url.protocol) || url.origin === origin ||
    (url.protocol === 'ws:' && new URL(base).protocol === 'http:' && url.host === new URL(base).host);
};
function localNetwork() {
  const external = [...c.networkRequests, ...c.webSocketUrls].filter(value => !networkAllowed(value));
  assert.deepEqual(external, [], 'renderer must not send requests outside the exact local origin');
}
async function noOverflow() {
  const metrics = await c.evaluate('({width:innerWidth,root:document.documentElement.scrollWidth,body:document.body.scrollWidth,main:document.querySelector(".xy-shell-main")?.scrollWidth,mainClient:document.querySelector(".xy-shell-main")?.clientWidth})');
  assert(metrics.root <= metrics.width && metrics.body <= metrics.width && metrics.main <= metrics.mainClient, JSON.stringify(metrics));
}
async function visibleNavigation(focused = false) {
  const opened = await c.evaluate('(()=>{const menu=document.querySelector(".xy-shell-menu");return Boolean(menu?.getClientRects().length)&&menu.getAttribute("aria-expanded")!=="true";})()');
  if (opened) await c.click('.xy-shell-menu');
  const metrics = await c.evaluate('(()=>{const nav=document.querySelector(".xy-ui-drawer .xy-shell-nav")??document.querySelector(".xy-shell-sidebar .xy-shell-nav"),item=' + (focused ? 'document.activeElement' : 'nav.querySelector("[aria-current=page],[aria-current=true]")') + ';if(!item||!nav.contains(item))throw new Error("Missing navigation target");const viewport=nav.closest(".xy-ui-drawer__body")??nav,bounds=viewport.getBoundingClientRect(),target=item.getBoundingClientRect(),style=getComputedStyle(item),ring=' + (focused ? 'parseFloat(style.outlineWidth)+Math.max(0,parseFloat(style.outlineOffset))' : '0') + ';return {viewport:innerWidth,language:document.documentElement.lang,theme:document.documentElement.dataset.theme,view:item.dataset.view??item.dataset.section,focused:' + JSON.stringify(focused) + ',outlineStyle:style.outlineStyle,ring,left:target.left-ring,right:target.right+ring,top:target.top-ring,bottom:target.bottom+ring,navLeft:bounds.left+viewport.clientLeft,navRight:bounds.left+viewport.clientLeft+viewport.clientWidth,navTop:bounds.top+viewport.clientTop,navBottom:bounds.top+viewport.clientTop+viewport.clientHeight};})()');
  navigationVisibility.push(metrics);
  assert(metrics.left >= metrics.navLeft - 1 && metrics.right <= metrics.navRight + 1 && metrics.top >= metrics.navTop - 1 && metrics.bottom <= metrics.navBottom + 1,
    'Navigation target and focus ring must stay inside its scroll viewport: ' + JSON.stringify(metrics));
  if (focused) assert(metrics.outlineStyle === 'solid' && metrics.ring >= 2, 'Keyboard focus must have a visible outline: ' + JSON.stringify(metrics));
  if (opened) await c.key('Escape');
}
async function fullWidthProviders() {
  const metrics = await c.evaluate('(()=>{const page=document.querySelector(".providers-page"),list=document.querySelector(".provider-workspace"),style=getComputedStyle(page),pageRect=page.getBoundingClientRect(),listRect=list.getBoundingClientRect();const left=pageRect.left+page.clientLeft+parseFloat(style.paddingLeft),width=page.clientWidth-parseFloat(style.paddingLeft)-parseFloat(style.paddingRight);return {viewport:innerWidth,contentLeft:left,contentRight:left+width,contentWidth:width,listLeft:listRect.left,listRight:listRect.right,listWidth:listRect.width};})()');
  layouts.push(metrics);
  assert(metrics.contentWidth > 0 && Math.abs(metrics.listLeft - metrics.contentLeft) <= 2 && Math.abs(metrics.listRight - metrics.contentRight) <= 2,
    'Providers list must fill the page content box, excluding intentional page padding: ' + JSON.stringify(metrics));
}
async function screenshot(name) {
  await pause(140); await c.screenshot(join(output, name + '.png')); screenshots.push(name + '.png');
}
async function openProviderEditor(mode) {
  await c.workspace('providers');
  if (mode === 'new') await c.click('[data-directory-action="add"]');
  else {
    await c.click('.provider-row[data-provider-id="qa-p2"] .provider-row__identity');
    await c.click('[data-edit-section="connection"]');
  }
  await c.waitFor('document.querySelector(".provider-editor-page")?.getAttribute("aria-busy")==="false"');
}
async function reset({ language = 'zh-CN', theme = 'light', width = 1440, height = 900, hash = '#providers' } = {}) {
  await c.viewport(width, height);
  await c.evaluate('localStorage.setItem("xy-qa:workspace-language",' + JSON.stringify(language) + ');localStorage.setItem("xy-switch:theme",' + JSON.stringify(theme) + ');');
  await c.navigate(base + '/' + hash);
  await c.waitFor('Boolean(window.__XY_QA__) && document.querySelector(".xy-shell-root") && !document.querySelector(".xy-shell-main .xy-ui-loading")');
  await c.waitFor('document.documentElement.lang===' + JSON.stringify(language));
  await c.waitFor('document.documentElement.dataset.theme===' + JSON.stringify(theme));
  assert.equal(await c.evaluate('window.__XY_QA__.providers.length'), 3, 'reset must restore the original fixture providers');
  assert.equal(await c.evaluate('window.__XY_QA__.current'), 'qa-p1', 'reset must restore the original current provider');
  await noAutomaticRequests();
}
async function run(name, work) {
  if (only && !only.includes(name)) return;
  console.log('RUN ' + name);
  const exceptionStart = c.exceptions.length;
  try {
    await work(); localNetwork(); assert.equal(c.exceptions.length, exceptionStart, 'unexpected renderer exception');
    reports.push({ name, passed: true }); console.log('PASS ' + name);
  } catch (error) {
    reports.push({ name, passed: false, error: String(error) });
    await screenshot(name + '-failure');
    await writeFile(join(output, name + '-failure.json'), JSON.stringify({ snapshot: await c.snapshot(), exceptions: c.exceptions }, null, 2));
    throw error;
  } finally { await writeFile(join(output, 'results.json'), JSON.stringify(reports, null, 2)); }
}
function contrast(foreground, background) {
  const luminance = color => {
    const values = color.match(/\d+(?:\.\d+)?/g).slice(0, 3).map(Number).map(value => {
      const channel = value / 255; return channel <= .04045 ? channel / 12.92 : ((channel + .055) / 1.055) ** 2.4;
    });
    return .2126 * values[0] + .7152 * values[1] + .0722 * values[2];
  };
  const a = luminance(foreground), b = luminance(background);
  return (Math.max(a, b) + .05) / (Math.min(a, b) + .05);
}
async function tokenContrast(theme) {
  const pairs = [['text', 'surface'], ['text-muted', 'surface'], ['text-faint', 'bg'], ['accent', 'accent-soft'],
    ['success', 'success-soft'], ['warning', 'warning-soft'], ['danger', 'danger-soft'], ['info', 'info-soft'],
    ['accent-contrast', 'accent'], ['nav-text', 'nav-bg'], ['nav-muted', 'nav-bg'], ['nav-accent', 'nav-active']];
  for (const [foreground, background] of pairs) {
    const colors = await c.evaluate('(()=>{const e=document.createElement("span");e.style.color="var(--xy-ds-' + foreground + ')";e.style.backgroundColor="var(--xy-ds-' + background + ')";document.body.append(e);const s=getComputedStyle(e),v=[s.color,s.backgroundColor];e.remove();return v;})()');
    const ratio = contrast(...colors); contrasts.push({ theme, foreground, background, ratio: Number(ratio.toFixed(3)) });
    assert(ratio >= 4.5, theme + ' ' + foreground + '/' + background + ' contrast ' + ratio);
  }
}
try {
  const source = '(()=>{const language=localStorage.getItem("xy-qa:workspace-language")||"zh-CN";localStorage.setItem("xy-qa:native-language",language);localStorage.setItem("xy-switch:language",language);})();\n' +
    await readFile(new URL('./workspace-qa/fixture.js', import.meta.url), 'utf8');
  ({ identifier: fixtureId } = await c.call('Page.addScriptToEvaluateOnNewDocument', { source }));
  await c.navigate(base + '/');
  await run('home-information-architecture', async () => {
    await reset({ hash: '' }); await c.waitFor('Boolean(document.querySelector(".overview-page"))');
    assert.equal(await c.evaluate('document.querySelectorAll(".xy-shell-activity").length'), 0);
    assert.equal(await c.evaluate('document.querySelectorAll(".xy-shell-section-nav .xy-shell-nav-item").length'), 0);
    assert.equal(await c.evaluate('document.querySelectorAll(".xy-shell-sidebar .xy-shell-nav-item").length'), 14);
    assert.equal(await c.evaluate('document.querySelector(".xy-shell-sidebar [aria-current=page]")?.dataset.view'), "overview");
    assert.equal(await c.evaluate('Boolean(document.querySelector(".xy-shell-subnav"))'), false);
    assert.equal(await c.evaluate('document.querySelector(".xy-shell-runtime").open'), false);
    await noAutomaticRequests(); await noOverflow(); await screenshot('overview-1440-zh-CN-light');
    await c.click('.xy-shell-runtime > summary');
    assert.equal(await c.evaluate('document.querySelector(".xy-shell-runtime").open'), true);
    await c.key('Escape');
    assert.equal(await c.evaluate('document.querySelector(".xy-shell-runtime").open'), false);
    assert.equal(await c.evaluate('document.activeElement.matches(".xy-shell-runtime > summary")'), true);
    await c.workspace('providers'); await c.waitFor('document.querySelectorAll(".provider-row").length===3');
    assert.equal(await c.evaluate('document.querySelector(".xy-shell-sidebar [aria-current=page]")?.dataset.view'), 'providers');
    assert.equal(await c.evaluate('document.querySelector(".xy-shell-sidebar [aria-current=page]")?.dataset.view'), 'providers');
    assert.equal(await c.evaluate('document.querySelectorAll(".xy-shell-sidebar .xy-shell-nav-item").length'), 14);
    assert.equal(await c.evaluate('Boolean(document.querySelector(".provider-inspector,[role=separator],[role=dialog]"))'), false);
    await fullWidthProviders();
    await noAutomaticRequests(); await noOverflow(); await screenshot('providers-1440-zh-CN-light');
  });
  await run('history-and-focus', async () => {
    await reset({ hash: '' }); const current = await c.evaluate('window.__XY_QA__.current');
    assert.equal(await c.evaluate('location.hash'), '');
    await c.button('管理供应商'); await c.waitFor('Boolean(document.querySelector(".providers-page"))');
    await c.button('查看 Relay 详情'); await c.waitFor('Boolean(document.querySelector(".provider-configuration-page"))');
    assert.equal(await c.evaluate('location.hash'), '#providers/qa-p2/configuration');
    assert.equal(await c.evaluate('document.activeElement.id'), 'workspace-content');
    assert.equal(await c.evaluate('window.__XY_QA__.current'), current); await noAutomaticRequests();
    await c.evaluate('history.back()'); await c.waitFor('location.hash==="#providers" && Boolean(document.querySelector(".providers-page"))');
    await c.evaluate('history.back()'); await c.waitFor('location.hash==="" && Boolean(document.querySelector(".overview-page"))');
    await c.evaluate('history.forward()'); await c.waitFor('Boolean(document.querySelector(".providers-page"))');
    await c.evaluate('history.forward()'); await c.waitFor('Boolean(document.querySelector(".provider-configuration-page"))');
    await c.evaluate('document.querySelector(".xy-shell-skip").focus()'); await c.key('Enter');
    assert.equal(await c.evaluate('location.hash'), '#providers/qa-p2/configuration', 'skip link must not overwrite the application route');
    assert.equal(await c.evaluate('document.activeElement.id'), 'workspace-content');
    await c.button('打开连接诊断'); await c.waitFor('Boolean(document.querySelector(".provider-diagnostics-page"))');
    assert.equal(await c.evaluate('document.querySelector("select[aria-label=诊断供应商]").value'), 'qa-p2');
    await noAutomaticRequests();
    await c.fill('select[aria-label="诊断供应商"]', 'qa-p3');
    assert.equal(await c.evaluate('location.hash'), '#diagnostics/qa-p3');
    assert.equal(await c.evaluate('window.__XY_QA__.current'), current);
    await c.evaluate('history.back()'); await c.waitFor('document.querySelector("select[aria-label=诊断供应商]")?.value==="qa-p2"');
    await noAutomaticRequests();
  });
  await run('configuration-management', async () => {
    await reset(); await fixture('q.providers[0].builtIn=true;q.providers[2].enabled=false;q.providers[1].advanced.headerOverrides={Authorization:"QA_SECRET_SENTINEL"};');
    await c.button('刷新'); await c.waitFor('!document.querySelector(".xy-ui-loading")');
    await c.button('查看 Orion 详情');
    assert.equal(await c.evaluate('[...document.querySelectorAll("button")].find(e=>e.textContent.trim()==="删除供应商").disabled'), true);
    await c.button('返回供应商'); await c.button('查看 Claude Lab 详情');
    assert.equal(await c.evaluate('[...document.querySelectorAll("button")].find(e=>e.textContent.trim()==="设为当前").disabled'), true);
    await noAutomaticRequests();
    await c.button('返回供应商'); await c.button('查看 Relay 详情');
    assert.equal(await c.evaluate('(()=>{const table=document.querySelector(".provider-model-document table");return Boolean(table?.getClientRects().length)&&!table.closest("details:not([open])");})()'), true);
    assert((await c.evaluate('document.body.innerText')).includes('qa-alpha')); assert((await c.evaluate('document.body.innerText')).includes('128000'));
    assert(!(await c.evaluate('document.body.innerText')).includes('QA_SECRET_SENTINEL'));
    await c.clickExpression('[...document.querySelectorAll("summary")].find(e=>e.textContent.trim()==="配置管理")');
    await c.button('复制（不含密钥）'); await c.waitFor('window.__XY_QA__.providers.length===4');
    assert.equal(await countCalls('set_provider_api_key'), 0); assert.equal(await c.evaluate('window.__XY_QA__.providers.at(-1).enabled'), false);
    assert.equal(await c.evaluate('window.__XY_QA__.current'), 'qa-p1');
    const savesBeforeEditing = await countCalls('save_provider');
    await c.button('编辑模型映射'); await c.waitFor('document.querySelector(".provider-editor-page")?.getAttribute("aria-busy")==="false"');
    assert.equal(await c.evaluate('document.activeElement.id'), 'provider-edit-models');
    await c.button('取消'); await c.waitFor('Boolean(document.querySelector(".provider-configuration-page"))');
    assert.equal(await c.evaluate('document.activeElement.textContent.trim()'), '编辑模型映射');
    assert.equal(await countCalls('save_provider'), savesBeforeEditing, 'cancelling an unchanged editor must not save');
    await c.clickExpression('[...document.querySelectorAll("summary")].find(e=>e.textContent.trim()==="配置管理")');
    await c.button('删除供应商'); assert.equal(await c.evaluate('document.activeElement.textContent.trim()'), '取消');
    await c.key('Escape'); assert.equal(await countCalls('delete_provider'), 0);
    await c.button('返回供应商'); await c.button('切换到 Relay'); await c.waitFor('window.__XY_QA__.current==="qa-p2"');
    assert.equal(await countCalls('set_current_provider'), 1, 'only an explicit switch activates the provider');
  });
  await run('provider-switch-runtime-read-recovery', async () => {
    for (const width of [1440, 320]) {
      await reset({ hash: '#overview', language: 'en', theme: 'dark', width, height: 800 });
      await c.waitFor('document.querySelector(".overview-switch-scope")?.dataset.scope==="configuration"');
      await fixture('q.fail.get_proxy_status="QA routing status read failed";');
      await c.button('Set current configuration: Relay');
      await c.waitFor('document.querySelector(".workspace-runtime-error")?.innerText.includes("QA routing status read failed")');
      assert.equal(await c.evaluate('document.querySelector(".overview-current-title h2").textContent'), 'Relay');
      assert.equal(await c.evaluate('document.querySelector(".xy-shell-status-connection strong").textContent'), 'Relay');
      assert((await c.evaluate('document.querySelector(".xy-shell-runtime summary").textContent')).includes('Status needs refresh'));
      assert.equal(await countCalls('set_current_provider'), 1);
      await c.click('.xy-shell-runtime > summary');
      assert.equal(await c.evaluate('document.querySelector(".xy-shell-runtime-panel button").disabled'), true);
      await noOverflow(); await screenshot('provider-switch-runtime-failure-' + width + '-en-dark');
      await c.key('Escape');
      await fixture('delete q.fail.get_proxy_status;');
      await c.button('Retry', 'document.querySelector(".workspace-runtime-error")');
      await c.waitFor('!document.querySelector(".workspace-runtime-error") && document.querySelector(".xy-shell-runtime summary").textContent.includes("Stopped")');
      assert.equal(await c.evaluate('document.querySelector(".xy-shell-status-connection strong").textContent'), 'Relay');
      assert.equal(await c.evaluate('document.querySelector(".overview-current-title h2").textContent'), 'Relay');
      assert.equal(await countCalls('set_current_provider'), 1, 'runtime retry must not repeat the successful configuration write');
      assert.equal(await countCalls('set_proxy_enabled'), 0);
      providerRecovery.push({ scenario: 'runtime-read', width, current: 'Relay', configurationWrites: 1, proxyWrites: 0, recovered: true });
      await noOverflow(); await screenshot('provider-switch-runtime-recovered-' + width + '-en-dark');
    }
  });
  await run('current-provider-read-fails-closed-and-recovers', async () => {
    for (const width of [1440, 320]) {
      await reset({ language: 'en', theme: 'dark', width, height: 800 });
      await fixture('q.fail.get_current_provider="QA current configuration read failed";');
      await c.button('Refresh', 'document.querySelector(".providers-page")');
      await c.waitFor('document.querySelector(".xy-shell-status-connection strong")?.textContent==="Status unknown"');
      await c.workspace('overview');
      await c.waitFor('document.querySelector(".overview-page")?.innerText.includes("Current configuration is unknown")');
      assert.equal(await c.evaluate('document.querySelectorAll(".overview-candidate button[aria-label]").length'), 3);
      assert.equal(await c.evaluate('[...document.querySelectorAll(".overview-candidate button[aria-label]")].every(button=>button.disabled)'), true);
      await noAutomaticRequests(); await noOverflow(); await screenshot('current-provider-read-failure-' + width + '-en-dark');
      await fixture('delete q.fail.get_current_provider;');
      await c.button('Retry', 'document.querySelector(".overview-page")');
      await c.waitFor('document.querySelector(".overview-current-title h2")?.textContent==="Orion" && document.querySelectorAll(".overview-candidate button[aria-label]:not(:disabled)").length===2');
      assert.equal(await c.evaluate('document.querySelector(".xy-shell-status-connection strong").textContent'), 'Orion');
      await noAutomaticRequests(); await noOverflow();
      providerRecovery.push({ scenario: 'current-read', width, current: 'Orion', configurationWrites: 0, recovered: true });
    }
  });
  await run('provider-directory-read-fails-closed-and-recovers', async () => {
    for (const width of [1440, 320]) {
      await reset({ language: 'en', theme: 'dark', width, height: 800 });
      await fixture('q.fail.get_current_provider="QA current configuration read failed";');
      await c.button('Refresh', 'document.querySelector(".providers-page")');
      await c.waitFor('document.querySelector(".xy-shell-status-connection strong")?.textContent==="Status unknown"');
      assert.equal(await c.evaluate('document.querySelectorAll(".provider-row [data-row-action=activate]").length'), 3);
      assert.equal(await c.evaluate('[...document.querySelectorAll(".provider-row [data-row-action=activate]")].every(button=>button.disabled)'), true);
      assert.equal(await c.evaluate('[...document.querySelectorAll(".provider-row [data-row-action=inspect]")].every(button=>!button.disabled)'), true);
      await c.evaluate('document.querySelectorAll(".provider-row [data-row-action=activate]").forEach(button=>button.click())');
      await noAutomaticRequests(); await noOverflow(); await screenshot('provider-directory-read-failure-' + width + '-en-dark');
      await c.click('.provider-row[data-provider-id="qa-p2"] .provider-row__identity');
      await c.waitFor('Boolean(document.querySelector(".provider-configuration-page"))');
      await c.button('Back to providers');
      assert.equal(await c.evaluate('[...document.querySelectorAll(".provider-row [data-row-action=activate]")].every(button=>button.disabled)'), true);
      await fixture('delete q.fail.get_current_provider;q.hold.push("get_current_provider");');
      await c.button('Retry', 'document.querySelector(".providers-page")');
      await c.waitFor('window.__XY_QA__.pending.some(item=>item.command==="get_current_provider")');
      assert.equal(await c.evaluate('document.querySelectorAll(".provider-row").length'), 0);
      await noAutomaticRequests(); await noOverflow(); await screenshot('provider-directory-read-pending-' + width + '-en-dark');
      await c.workspace('overview');
      await c.waitFor('Boolean(document.querySelector(".overview-page"))');
      assert.equal(await c.evaluate('document.querySelectorAll(".overview-candidate button[aria-label]").length'), 3);
      assert.equal(await c.evaluate('[...document.querySelectorAll(".overview-candidate button[aria-label]")].every(button=>button.disabled)'), true);
      await fixture('q.release("get_current_provider");');
      await c.waitFor('document.querySelector(".overview-current-title h2")?.textContent==="Orion"');
      await c.workspace('providers');
      assert.equal(await c.evaluate('document.querySelectorAll(".provider-row [data-row-action=activate]:not(:disabled)").length'), 2);
      assert.equal(await c.evaluate('document.querySelector(".xy-shell-status-connection strong").textContent'), 'Orion');
      await noAutomaticRequests();
      await c.button('Switch to Relay');
      await c.waitFor('window.__XY_QA__.current==="qa-p2" && document.querySelector(".xy-shell-status-connection strong")?.textContent==="Relay"');
      assert.equal(await countCalls('set_current_provider'), 1, 'only an explicit switch after successful refresh may change the configuration');
      assert.equal(await countCalls('save_provider'), 0);
      await noOverflow(); await screenshot('provider-directory-read-recovered-' + width + '-en-dark');
      providerRecovery.push({ scenario: 'directory-current-read', width, failureDisabled: 3, inspectionAvailable: true, failureWrites: 0,
        pendingListHidden: true, pendingOverviewDisabled: true, recoveredEnabled: 2, restoredCurrent: 'Orion', current: 'Relay', configurationWrites: 1, recovered: true });
    }
  });
  await run('takeover-scope-fails-closed-while-refreshing', async () => {
    await reset({ hash: '#overview', language: 'en', theme: 'dark', width: 320, height: 800 });
    await c.waitFor('document.querySelector(".overview-switch-scope")?.dataset.scope==="configuration"');
    assert((await c.evaluate('document.querySelector(".overview-switch-scope").innerText')).includes('no client configuration is taken over'));
    await c.evaluate('Promise.all(["claude","codex"].map(app=>window.__TAURI_INTERNALS__.invoke("set_app_takeover_enabled",{app,enabled:true})))');
    await c.button('Refresh takeover status');
    await c.waitFor('document.querySelector(".overview-switch-scope")?.dataset.scope==="takeover"');
    const affected = await c.evaluate('document.querySelector(".overview-switch-scope").innerText');
    assert(affected.includes('Claude Code') && affected.includes('Codex'));
    await fixture('q.fail.list_app_takeovers="QA takeover scope unavailable";');
    await c.button('Refresh takeover status');
    await c.waitFor('document.querySelector(".overview-switch-scope")?.dataset.scope==="unknown"');
    assert.equal(await c.evaluate('[...document.querySelectorAll(".overview-candidate button[aria-label]")].every(button=>button.disabled)'), true);
    await screenshot('takeover-scope-read-failure-320-en-dark');
    await fixture('delete q.fail.list_app_takeovers;q.hold.push("list_app_takeovers");');
    await c.button('Refresh takeover status');
    await c.waitFor('window.__XY_QA__.pending.some(item=>item.command==="list_app_takeovers")');
    assert.equal(await c.evaluate('[...document.querySelectorAll(".overview-candidate button[aria-label]")].every(button=>button.disabled)'), true);
    await noAutomaticRequests(); await noOverflow(); await screenshot('takeover-scope-read-pending-320-en-dark');
    await fixture('q.release("list_app_takeovers");');
    await c.waitFor('document.querySelector(".overview-switch-scope")?.dataset.scope==="takeover" && document.querySelectorAll(".overview-candidate button[aria-label]:not(:disabled)").length===2');
    await noAutomaticRequests(); await noOverflow();
    providerRecovery.push({ scenario: 'takeover-scope', width: 320, clients: ['Claude Code', 'Codex'], configurationWrites: 0, failureLocked: true, pendingLocked: true, recovered: true });
  });
  await run('diagnostics-tabs-and-default-off', async () => {
    await reset({ hash: '#diagnostics/qa-p2' });
    assert.equal(await c.evaluate('Boolean(document.querySelector(".provider-response-probe"))'), false, 'model module is not mounted on the connection tab');
    const tabTargets = await c.evaluate('[...document.querySelectorAll("[role=tab]")].map(e=>({selected:e.getAttribute("aria-selected"),index:e.tabIndex,target:document.getElementById(e.getAttribute("aria-controls"))?.getAttribute("role")}))');
    assert.deepEqual(tabTargets, [{ selected: 'true', index: 0, target: 'tabpanel' }, { selected: 'false', index: -1, target: 'tabpanel' }]);
    await c.click('[role="tab"][aria-selected="true"]'); await c.key('End');
    await c.waitFor('Boolean(document.querySelector(".provider-response-probe"))'); await c.waitFor('window.__XY_QA__.calls.some(c=>c.command==="model_verification_status")');
    assert.equal(await c.evaluate('document.activeElement.getAttribute("aria-selected")'), 'true');
    assert.equal(await c.evaluate('document.querySelector(".provider-response-probe [role=switch]").getAttribute("aria-checked")'), 'false');
    assert.equal(await c.evaluate('window.__XY_QA__.verification.settings.activeProbeEnabled'), false);
    await noAutomaticRequests(); await screenshot('diagnostics-model-default-off');
    await c.key('ArrowRight'); await c.waitFor('!document.querySelector(".provider-response-probe")');
    assert.equal(await c.evaluate('document.activeElement.textContent.trim()'), '连接测试');
    await c.key('ArrowLeft'); await c.waitFor('Boolean(document.querySelector(".provider-response-probe"))');
    await c.key('Home'); await c.waitFor('!document.querySelector(".provider-response-probe")');
    assert.equal(await c.evaluate('getComputedStyle(document.activeElement).outlineStyle'), 'solid');
    await noAutomaticRequests();
  });
  await run('diagnostics-health-and-explicit-test', async () => {
    await reset(); await fixture('q.healthOverrides["qa-p2"]={status:"unknown",successRate:0,latencyMs:null,lastCheckedAt:0};q.providers[2].enabled=false;');
    await c.button('刷新'); await c.waitFor('!document.querySelector(".xy-ui-loading")');
    await c.workspace('diagnostics'); await c.fill('select[aria-label="诊断供应商"]', 'qa-p2');
    const summary = await c.evaluate('document.querySelector(".provider-health-summary").innerText');
    assert(summary.includes('状态未知') && summary.includes('无记录')); assert(!summary.includes('测试通过') && !summary.includes('0.0%'));
    await c.fill('select[aria-label="诊断供应商"]', 'qa-p3');
    assert.equal(await c.evaluate('[...document.querySelectorAll("button")].find(e=>e.textContent.trim()==="测试连接").disabled'), true);
    await c.fill('select[aria-label="诊断供应商"]', 'qa-p2'); await noAutomaticRequests();
    await fixture('delete q.healthOverrides["qa-p2"];q.hold.push("test_provider_health");');
    await c.button('测试连接'); await c.waitFor('window.__XY_QA__.pending.some(e=>e.command==="test_provider_health")');
    assert.equal(await c.evaluate('document.querySelector("select[aria-label=诊断供应商]").disabled'), true);
    assert.equal(await c.evaluate('[...document.querySelectorAll("button")].find(e=>e.textContent.trim()==="测试全部").disabled'), true);
    await fixture('q.release("test_provider_health");'); await c.waitFor('document.querySelector(".provider-health-summary").innerText.includes("98.5%")');
    assert.equal(await countCalls('test_provider_health'), 1); assert.equal(await countCalls('test_all_provider_health'), 0);
    assert.equal(await c.evaluate('window.__XY_QA__.calls.find(c=>c.command==="test_provider_health").args.providerId'), 'qa-p2');
    assert.equal(await c.evaluate('window.__XY_QA__.current'), 'qa-p1');
    await fixture('q.fail.test_provider_health="QA diagnostic request denied";'); await c.button('测试连接');
    await c.waitFor('document.body.innerText.includes("QA diagnostic request denied")');
    await screenshot('diagnostics-connection-error');
  });
  await run('keyboard-sort-without-activation', async () => {
    await reset(); await c.evaluate('document.querySelector(' + JSON.stringify('[aria-label="拖动排序 Orion"]') + ').focus()');
    await c.key('Enter'); await c.waitFor('document.querySelector(".provider-row[data-dragging=true]")');
    await c.key('ArrowDown'); await c.key('Enter'); await c.waitFor('window.__XY_QA__.calls.some(c=>c.command==="reorder_providers")');
    assert.deepEqual(await c.evaluate('window.__XY_QA__.providers.map(p=>p.id)'), ['qa-p2', 'qa-p1', 'qa-p3']);
    assert.equal(await c.evaluate('window.__XY_QA__.current'), 'qa-p1'); assert.equal(await countCalls('set_current_provider'), 0);
    assert.equal(await c.evaluate('document.activeElement.getAttribute("aria-label")'), '拖动排序 Orion');
  });
  await run('responsive-languages-themes-and-tokens', async () => {
    for (const [width, height] of [[1440, 900], [820, 700], [390, 720]]) {
      for (const language of ['zh-CN', 'en']) for (const theme of ['light', 'dark']) {
        for (const [page, hash] of [['overview', ''], ['providers', '#providers'], ['configuration', '#providers/qa-p2/configuration'], ['diagnostics', '#diagnostics/qa-p2']]) {
          await reset({ width, height, language, theme, hash }); await noOverflow(); await noAutomaticRequests(); await visibleNavigation();
          if (page === 'providers') await fullWidthProviders();
          assert.equal(await c.evaluate('Boolean(document.querySelector(".provider-inspector,[role=separator],[role=dialog]"))'), false);
          if (language === 'en') {
            assert.equal(await c.evaluate('[...document.querySelectorAll(".xy-shell-nav-item")].some(e=>/[\u3400-\u9fff]/u.test(e.getAttribute("title")))'), false);
            assert.equal(await c.evaluate('/[\u3400-\u9fff]/u.test(document.querySelector(".xy-shell-main").innerText)'), false);
          }
          await screenshot(page + '-' + width + '-' + language + '-' + theme);
        }
        if (width === 1440 && language === 'en') await tokenContrast(theme);
      }
    }
    await c.call('Emulation.setEmulatedMedia', { features: [{ name: 'prefers-reduced-motion', value: 'reduce' }] });
    assert.equal(await c.evaluate('getComputedStyle(document.querySelector(".xy-shell-nav-item")).transitionDuration'), '0s');
    await c.call('Emulation.setEmulatedMedia', { features: [] });
  });
  await run('compact-navigation-focus', async () => {
    const views = ['overview', 'providers', 'clients', 'accounts', 'diagnostics', 'plugins', 'marketplace', 'mcp', 'prompts', 'skills', 'sessions', 'usage', 'settings', 'governance'];
    for (const language of ['zh-CN', 'en']) for (const theme of ['light', 'dark']) {
      await reset({ width: 390, height: 720, language, theme, hash: '#diagnostics/qa-p2' });
      await visibleNavigation();
      await c.click('.xy-shell-menu');
      assert.equal(await c.evaluate('document.querySelectorAll(".xy-ui-drawer .xy-shell-nav-item").length'), 14);
      await c.evaluate('document.querySelector(".xy-ui-drawer__header button").focus()');
      for (const view of views) {
        await c.key('Tab');
        assert.equal(await c.evaluate('document.activeElement.dataset.view'), view, 'Tab follows the complete navigation order');
        await visibleNavigation(true);
        if (view === 'usage') await screenshot('navigation-focus-390-' + language + '-' + theme);
      }
      for (const view of views.slice(0, -1).reverse()) {
        await c.key('Tab', 8);
        assert.equal(await c.evaluate('document.activeElement.dataset.view'), view, 'Shift+Tab reverses the complete navigation order');
        await visibleNavigation(true);
      }
      await c.key('Escape');
      assert.equal(await c.evaluate('Boolean(document.querySelector("[role=dialog]"))'), false);
      assert.equal(await c.evaluate('document.activeElement.matches(".xy-shell-menu")'), true);
      await c.viewport(820, 700); await visibleNavigation();
      await c.viewport(740, 700); await c.click('.xy-shell-menu'); await c.viewport(1440, 900);
      await c.waitFor('!document.querySelector("[role=dialog]")'); await visibleNavigation();
      await c.viewport(320, 720); await visibleNavigation();
      await c.workspace('settings');
      assert.equal(await c.evaluate('Boolean(document.querySelector("[role=dialog]"))'), false);
      await noAutomaticRequests(); await noOverflow();
    }
  });
  await run('provider-editor-unsaved-global-navigation', async () => {
    for (const width of [1440, 320]) for (const mode of ['new', 'edit']) for (const destination of mode === 'edit' ? ['overview', 'providers'] : ['overview']) {
      const language = width === 320 ? 'en' : 'zh-CN', theme = width === 320 ? 'dark' : 'light';
      await reset({ width, height: 800, language, theme, hash: '#overview' });
      await openProviderEditor(mode);
      const field = '.provider-editor-page input', keyField = '.provider-editor-page input[type=password]';
      const draft = 'Unsaved ' + mode + ' provider', unsavedKey = 'sk-qa-draft-not-to-be-saved';
      await c.fill(field, draft); await c.fill(keyField, unsavedKey);
      const originalHash = await c.evaluate('location.hash');
      const overflow = await c.evaluate('document.body.style.overflow');
      if (width === 320) await c.click('.xy-shell-menu');
      const target = (width === 320 ? '.xy-ui-drawer' : '.xy-shell-sidebar') + ' [data-view="' + destination + '"]';
      await c.click(target); await c.waitFor('Boolean(document.querySelector(".task-leave-dialog"))');
      assert.equal(await c.evaluate('location.hash'), originalHash);
      const dialog = await c.evaluate('(()=>{const modal=document.querySelector(".task-leave-dialog"),drawer=document.querySelector(".xy-ui-drawer-shell"),button=modal.querySelector(".xy-ui-modal__footer button:last-child"),rect=button.getBoundingClientRect();return {focused:modal.contains(document.activeElement),hit:button.contains(document.elementFromPoint(rect.x+rect.width/2,rect.y+rect.height/2)),aboveDrawer:!drawer||Number(getComputedStyle(modal.parentElement).zIndex)>Number(getComputedStyle(drawer).zIndex)};})()');
      assert(dialog.focused && dialog.hit && dialog.aboveDrawer, JSON.stringify(dialog));
      await screenshot('provider-' + mode + '-leave-' + destination + '-' + width + '-' + language + '-' + theme);
      await c.key('Escape');
      assert.equal(await c.evaluate('document.activeElement.matches(' + JSON.stringify(target) + ')'), true);
      assert.equal(await c.evaluate('document.querySelector(' + JSON.stringify(field) + ').value'), draft);
      assert.equal(await c.evaluate('document.querySelector(' + JSON.stringify(keyField) + ').value'), unsavedKey);
      await c.click(target); await c.click('.task-leave-dialog .xy-ui-modal__footer button:first-child');
      assert.equal(await c.evaluate('document.querySelector(' + JSON.stringify(field) + ').value'), draft);
      await c.click(target); await c.click('.task-leave-dialog .xy-ui-modal__footer button:last-child');
      await c.waitFor('document.querySelector(".xy-shell-root")?.dataset.workspaceView===' + JSON.stringify(destination));
      assert.equal(await c.evaluate('Boolean(document.querySelector(".provider-editor-page"))'), false);
      assert.equal(await c.evaluate('document.querySelectorAll("[role=dialog]").length'), 0);
      assert.equal(await c.evaluate('document.body.style.overflow'), overflow);
      await openProviderEditor(mode);
      assert.equal(await c.evaluate('document.querySelector(' + JSON.stringify(field) + ').value'), mode === 'new' ? '' : 'Relay');
      assert.equal(await c.evaluate('document.querySelector(' + JSON.stringify(keyField) + ').value'), '');
      await c.workspace('overview');
      for (const command of ['save_provider', 'set_provider_api_key', 'delete_provider_api_key']) assert.equal(await countCalls(command), 0);
      await noAutomaticRequests(); await noOverflow();
      providerNavigation.push({ scenario: 'global', mode, width, destination, cancelledDraftRetained: true, discardedKeyCleared: true, configurationWrites: 0, credentialWrites: 0, dialog });
    }
  });
  await run('provider-editor-unsaved-history-navigation', async () => {
    for (const mode of ['new', 'edit']) {
      await reset({ hash: '#overview' }); await openProviderEditor(mode);
      const originalHash = await c.evaluate('location.hash'), draft = 'History draft ' + mode;
      await c.fill('.provider-editor-page input', draft);
      await c.evaluate('history.back()'); await c.waitFor('Boolean(document.querySelector(".task-leave-dialog"))');
      assert.equal(await c.evaluate('location.hash'), originalHash);
      await c.key('Escape');
      assert.equal(await c.evaluate('document.querySelector(".provider-editor-page input").value'), draft);
      await c.evaluate('history.back()'); await c.waitFor('Boolean(document.querySelector(".task-leave-dialog"))');
      await c.click('.task-leave-dialog .xy-ui-modal__footer button:last-child');
      const destination = mode === 'edit' ? 'providers' : 'overview';
      await c.waitFor('document.querySelector(".xy-shell-root")?.dataset.workspaceView===' + JSON.stringify(destination));
      assert.equal(await c.evaluate('Boolean(document.querySelector(".provider-editor-page"))'), false);
      await c.evaluate('history.forward()');
      await c.waitFor('location.hash===' + JSON.stringify(originalHash));
      assert.equal(await c.evaluate('Boolean(document.querySelector(".provider-editor-page"))'), false);
      assert.equal(await countCalls('save_provider'), 0); await noAutomaticRequests();
      providerNavigation.push({ scenario: 'history', mode, cancelledDraftRetained: true, confirmedBackAndForward: true, configurationWrites: 0 });
    }
  });
  await run('provider-editor-blocks-navigation-while-saving', async () => {
    await reset({ hash: '#overview' }); await openProviderEditor('edit');
    await c.fill('.provider-editor-page input', 'Relay saved once');
    await fixture('q.hold.push("save_provider");');
    await c.click('.provider-editor-page button[type=submit]');
    await c.waitFor('window.__XY_QA__.pending.some(item=>item.command==="save_provider")');
    await c.click('.xy-shell-sidebar [data-view="overview"]');
    await c.waitFor('Boolean(document.querySelector(".task-navigation-wait"))');
    assert.equal(await c.evaluate('location.hash'), '#providers/qa-p2/configuration');
    await c.evaluate('history.back()'); await pause(120);
    assert.equal(await c.evaluate('location.hash'), '#providers/qa-p2/configuration');
    assert.equal(await c.evaluate('document.querySelector(".provider-editor-page").getAttribute("aria-busy")'), 'true');
    assert.equal(await c.evaluate('Boolean(document.querySelector(".task-leave-dialog"))'), false);
    assert.equal(await countCalls('save_provider'), 1);
    await screenshot('provider-edit-saving-navigation-blocked-1440-zh-CN-light');
    await fixture('q.release("save_provider");');
    await c.waitFor('Boolean(document.querySelector(".provider-configuration-page"))');
    assert.equal(await c.evaluate('window.__XY_QA__.providers.find(provider=>provider.id==="qa-p2").name'), 'Relay saved once');
    await c.workspace('overview');
    assert.equal(await countCalls('save_provider'), 1);
    assert.equal(await countCalls('set_provider_api_key'), 0); await noAutomaticRequests();
    providerNavigation.push({ scenario: 'saving', globalAndHistoryBlocked: true, recovered: true, configurationWrites: 1, credentialWrites: 0 });
  });
  await run('unsaved-task-navigation-dialogs', async () => {
    const governance = await readFile(new URL('./workspace-qa/governance-fixture.js', import.meta.url), 'utf8');
    for (const language of ['zh-CN', 'en']) for (const theme of ['light', 'dark']) for (const task of ['privacy', 'access']) {
      await reset({ width: 320, height: 760, language, theme, hash: '#settings' });
      await c.evaluate(governance);
      await c.evaluate('(()=>{const original=window.__TAURI_INTERNALS__.invoke;window.__TAURI_INTERNALS__.invoke=async(command,args)=>{if(command==="get_privacy_settings"){window.__XY_QA__.calls.push({command,args});return {redactPersonalInformation:true,blockUninspectedAttachments:true};}return original(command,args);};})()');
      const destination = task === 'privacy' ? 'providers' : 'settings';
      let field, draft;
      if (task === 'privacy') {
        await c.click('#settings-task-privacy');
        await c.waitFor('Boolean(document.querySelector("[role=switch]:not(:disabled)"))');
        await c.click('.settings-page .workspace-disclosure > summary');
        field = '.settings-page textarea'; draft = 'Synthetic unsaved privacy text';
      } else {
        await c.workspace('governance'); await c.click('#gov-task-access');
        await c.waitFor('Boolean(document.querySelector(".gov-form-rows input[type=number]"))');
        field = '.gov-form-rows input[type=number]'; draft = '121';
      }
      await c.fill(field, draft);
      const overflow = await c.evaluate('document.body.style.overflow');
      await c.click('.xy-shell-menu');
      const target = '.xy-ui-drawer [data-view="' + destination + '"]';
      await c.click(target);
      await c.waitFor('Boolean(document.querySelector(".task-leave-dialog"))');
      const layers = await c.evaluate('(()=>{const dialog=document.querySelector(".task-leave-dialog"),drawer=document.querySelector(".xy-ui-drawer-shell"),button=dialog.querySelector(".xy-ui-modal__footer button:last-child"),rect=button.getBoundingClientRect();return {dialog:Number(getComputedStyle(dialog.parentElement).zIndex),drawer:Number(getComputedStyle(drawer).zIndex),hit:button.contains(document.elementFromPoint(rect.x+rect.width/2,rect.y+rect.height/2)),focused:dialog.contains(document.activeElement)};})()');
      assert(layers.dialog > layers.drawer && layers.hit && layers.focused, JSON.stringify(layers));
      await screenshot('unsaved-' + task + '-navigation-320-' + language + '-' + theme);
      await c.key('Tab', 8);
      assert.equal(await c.evaluate('document.activeElement===document.querySelector(".task-leave-dialog .xy-ui-modal__footer button:last-child")'), true);
      await c.key('Tab');
      assert.equal(await c.evaluate('document.activeElement===document.querySelector(".task-leave-dialog .xy-ui-modal__header button")'), true);
      await c.key('Escape');
      assert.equal(await c.evaluate('Boolean(document.querySelector(".task-leave-dialog"))'), false);
      assert.equal(await c.evaluate('document.activeElement.matches(' + JSON.stringify(target) + ')'), true);
      assert.equal(await c.evaluate('document.querySelector(' + JSON.stringify(field) + ').value'), draft);
      await c.click(target);
      await c.click('.task-leave-dialog .xy-ui-modal__footer button:first-child');
      assert.equal(await c.evaluate('document.querySelector(' + JSON.stringify(field) + ').value'), draft);
      assert.equal(await c.evaluate('Boolean(document.querySelector(".xy-ui-drawer"))'), true);
      await c.click(target);
      await c.click('.task-leave-dialog .xy-ui-modal__footer button:last-child');
      await c.waitFor('document.querySelector(".xy-shell-root")?.dataset.workspaceView===' + JSON.stringify(destination));
      assert.equal(await c.evaluate('document.querySelectorAll("[role=dialog]").length'), 0);
      assert.equal(await c.evaluate('document.body.style.overflow'), overflow);
      assert.equal(await countCalls(task === 'privacy' ? 'save_privacy_settings' : 'save_access_policy'), 0);
      await c.click('.xy-shell-menu');
      assert.equal(await c.evaluate('getComputedStyle(document.querySelector(".xy-ui-drawer-shell")).zIndex'), '100');
      await c.key('Escape'); await noOverflow(); await noAutomaticRequests();
    }
  });
  localNetwork();
  await writeFile(join(output, 'acceptance.json'), JSON.stringify({ passed: true, scope: 'Isolated renderer with synthetic IPC; not packaged native acceptance', reports, screenshots,
    contrasts, layouts, navigationVisibility, providerRecovery, providerNavigation, networkOrigins: [...new Set(c.networkRequests.map(value => new URL(value).origin))], exceptionCount: c.exceptions.length,
    packagedApplicationExecuted: false, realProviderRequests: false }, null, 2));
  console.log(JSON.stringify({ passed: reports.length, output }));
} finally {
  if (fixtureId) await c.call('Page.removeScriptToEvaluateOnNewDocument', { identifier: fixtureId }).catch(() => {});
  await c.close();
}
