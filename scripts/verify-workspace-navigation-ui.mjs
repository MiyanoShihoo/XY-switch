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
const reports = [], screenshots = [], contrasts = [], layouts = [];
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
async function fullWidthProviders() {
  const metrics = await c.evaluate('(()=>{const page=document.querySelector(".providers-page"),list=document.querySelector(".provider-workspace"),style=getComputedStyle(page),pageRect=page.getBoundingClientRect(),listRect=list.getBoundingClientRect();const left=pageRect.left+page.clientLeft+parseFloat(style.paddingLeft),width=page.clientWidth-parseFloat(style.paddingLeft)-parseFloat(style.paddingRight);return {viewport:innerWidth,contentLeft:left,contentRight:left+width,contentWidth:width,listLeft:listRect.left,listRight:listRect.right,listWidth:listRect.width};})()');
  layouts.push(metrics);
  assert(metrics.contentWidth > 0 && Math.abs(metrics.listLeft - metrics.contentLeft) <= 2 && Math.abs(metrics.listRight - metrics.contentRight) <= 2,
    'Providers list must fill the page content box, excluding intentional page padding: ' + JSON.stringify(metrics));
}
async function screenshot(name) {
  await pause(140); await c.screenshot(join(output, name + '.png')); screenshots.push(name + '.png');
}
async function reset({ language = 'zh-CN', theme = 'light', width = 1440, height = 900, hash = '' } = {}) {
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
    await reset(); await c.waitFor('document.querySelectorAll(".provider-row").length===3');
    assert.equal(await c.evaluate('document.querySelectorAll(".xy-shell-nav-item").length'), 9);
    assert.equal(await c.evaluate('document.querySelectorAll(".xy-shell-nav-group").length'), 3);
    assert.equal(await c.evaluate('Boolean(document.querySelector(".provider-inspector,[role=separator],[role=dialog]"))'), false);
    await fullWidthProviders();
    await noAutomaticRequests(); await noOverflow(); await screenshot('providers-1440-zh-CN-light');
  });
  await run('history-and-focus', async () => {
    await reset(); const current = await c.evaluate('window.__XY_QA__.current');
    assert.equal(await c.evaluate('location.hash'), '');
    await c.button('查看 Relay 详情'); await c.waitFor('Boolean(document.querySelector(".provider-configuration-page"))');
    assert.equal(await c.evaluate('location.hash'), '#providers/qa-p2/configuration');
    assert.equal(await c.evaluate('document.activeElement.id'), 'workspace-content');
    assert.equal(await c.evaluate('window.__XY_QA__.current'), current); await noAutomaticRequests();
    await c.evaluate('history.back()'); await c.waitFor('location.hash==="" && Boolean(document.querySelector(".providers-page"))');
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
    assert((await c.evaluate('document.body.innerText')).includes('qa-alpha')); assert((await c.evaluate('document.body.innerText')).includes('128000'));
    assert(!(await c.evaluate('document.body.innerText')).includes('QA_SECRET_SENTINEL'));
    await c.button('复制（不含密钥）'); await c.waitFor('window.__XY_QA__.providers.length===4');
    assert.equal(await countCalls('set_provider_api_key'), 0); assert.equal(await c.evaluate('window.__XY_QA__.providers.at(-1).enabled'), false);
    assert.equal(await c.evaluate('window.__XY_QA__.current'), 'qa-p1');
    await c.button('编辑模型映射'); await c.waitFor('Boolean(document.querySelector("[role=dialog]"))');
    assert.equal(await c.evaluate('document.querySelector("[role=dialog]").contains(document.activeElement)'), true);
    await c.key('Escape'); assert.equal(await c.evaluate('document.activeElement.textContent.trim()'), '编辑模型映射');
    await c.button('删除供应商'); assert.equal(await c.evaluate('document.activeElement.textContent.trim()'), '取消');
    await c.key('Escape'); assert.equal(await countCalls('delete_provider'), 0);
    await c.button('返回供应商'); await c.button('切换到 Relay'); await c.waitFor('window.__XY_QA__.current==="qa-p2"');
    assert.equal(await countCalls('set_current_provider'), 1, 'only an explicit switch activates the provider');
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
    await c.click('.xy-shell-nav-item[data-view="diagnostics"]'); await c.fill('select[aria-label="诊断供应商"]', 'qa-p2');
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
    for (const [width, height] of [[1440, 900], [820, 700], [480, 720]]) {
      for (const language of ['zh-CN', 'en']) for (const theme of ['light', 'dark']) {
        for (const [page, hash] of [['providers', ''], ['configuration', '#providers/qa-p2/configuration'], ['diagnostics', '#diagnostics/qa-p2']]) {
          await reset({ width, height, language, theme, hash }); await noOverflow(); await noAutomaticRequests();
          if (page === 'providers') await fullWidthProviders();
          assert.equal(await c.evaluate('Boolean(document.querySelector(".provider-inspector,[role=separator],[role=dialog]"))'), false);
          if (language === 'en') {
            assert.equal(await c.evaluate('[...document.querySelectorAll(".xy-shell-nav-item")].some(e=>/[\u3400-\u9fff]/u.test(e.getAttribute("aria-label")))'), false);
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
  localNetwork();
  await writeFile(join(output, 'acceptance.json'), JSON.stringify({ passed: true, scope: 'Isolated renderer with synthetic IPC; not packaged native acceptance', reports, screenshots,
    contrasts, layouts, networkOrigins: [...new Set(c.networkRequests.map(value => new URL(value).origin))], exceptionCount: c.exceptions.length,
    packagedApplicationExecuted: false, realProviderRequests: false }, null, 2));
  console.log(JSON.stringify({ passed: reports.length, output }));
} finally {
  if (fixtureId) await c.call('Page.removeScriptToEvaluateOnNewDocument', { identifier: fixtureId }).catch(() => {});
  await c.close();
}
