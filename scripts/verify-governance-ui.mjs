import assert from 'node:assert/strict';
import {readFile,writeFile,mkdir} from 'node:fs/promises';
import {join} from 'node:path';
import {connect} from './workspace-qa/cdp.mjs';

const base=process.env.APP_URL??'http://127.0.0.1:1431';
const output=process.env.QA_OUTPUT??'artifacts/governance-qa/ui';
await mkdir(output,{recursive:true});
const source=`localStorage.setItem('xy-qa:native-language',localStorage.getItem('xy-governance-qa:language')||'zh-CN');
`+
  await readFile(new URL('./workspace-qa/fixture.js',import.meta.url),'utf8')+'\n'+
  await readFile(new URL('./workspace-qa/governance-fixture.js',import.meta.url),'utf8');
const c=await connect(undefined,{fresh:true}), reports=[], only=process.env.QA_ONLY?.split(',');
const page='document.querySelector(".governance-page")';
const expression=label=>`[...${page}.querySelectorAll('button')].find(b=>b.textContent.trim()===${JSON.stringify(label)}||b.getAttribute('aria-label')===${JSON.stringify(label)})`;
const tab=name=>c.click('#governance-tab-'+name);
const text=()=>c.evaluate(page+'.innerText');
const calls=command=>c.evaluate(`window.__XY_QA__.calls.filter(c=>c.command===${JSON.stringify(command)})`);
const click=label=>c.button(label,page);
const disabled=async(label,value=true)=>assert.equal(await c.evaluate(`Boolean(${expression(label)}?.matches(':disabled'))`),value,label);
const field=async label=>{
  const id=await c.evaluate(`[...${page}.querySelectorAll('label')].find(e=>e.textContent.trim()===${JSON.stringify(label)})?.htmlFor`);
  assert(id,'Missing field '+label);return '[id='+JSON.stringify(id)+']';
};
const fill=async(label,value)=>c.fill(await field(label),String(value));
const check=async(label,value=true)=>{
  const e=`[...${page}.querySelectorAll('label')].find(e=>e.textContent.trim()===${JSON.stringify(label)})?.querySelector('input[type="checkbox"]')`;
  const current=await c.evaluate(e+'?.checked');assert.notEqual(current,undefined,'Missing checkbox '+label);
  if(current!==value)await c.clickExpression(e);
};
const confirm=(word,value=word)=>fill('输入确认文字 · '+word,value);
async function noOverflow(){const m=await c.evaluate('({width:innerWidth,root:document.documentElement.scrollWidth,body:document.body.scrollWidth})');assert(m.root<=m.width&&m.body<=m.width,JSON.stringify(m));}
async function reset({width=1280,language='zh-CN',mobile=false,running=false,budget=false,theme='light'}={}){
  await c.evaluate(`localStorage.setItem('xy-governance-qa:language',${JSON.stringify(language)});localStorage.setItem('xy-switch:language',${JSON.stringify(language)});localStorage.setItem('xy-governance-qa:mobile','${mobile}');localStorage.setItem('xy-governance-qa:running','${running}');localStorage.setItem('xy-governance-qa:budget','${budget}');localStorage.setItem('xy-switch:theme','${theme}');`);
  await c.viewport(width,width<500?844:960);await c.navigate(base+'/');
  await c.click('.xy-shell-nav-item[title='+JSON.stringify(language==='en'?'Safety & Acceptance':'安全与验收')+']');
  await c.waitFor('Boolean(document.querySelector("#governance-tab-audit"))');
}
async function mountPreview(){
  await tab('mounts');await click('选择项目目录');await check('代码审查');await click('只预览，不写入');
  await c.waitFor('Boolean(document.querySelector(".gov-diff"))');
}
async function run(name,work){
  if(only&&!only.includes(name))return;
  const start=c.exceptions.length;console.log('RUN '+name);
  try{await work();assert.equal(c.exceptions.length,start,'Unexpected browser exception');reports.push({name,passed:true});console.log('PASS '+name);}
  catch(error){reports.push({name,passed:false,error:String(error)});await c.screenshot(join(output,name+'-failure.png'));await writeFile(join(output,name+'-failure.json'),JSON.stringify({snapshot:await c.snapshot(),exceptions:c.exceptions.slice(start)},null,2));console.error('FAIL '+name+': '+error);}
  finally{await writeFile(join(output,'results.json'),JSON.stringify(reports,null,2));}
}
let fixtureId;
try{
  ({identifier:fixtureId}=await c.call('Page.addScriptToEvaluateOnNewDocument',{source}));
  await c.navigate(base+'/');
  await run('overview-defaults',async()=>{
    await reset();assert.equal(await c.evaluate(`${page}.querySelectorAll('[role="tab"]').length`),7);
    assert((await text()).includes('98.5%'));assert(!(await text()).includes('9850'));
    for(const command of ['run_synthetic_check','save_synthetic_monitor','create_access_token','apply_explicit_mount','apply_config_rollback'])assert.equal((await calls(command)).length,0,command);
    await noOverflow();await c.screenshot(join(output,'desktop-overview.png'));
  });
  await run('access-confirmation-and-token-lifetime',async()=>{
    await reset();await tab('access');await disabled('保存访问策略');await check('我已检查策略，确认应用此变更');await disabled('保存访问策略',false);
    await fill('每分钟请求上限',60);await disabled('保存访问策略');await check('我已检查策略，确认应用此变更');await click('保存访问策略');
    await c.waitFor("document.body.innerText.includes('访问策略已保存')");assert.equal((await calls('save_access_policy'))[0].args.policy.requestsPerMinute,60);
    await fill('令牌名称','QA read-only');await c.fill(await field('权限范围'),'monitor');await fill('有效天数',30);await click('创建令牌');
    await c.waitFor('Boolean(document.querySelector(".gov-secret"))');assert((await text()).includes('xy_qa_single_display'));
    assert(!(await c.evaluate('JSON.stringify({...localStorage,...sessionStorage})')).includes('xy_qa_single_display'));
    assert.equal((await calls('create_access_token'))[0].args.scope,'monitor');
    await click('我已保存，隐藏令牌');assert(!(await text()).includes('xy_qa_single_display'));
    await tab('overview');await tab('access');assert(!(await text()).includes('xy_qa_single_display'));
    await click('撤销');await disabled('确认撤销');await confirm('REVOKE','revoke');await disabled('确认撤销');await confirm('REVOKE');await click('确认撤销');
    await c.waitFor("window.__XY_QA__.governance.tokens[0].revoked");assert.equal((await calls('revoke_access_token')).length,1);
  });
  await run('running-proxy-locks-sensitive-changes',async()=>{
    await reset({running:true});await tab('access');await check('我已检查策略，确认应用此变更');await disabled('保存访问策略');
    await tab('versions');await click('预览回滚差异');await check('我已查看上述 diff');await confirm('ROLLBACK');await disabled('确认回滚配置');
    assert.equal((await calls('save_access_policy')).length,0);assert.equal((await calls('apply_config_rollback')).length,0);
  });
  await run('budget-validation-and-persistent-reservation',async()=>{
    await reset();await tab('budget');await disabled('保存预算策略');await click('启用硬预算准入');await fill('每日 Token 预留上限',-1);await check('我已检查策略，确认应用此变更');await disabled('保存预算策略');
    await fill('每日 Token 预留上限',20000);await disabled('保存预算策略');await check('我已检查策略，确认应用此变更');await click('保存预算策略');
    await c.waitFor("document.body.innerText.includes('预算策略已保存')");const call=(await calls('save_hard_budget'))[0];assert.equal(call.args.policy.dailyTokens,20000);assert.equal(call.args.confirmed,true);
    assert.equal(await c.evaluate('window.__XY_QA__.governance.budget.reservedTokens'),4096);assert((await text()).includes('4,096'));
  });
  await run('paid-monitor-three-gates-and-single-dispatch',async()=>{
    await reset({running:true});await tab('monitor');await confirm('ALLOW PAID CHECKS');await disabled('执行一次收费验收');assert.equal((await calls('run_synthetic_check')).length,0);
    await reset({running:true,budget:true});await tab('monitor');await disabled('执行一次收费验收');await confirm('ALLOW PAID CHECKS','ALLOW');await disabled('执行一次收费验收');
    await confirm('ALLOW PAID CHECKS');await disabled('执行一次收费验收',false);await c.fill(await field('验收模式'),'tools');await disabled('执行一次收费验收');
    await confirm('ALLOW PAID CHECKS');await c.evaluate("window.__XY_QA__.hold.push('run_synthetic_check')");await click('执行一次收费验收');await disabled('执行一次收费验收');
    assert.equal((await calls('run_synthetic_check')).length,1);await c.evaluate("window.__XY_QA__.release('run_synthetic_check')");await c.waitFor('document.querySelectorAll(".gov-receipts li").length===1');
    assert((await text()).includes('HTTP 200'));assert.equal((await calls('run_synthetic_check'))[0].args.policy.mode,'tools');await disabled('执行一次收费验收');
    assert.equal((await calls('save_synthetic_monitor')).length,0);await c.screenshot(join(output,'monitor-receipt.png'));
  });
  await run('version-preview-stale-error-and-exact-hashes',async()=>{
    await reset();await tab('versions');await click('预览回滚差异');assert.equal((await calls('apply_config_rollback')).length,0);
    await disabled('确认回滚配置');await confirm('ROLLBACK');await disabled('确认回滚配置');await check('我已查看上述 diff');await disabled('确认回滚配置',false);
    assert.equal(await c.evaluate('document.querySelectorAll(".gov-diff [data-kind=remove]").length'),1);
    await c.evaluate("window.__XY_QA__.fail.apply_config_rollback='QA stale configuration: preview again'");await click('确认回滚配置');
    await c.waitFor("document.body.innerText.includes('QA stale configuration')");assert(!(await text()).includes('已回滚配置，并创建了回滚前备份'));
    await c.evaluate('delete window.__XY_QA__.fail.apply_config_rollback');await click('确认回滚配置');await c.waitFor("document.body.innerText.includes('已回滚配置，并创建了回滚前备份')");
    const args=(await calls('apply_config_rollback'))[1].args;assert.deepEqual(args,{id:'qa-version-1',expectedCurrentHash:'qa-current-hash',targetHash:'qa-target-hash',confirmation:'ROLLBACK'});
  });
  await run('explicit-mount-preview-expiry-and-atomic-confirmation',async()=>{
    await reset();await tab('mounts');await disabled('只预览，不写入');await mountPreview();
    assert.equal((await calls('apply_explicit_mount')).length,0);const args=(await calls('preview_explicit_mount'))[0].args;
    assert.deepEqual(args,{rootId:'qa-root-handle',client:'codex',promptIds:['qa-prompt'],skillIds:[]});
    assert((await text()).includes('Keep this block.'));await disabled('备份并应用文件变更');await confirm('WRITE FILE');await disabled('备份并应用文件变更');await check('我已查看上述 diff');await disabled('备份并应用文件变更',false);
    await c.screenshot(join(output,'explicit-mount-diff.png'));
    await c.evaluate('window.__qaNow=Date.now;Date.now=()=>window.__qaNow()+301000');await c.waitFor("document.body.innerText.includes('预览已过期，请重新生成')");await disabled('备份并应用文件变更');
    await c.evaluate('Date.now=window.__qaNow;delete window.__qaNow');
    // Let the component clock observe restored time before generating a new preview.
    await c.waitFor("!document.body.innerText.includes('预览已过期，请重新生成')");
    await c.fill(await field('挂载目标'),'gemini');assert.equal(await c.evaluate('Boolean(document.querySelector(".gov-diff"))'),false);
    await click('只预览，不写入');await c.waitFor('Boolean(document.querySelector(".gov-diff"))');await disabled('备份并应用文件变更');assert((await text()).includes('GEMINI.md'));
    await check('我已查看上述 diff');await confirm('WRITE FILE');await c.evaluate("window.__XY_QA__.hold.push('apply_explicit_mount')");await click('备份并应用文件变更');await disabled('备份并应用文件变更');await disabled('选择项目目录');
    assert.equal((await calls('apply_explicit_mount')).length,1);await c.evaluate("window.__XY_QA__.release('apply_explicit_mount')");await c.waitFor("document.body.innerText.includes('文件已按预览更新，原文备份已保留')");
    assert.equal(await c.evaluate('window.__XY_QA__.governance.backups.length'),1);assert.equal(await c.evaluate('Boolean(document.querySelector(".gov-diff"))'),false);
  });
  await run('backup-preview-and-explicit-deletion',async()=>{
    await reset();await c.evaluate("window.__XY_QA__.governance.backups=[{id:'qa-existing-backup',target:'QA:/selected/project/AGENTS.md',createdAt:1790000000}]");await tab('mounts');await disabled('预览文件回滚');
    await click('选择项目目录');await click('预览文件回滚');await disabled('备份并应用文件变更');assert.equal((await calls('apply_explicit_mount')).length,0);await click('取消');
    await click('删除备份');await disabled('永久删除此备份');await confirm('DELETE BACKUP');await click('永久删除此备份');await c.waitFor('window.__XY_QA__.governance.backups.length===0');
    assert.equal((await calls('apply_explicit_mount')).length,0);await tab('versions');await click('删除版本');await disabled('永久删除此版本');await confirm('DELETE VERSION');await click('永久删除此版本');await c.waitFor('window.__XY_QA__.governance.versions.length===0');
  });
  await run('mobile-native-is-read-only',async()=>{
    await reset({width:390,mobile:true,running:true,budget:true});assert.equal(await c.evaluate(`${page}.querySelectorAll('[role="tab"]').length`),3);
    assert((await text()).includes('仅显示本机数据'));assert((await text()).includes('尚未连接其他设备'));
    await noOverflow();await c.screenshot(join(output,'mobile-read-only.png'));await tab('monitor');assert.equal(await c.evaluate(`${page}.querySelectorAll('input,select').length`),0);
    assert(!(await text()).includes('执行一次收费验收'));await tab('audit');await noOverflow();
    assert.equal((await calls('run_synthetic_check')).length,0);assert.equal((await calls('choose_mount_root')).length,0);
  });
  await run('english-dark-responsive-all-tabs',async()=>{
    await reset({width:390,language:'en',theme:'dark'});
    assert.deepEqual(await c.evaluate(`[...${page}.querySelectorAll('[role="tab"]')].map(e=>e.textContent)`),['Read-only overview','Access control','Hard budget','Acceptance monitoring','Config versions','Explicit mounts','Governance audit']);
    for(const name of ['overview','access','budget','monitor','versions','mounts','audit']){await tab(name);await noOverflow();}
    await tab('mounts');assert((await text()).includes('Choose project directory'));await c.screenshot(join(output,'english-dark-mounts.png'));
  });
  await run('browser-preview-never-pretends-to-save',async()=>{
    await c.call('Page.removeScriptToEvaluateOnNewDocument',{identifier:fixtureId});fixtureId=null;
    await reset();assert((await text()).includes('浏览器中仅展示预览'));assert.equal(await c.evaluate('"__TAURI_INTERNALS__" in window'),false);
    await tab('budget');await check('我已检查策略，确认应用此变更');await click('保存预算策略');
    await c.waitFor("document.body.innerText.includes('浏览器预览不能保存治理设置')");assert(!(await text()).includes('预算策略已保存，历史预留保持不变'));
    await tab('monitor');await confirm('ALLOW PAID CHECKS');await disabled('执行一次收费验收');await noOverflow();
  });
}finally{await c.close();}
console.log(`${reports.filter(r=>r.passed).length}/${reports.length} governance browser checks passed`);
if(reports.some(r=>!r.passed))process.exitCode=1;
