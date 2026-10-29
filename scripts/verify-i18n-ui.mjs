import assert from 'node:assert/strict';
import { mkdir, readFile, writeFile } from 'node:fs/promises';
import { resolve, join } from 'node:path';
import { connect } from './workspace-qa/cdp.mjs';

// Local synthetic IPC only. Never reads real credentials, config or conversation files.
const base = process.env.APP_URL ?? 'http://127.0.0.1:1431';
const output = resolve(process.env.I18N_QA_OUTPUT ?? 'artifacts/i18n-ui-qa');
await mkdir(output, { recursive: true });
const c = await connect(undefined, { fresh: true });
const fixtureSource = await readFile(new URL('./workspace-qa/fixture.js', import.meta.url), 'utf8');
let fixtureId;
const reports = [];
const snapshots = [];
const pause = ms => new Promise(r => setTimeout(r, ms));
const languageModule = '/src/i18n/language.ts';
// Vite HMR adds a timestamp query. Reuse the app's existing module, not a second state store.
const languageImport = `import(performance.getEntriesByType('resource').find(e=>new URL(e.name).pathname===${JSON.stringify(languageModule)})?.name ?? ${JSON.stringify(languageModule)})`;
const tr = source => c.evaluate(`${languageImport}.then(m=>m.t(${JSON.stringify(source)}))`);
async function switchTo(language) {
  await c.evaluate(`${languageImport}.then(m=>m.setLanguagePreference(${JSON.stringify(language)}))`);
  await c.waitFor(`document.documentElement.lang===${JSON.stringify(language)}`);
}
async function nav(source) {
  const label = await tr(source);
  await c.waitFor('Boolean(document.querySelector('+JSON.stringify('.xy-shell-nav-item[title='+JSON.stringify(label)+']')+'))');
  await c.click('.xy-shell-nav-item[title=' + JSON.stringify(label) + ']');
  await c.waitFor(`document.querySelector('.xy-shell-nav-item[aria-current=page]')?.title===${JSON.stringify(label)}`);
  await pause(100);
  await c.waitFor('!document.querySelector(".xy-shell-main .xy-ui-loading")');
}
async function field(source) {
  const label = await tr(source);
  const id = await c.evaluate(`[...document.querySelectorAll('label')].find(e=>e.textContent.replace(/[*]/g,'').trim()===${JSON.stringify(label)})?.htmlFor`);
  assert(id, 'Missing field ' + label);
  return '[id=' + JSON.stringify(id) + ']';
}
async function noOverflow() {
  const metrics = await c.evaluate('({width:innerWidth,root:document.documentElement.scrollWidth,body:document.body.scrollWidth})');
  assert(metrics.root <= metrics.width && metrics.body <= metrics.width, JSON.stringify(metrics));
}
async function noLeaks() {
  const snapshot = await c.snapshot();
  assert(!snapshot.text.includes('Unexpected QA IPC:'), 'Incomplete synthetic IPC: ' + snapshot.text);
  assert(!snapshot.text.includes('undefined'), 'Unexpected undefined UI text');
  const raw = await c.evaluate(`(() => {
    const opaque=[]; const walk=v=>{if(typeof v==='string')opaque.push(v);else if(Array.isArray(v))v.forEach(walk);else if(v&&typeof v==='object')Object.values(v).forEach(walk);};
    ['providers','prompts','mcp','skills','sessions'].forEach(k=>walk(window.__XY_QA__[k]));
    opaque.push('简体中文','请检查界面边界。','已保留全部审计证据。\\nconst result = true;');
    let text=document.body.innerText+'\\n'+[...document.querySelectorAll('[aria-label],[title],[placeholder]')].filter(e=>e.getClientRects().length).map(e=>['aria-label','title','placeholder'].map(a=>e.getAttribute(a)||'').join(' ')).join('\\n');
    [...new Set(opaque)].filter(Boolean).sort((a,b)=>b.length-a.length).forEach(value=>{text=text.split(value).join('');});
    return text.split('\\n').filter(line=>/[\\u3400-\\u9fff]/u.test(line));
  })()`);
  assert.deepEqual(raw, [], 'Untranslated application-owned Chinese: ' + JSON.stringify(raw));
  return snapshot;
}
async function run(name, work) {
  console.log('RUN ' + name);
  const start = c.exceptions.length;
  try {
    await work();
    assert.equal(c.exceptions.length, start, 'Unexpected runtime exception');
    reports.push({name, passed:true}); console.log('PASS ' + name);
  } catch(error) {
    reports.push({name, passed:false, error:String(error)});
    await c.screenshot(join(output, name + '-failure.png'));
    await writeFile(join(output, name + '-failure.json'), JSON.stringify({snapshot:await c.snapshot(), exceptions:c.exceptions},null,2));
    throw error;
  } finally {
    await writeFile(join(output,'results.json'), JSON.stringify({synthetic:true, reports, snapshots},null,2));
  }
}
try {
  ({identifier:fixtureId} = await c.call('Page.addScriptToEvaluateOnNewDocument', {source:'localStorage.setItem("xy-qa:native-language","zh-CN");localStorage.setItem("xy-switch:language","zh-CN");\n'+fixtureSource}));
  await c.viewport(1180,760);await c.navigate(base+'/');await c.waitFor('document.body.innerText.includes("Orion")');
  await c.call('Page.removeScriptToEvaluateOnNewDocument',{identifier:fixtureId});
  ({identifier:fixtureId} = await c.call('Page.addScriptToEvaluateOnNewDocument',{source:fixtureSource}));

  await run('native-language-control-and-persistence', async()=>{
    await nav('设置');await c.fill(await field('请求超时（秒）'),'120');
    await c.fill('select[aria-label="界面语言"]','en');await c.waitFor('document.documentElement.lang==="en"');
    assert.equal(await c.evaluate('document.querySelector(".settings-page input[type=number]").value'),'120','language must not remount settings draft');
    await c.evaluate('window.__XY_QA__.fail.set_interface_language="QA save failed: opaque 中文 JSON"');
    await c.fill('select[aria-label="Interface language"]','zh-CN');await c.waitFor('document.body.innerText.includes("QA save failed: opaque 中文 JSON")');
    assert.equal(await c.evaluate('document.documentElement.lang'),'en');
    assert.equal(await c.evaluate('document.querySelector("select[aria-label=\\"Interface language\\"]").value'),'en');
    assert.equal(await c.evaluate('window.__XY_QA__.language'),'en');
    await c.screenshot(join(output,'language-save-failure-en.png'));
    await c.evaluate('delete window.__XY_QA__.fail.set_interface_language');
    await c.fill('select[aria-label="Interface language"]','zh-CN');await c.waitFor('document.documentElement.lang==="zh-CN"');
    await c.fill('select[aria-label="界面语言"]','en');await c.waitFor('window.__XY_QA__.language==="en"');
    await c.navigate(base+'/');await c.waitFor('document.documentElement.lang==="en"');
    assert.equal(await c.evaluate('window.__XY_QA__.resolvedLanguage'),'en');
  });
  await run('preserve-search-and-unsaved-prompt',async()=>{
    await switchTo('zh-CN');await nav('供应商');await c.fill('input[aria-label="搜索供应商"]','Relay');
    await c.evaluate('void(window.__i18nQaSearch=document.querySelector("input[aria-label=搜索供应商]"))');
    await switchTo('en');assert.equal(await c.evaluate('document.querySelector("input[aria-label=\\"Search providers\\"]")===window.__i18nQaSearch'),true);
    assert.equal(await c.evaluate('window.__i18nQaSearch.value'),'Relay');
    await c.fill('input[aria-label="Search providers"]','');await nav('提示词');await c.button(await tr('添加提示词'));
    await c.fill(await field('名称'),'中文 User name');await c.fill(await field('提示词内容'),'未保存正文\nconst model = "我的模型";');
    await c.evaluate('void(window.__i18nQaDraft=document.querySelector("[role=dialog] textarea"))');
    await switchTo('zh-CN');assert.equal(await c.evaluate('document.querySelector("[role=dialog] textarea")===window.__i18nQaDraft'),true);
    assert.equal(await c.evaluate('window.__i18nQaDraft.value'),'未保存正文\nconst model = "我的模型";');
    await switchTo('en');assert.equal(await c.evaluate('window.__i18nQaDraft.value'),'未保存正文\nconst model = "我的模型";');
    await noOverflow();await c.screenshot(join(output,'unsaved-prompt-en.png'));await c.button(await tr('取消'));
    assert.equal(await c.evaluate('window.__XY_QA__.calls.filter(c=>c.command==="save_prompt").length'),0);
  });
  await run('system-primary-language-and-opaque-diagnostics',async()=>{
    await c.evaluate('Object.defineProperty(navigator,"languages",{configurable:true,value:["en-US","zh-CN"]})');
    await c.evaluate(`${languageImport}.then(m=>m.setLanguagePreference('system'))`);await c.waitFor('document.documentElement.lang==="en"');
    assert.equal(await c.evaluate('window.__XY_QA__.resolvedLanguage'),'en');
    await c.evaluate('Object.defineProperty(navigator,"languages",{configurable:true,value:["zh-TW","en-US"]});window.dispatchEvent(new Event("languagechange"))');
    await c.waitFor('document.documentElement.lang==="zh-CN"');await c.waitFor('window.__XY_QA__.resolvedLanguage==="zh-CN"');
    await switchTo('en');await nav('提示词');
    const diagnostic = 'HTTP 502 {"model":"我的模型","path":"opaque 中文"}';
    await c.evaluate('window.__XY_QA__.fail.list_prompts='+JSON.stringify('网络请求失败：'+diagnostic));await c.button(await tr('刷新'));
    await c.waitFor('document.querySelector(".xy-ui-inline-error")?.innerText.includes("HTTP 502")');
    const text=await c.evaluate('document.querySelector(".xy-ui-inline-error").innerText');assert(text.includes(diagnostic));assert(text.includes('Failed to load prompts: '),'translate the application wrapper; nested diagnostic stays opaque');
    await switchTo('zh-CN');assert((await c.evaluate('document.querySelector(".xy-ui-inline-error").innerText')).includes(diagnostic));
    await c.evaluate('delete window.__XY_QA__.fail.list_prompts');await c.button(await tr('刷新'));await c.waitFor('!document.querySelector(".xy-ui-inline-error")');
    await c.button(await tr('添加提示词'));await c.fill(await field('名称'),'诊断 QA');await c.fill(await field('提示词内容'),'opaque 正文');
    await c.evaluate('window.__XY_QA__.fail.save_prompt='+JSON.stringify('网络请求失败：'+diagnostic));await c.button(await tr('保存提示词'));
    await c.waitFor('document.querySelector("[role=dialog] .xy-ui-inline-error")?.innerText.includes("HTTP 502")');await switchTo('en');
    const saveError=await c.evaluate('document.querySelector("[role=dialog] .xy-ui-inline-error").innerText');assert(saveError.includes('Network request failed: '));assert(saveError.includes(diagnostic));
    await c.evaluate('delete window.__XY_QA__.fail.save_prompt');await c.button(await tr('取消'));
  });
  const pages=['供应商','MCP 服务器','提示词','Skills','会话','用量与审计','设置'];
  for(const language of ['zh-CN','en']) for(const [width,height] of [[1180,760],[820,600],[390,844]]) for(const theme of ['light','dark']) {
    await run(`layout-${language}-${width}-${theme}`,async()=>{
      await switchTo(language);await c.viewport(width,height);
      await c.evaluate('localStorage.setItem("xy-switch:theme",'+JSON.stringify(theme)+');window.dispatchEvent(new StorageEvent("storage",{key:"xy-switch:theme",newValue:'+JSON.stringify(theme)+'}));document.documentElement.dataset.theme='+JSON.stringify(theme));
      for(const [i,page] of pages.entries()) {
        await nav(page);await noOverflow();
        const snapshot=language==='en'?await noLeaks():await c.snapshot();
        snapshots.push({language,width,height,theme,page,scrollWidth:snapshot.scrollWidth});
        await c.screenshot(join(output,`${language}-${width}-${theme}-${i}.png`));
        if(page==='提示词')assert(snapshot.text.includes('代码审查'),'user prompt names remain Chinese');
        if(page==='MCP 服务器')assert(snapshot.text.includes('合成 MCP 服务'),'user MCP description remains Chinese');
      }
    });
  }
  await run('mobile-capability-boundaries',async()=>{
    await c.call('Page.removeScriptToEvaluateOnNewDocument',{identifier:fixtureId});
    ({identifier:fixtureId}=await c.call('Page.addScriptToEvaluateOnNewDocument',{source:fixtureSource+'\nwindow.__XY_QA__.platform={os:"android",mobile:true,systemTray:false,autostart:false,clientTakeover:false,credentialStore:"Android Keystore"};'}));
    await c.navigate(base+'/');await c.waitFor('document.body.innerText.includes("Orion")');await switchTo('en');await nav('设置');
    assert.equal(await c.evaluate('Boolean(document.querySelector("button[aria-label=\\"Start at login\\"]"))'),false);
    assert.equal(await c.evaluate('document.querySelector("button[aria-label=\\"Minimize to tray on close\\"]").disabled'),true);
    await c.screenshot(join(output,'mobile-settings-en.png'));
    const writes=await c.evaluate('window.__XY_QA__.calls.filter(c=>c.command==="set_app_takeover_enabled").length');assert.equal(writes,0);
  });
  console.log(JSON.stringify({passed:reports.filter(r=>r.passed).length,screens:snapshots.length,output}));
} finally {
  if(fixtureId)await c.call('Page.removeScriptToEvaluateOnNewDocument',{identifier:fixtureId}).catch(()=>{});
  await c.close();
}
