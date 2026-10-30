import assert from 'node:assert/strict';
import { mkdir, writeFile } from 'node:fs/promises';
import { resolve, join } from 'node:path';
import { connect } from './workspace-qa/cdp.mjs';

const base = process.env.APP_URL ?? 'http://127.0.0.1:1431';
const output = resolve(process.env.WEBSITE_QA_OUTPUT ?? 'artifacts/website-ui-qa');
await mkdir(output, {recursive:true});
const c = await connect(undefined, {fresh:true});
const reports = [];
async function run(name, work) {
  try {
    await work();
    assert.equal(c.exceptions.length, 0, 'Unexpected page exception');
    reports.push({name, passed:true});
    console.log('PASS ' + name);
  } catch (error) {
    reports.push({name, passed:false, error:String(error)});
    await c.screenshot(join(output, name+'-failure.png'));
    throw error;
  } finally {
    await writeFile(join(output,'results.json'), JSON.stringify({reports},null,2));
  }
}
try {
  for (const language of ['en','zh-CN']) for (const width of [1180,820,390]) {
    await run(language+'-'+width,async()=>{
      await c.viewport(width,width===390?844:760);
      await c.navigate(base+'/website/?lang='+language);
      await c.waitFor('document.documentElement.lang==='+JSON.stringify(language));
      const state=await c.evaluate(`(() => {
        const language=document.documentElement.lang;
        const catalog=JSON.parse(document.getElementById('translations').textContent)[language];
        return {language, platforms:[...document.querySelectorAll(".platform h3")].map(e=>e.dataset.i18n), width:innerWidth, scroll:document.documentElement.scrollWidth,
          untranslated:[...document.querySelectorAll('[data-i18n]')].filter(e=>e.textContent!==catalog[e.dataset.i18n]).map(e=>e.dataset.i18n),
          selected:document.querySelector('[data-language][aria-pressed=true]')?.dataset.language,
          docs:[...document.querySelectorAll('[data-document]')].map(e=>e.href),
          remoteResources:performance.getEntriesByType('resource').filter(e=>new URL(e.name).origin!==location.origin).map(e=>e.name)};
      })()`);
      assert.equal(state.language,language);assert.equal(state.selected,language);
      assert.equal(state.untranslated.length,0);assert(state.scroll<=state.width,JSON.stringify(state));
      assert.equal(state.remoteResources.length,0);
      assert.deepEqual(state.platforms,["windows","linux","macos"]);
      assert(state.docs[0].endsWith(language==='en'?'user-guide-en.md':'user-guide-zh.md'));
      assert(state.docs[1].endsWith(language==='en'?'packaging-en.md':'packaging.md'));
      await c.screenshot(join(output,language+'-'+width+'.png'));
    });
  }
  await run('language-buttons-and-persistence',async()=>{
    await c.navigate(base+'/website/?lang=en');
    await c.click('[data-language="zh-CN"]');
    assert.equal(await c.evaluate('document.documentElement.lang'),'zh-CN');
    assert.equal(await c.evaluate('localStorage.getItem("xy-switch:website-language")'),'zh-CN');
    await c.navigate(base+'/website/');
    assert.equal(await c.evaluate('document.documentElement.lang'),'zh-CN');
    await c.click('[data-language="en"]');
    await c.navigate(base+'/website/');
    assert.equal(await c.evaluate('document.documentElement.lang'),'en');
  });
  await run('query-language-overrides-saved-preference',async()=>{
    await c.navigate(base+'/website/?lang=zh-CN');
    assert.equal(await c.evaluate('document.documentElement.lang'),'zh-CN');
    assert.equal(await c.evaluate('localStorage.getItem("xy-switch:website-language")'),'en');
  });
  console.log(JSON.stringify({passed:reports.length,output}));
} finally { await c.close(); }
