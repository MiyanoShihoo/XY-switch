import assert from 'node:assert/strict';
import {readFile,writeFile,mkdir} from 'node:fs/promises';
import {join} from 'node:path';
import {connect} from './workspace-qa/cdp.mjs';

const base=process.env.APP_URL??'http://127.0.0.1:1431';
const output=process.env.QA_OUTPUT??'out/xy-switch_governance-ui';
await mkdir(output,{recursive:true});
const source=`localStorage.setItem('xy-qa:native-language',localStorage.getItem('xy-governance-qa:language')||'zh-CN');
`+
  await readFile(new URL('./workspace-qa/fixture.js',import.meta.url),'utf8')+'\n'+
  await readFile(new URL('./workspace-qa/governance-fixture.js',import.meta.url),'utf8');
const requested=(process.env.QA_ONLY??'').split(',').map(name=>name.trim()).filter(Boolean);
const only=requested.length?requested:null;
const c=await connect(undefined,{fresh:true}), reports=[];
const page='document.querySelector(".governance-page")';
const expression=label=>`[...${page}.querySelectorAll('button')].find(b=>b.textContent.trim()===${JSON.stringify(label)}||b.getAttribute('aria-label')===${JSON.stringify(label)})`;
const taskNames=['access','tokens','budget','tasks','monitor','versions','mounts','audit'];
const taskSwitcher='.gov-task-switcher select', backButton='.governance-page .page-heading button';
const leaveDialog='.task-leave-dialog[role="dialog"]';
const writeCommands=['save_access_policy','create_access_token','revoke_access_token','save_hard_budget','cancel_proxy_task','save_synthetic_monitor','run_synthetic_check','create_config_version','apply_config_rollback','delete_config_version','apply_explicit_mount','delete_mount_backup'];
let taskEntries=[];
const writes=()=>c.evaluate(`window.__XY_QA__?.calls.filter(call=>${JSON.stringify(writeCommands)}.includes(call.command))??[]`);
const taskState=()=>c.evaluate(`({workspace:document.querySelector('.xy-shell-root')?.dataset.workspaceView,task:document.querySelector('${taskSwitcher}')?.value,heading:document.querySelector('#governance-title')?.textContent,region:document.querySelector('.gov-panel')?.getAttribute('aria-label'),tabs:[...document.querySelectorAll('.gov-panel [role=tab][aria-selected=true]')].map(e=>e.id),fields:[...document.querySelectorAll('.gov-panel input,.gov-panel select,.gov-panel textarea')].map(e=>[e.id,e.value,e.checked])})`);
async function interactive(selector){
  const entry='document.querySelector('+JSON.stringify(selector)+')';
  await c.waitFor(`Boolean(${entry}?.getClientRects().length)&&!${entry}.matches(':disabled')`);
  assert(await c.evaluate(`!${entry}.matches(':disabled')&&!${entry}.closest('details:not([open])')&&getComputedStyle(${entry}).visibility!=='hidden'&&!document.querySelector('[aria-modal="true"]')`),'Control is not interactive: '+selector);
}
async function homeEntries(){
  await c.waitFor('Boolean(document.querySelector("#gov-task-audit")?.getClientRects().length)&&!document.querySelector(".gov-task-switcher")');
  const entries=await c.evaluate(`[...${page}.querySelectorAll('.gov-task-index nav > button')].map(e=>({value:e.id.replace(/^gov-task-/,''),label:e.querySelector('strong')?.textContent.trim(),visible:e.getClientRects().length>0}))`);
  assert(entries.every(entry=>entry.visible&&entry.label),'Task entries must be visible and labelled');
  return entries.map(({value,label})=>({value,label}));
}
async function categoryLabels(){return (await homeEntries()).map(entry=>entry.label);}
async function assertTask(name){
  const entry=taskEntries.find(task=>task.value===name);assert(entry,'Unavailable task '+name);
  await c.waitFor(`document.querySelector('${taskSwitcher}')?.value===${JSON.stringify(name)}&&document.querySelector('.gov-panel[role=region]')?.getAttribute('aria-label')===${JSON.stringify(entry.label)}&&document.querySelector('#governance-title')?.textContent===${JSON.stringify(entry.label)}`);
  assert.deepEqual(await c.evaluate(`[...document.querySelectorAll('${taskSwitcher} option')].map(e=>({value:e.value,label:e.textContent.trim()}))`),taskEntries);
}
async function navigateTask(action,{discard=false,allowDiscard=false}={}){
  assert.equal(await c.evaluate('Boolean(document.querySelector("[aria-modal=true]"))'),false,'Close the current dialog before navigating');
  const before=await taskState(),beforeWrites=await writes();
  await action();
  if(discard||(allowDiscard&&await c.evaluate(`Boolean(document.querySelector('${leaveDialog}'))`))){
    for(const decision of ['continue','discard']){
      await c.waitFor(`Boolean(document.querySelector('${leaveDialog}')?.getClientRects().length)&&document.querySelector('${leaveDialog}').contains(document.activeElement)`);
      assert.equal(await c.evaluate(`document.querySelector('${leaveDialog}').getAttribute('aria-modal')`),'true');
      assert.equal(await c.evaluate(`Boolean(document.querySelector('${leaveDialog}').closest('.governance-page'))`),false,'Discard confirmation must be separate from the task panel');
      assert.deepEqual(await taskState(),before,'Navigation must retain the current task and draft until discard');
      assert.deepEqual(await writes(),beforeWrites,'Opening a discard confirmation must not write');
      await c.click(leaveDialog+' .xy-ui-modal__footer > button:'+(decision==='continue'?'first-child':'last-child'));
      await c.waitFor(`!document.querySelector('${leaveDialog}')`);
      if(decision==='continue'){
        assert.deepEqual(await taskState(),before,'Continue editing must preserve fields and confirmations');
        assert.deepEqual(await writes(),beforeWrites,'Continue editing must not write');
        await action();
      }
    }
  }else assert.equal(await c.evaluate(`Boolean(document.querySelector('${leaveDialog}'))`),false,'Unexpected unsaved draft');
  assert.deepEqual(await writes(),beforeWrites,'Task navigation must not write or dispatch paid checks');
}
async function openTask(name,options){
  assert(taskNames.includes(name),'Unknown governance task '+name);
  assert(taskEntries.some(entry=>entry.value===name),'Task is not available on this platform: '+name);
  const active=await c.evaluate(`document.querySelector('${taskSwitcher}')?.value`);
  if(active!==name)await navigateTask(async()=>{
    if(active){await interactive(taskSwitcher);await c.fill(taskSwitcher,name);}
    else{const selector='#gov-task-'+name;await interactive(selector);await c.click(selector);}
  },options);
  await assertTask(name);
}
async function returnHome(options){
  await navigateTask(async()=>{await interactive(backButton);await c.click(backButton);},options);
  assert.deepEqual(await homeEntries(),taskEntries);
}
async function navigationBusy(expected=true){
  await c.waitFor(`document.querySelector('${taskSwitcher}')?.disabled===${expected}`);
  assert.equal(await c.evaluate(`document.querySelector('${backButton}')?.disabled`),expected,'Back navigation busy state');
  if(!expected)await c.waitFor('!document.querySelector(".task-navigation-wait")');
}
async function busyDialog(selector){
  await c.waitFor(`document.querySelector(${JSON.stringify(selector)})?.getAttribute('aria-busy')==='true'`);
  assert.equal(await c.evaluate(`document.querySelector(${JSON.stringify(selector+' header button')})?.matches(':disabled')`),true,'Busy dialog cannot be closed');
  await c.key('Escape');assert.equal(await c.evaluate(`Boolean(document.querySelector(${JSON.stringify(selector)}))`),true,'Escape must not dismiss a busy operation');
}
async function subtab(section,name,options){
  assert(({monitor:['configuration','receipts'],mounts:['configure','backups']})[section]?.includes(name),'Unknown task-local tab');
  const selector='#'+section+'-tab-'+name;
  await navigateTask(async()=>{await interactive(selector);await c.click(selector);},options);
  await c.waitFor('document.querySelector('+JSON.stringify(selector)+')?.getAttribute("aria-selected")==="true"&&Boolean(document.querySelector('+JSON.stringify('#'+section+'-panel-'+name)+'))');
}
async function editBudget(){
  await assertTask('budget');await c.waitFor('Boolean(document.querySelector(".gov-ledger"))&&!document.querySelector(".gov-policy-editor")');
  await click('修改预算策略');await c.waitFor('Boolean(document.querySelector(".gov-policy-editor")?.getClientRects().length)');
}
async function visibleSection(selector,label){
  const entry=`[...${page}.querySelectorAll(${JSON.stringify(selector)})].find(e=>e.querySelector('h3')?.textContent.trim()===${JSON.stringify(label)})`;
  assert(await c.evaluate(`Boolean(${entry}?.getClientRects().length)&&!${entry}?.closest('details:not([open])')`),'Missing visible section '+label);
}
async function reviewDiff(identity){
  await c.waitFor('Boolean(document.querySelector(".gov-review-modal[role=dialog][aria-busy=false] .gov-diff")?.getClientRects().length)');
  assert.equal(await c.evaluate('document.querySelector(".gov-review-modal .gov-diff").getAttribute("aria-label")'),'变更预览');
  if(identity)assert.equal(await c.evaluate('document.querySelector(".gov-review-modal .gov-diff__identity").textContent'),identity);
  assert.deepEqual(await c.evaluate('[...document.querySelectorAll(".gov-review-modal .gov-diff-lines thead th")].map(e=>e.textContent)'),['修改前','修改后','变更类型','内容']);
}
async function disclose(label,open=true){
  const entry=`[...${page}.querySelectorAll('details')].find(e=>e.querySelector(':scope > summary')?.textContent.trim().startsWith(${JSON.stringify(label)}))`;
  const current=await c.evaluate(entry+'?.open');assert.notEqual(current,undefined,'Missing disclosure '+label);
  if(current!==open)await c.clickExpression(entry+'.querySelector(":scope > summary")');
  await c.waitFor(entry+'.open==='+open);
}
async function closeDialog(){
  if(await c.evaluate(`Boolean(${page}?.querySelector('[role="dialog"]'))`)){
    await c.waitFor(`!${page}?.querySelector('[role="dialog"][aria-busy="true"]')`);
    await c.key('Escape');await c.waitFor(`!${page}?.querySelector('[role="dialog"]')`);
  }
}
async function expandAttempts(){
  const count=await c.evaluate('document.querySelectorAll(".gov-task-attempts details:not([open])").length');
  for(let index=0;index<count;index++)await c.click('.gov-task-attempts details:not([open]) > summary');
}
const text=()=>c.evaluate(page+'.innerText');
const calls=command=>c.evaluate(`window.__XY_QA__.calls.filter(c=>c.command===${JSON.stringify(command)})`);
const click=label=>c.button(label,page);
const taskRows='.gov-task-table tbody tr';
const taskCard='document.querySelector(".gov-panel > .gov-card")';
const taskText=()=>c.evaluate('document.querySelector(".gov-task-detail").innerText');
const waitTask=async id=>{
  await c.waitFor('document.querySelector(".gov-task-detail .gov-task-summary > .gov-facts code")?.textContent==='+JSON.stringify(id));
  if(!await c.evaluate('document.querySelector(".gov-task-summary details").open'))await c.click('.gov-task-summary details > summary');
  await c.waitFor('document.querySelector(".gov-task-summary details").open');
};
const selectTask=async(id,wait=true)=>{
  await closeDialog();
  await openTask('tasks');
  const row='[...document.querySelectorAll('+JSON.stringify(taskRows)+')].find(e=>e.querySelector("th[scope=row] code")?.textContent==='+JSON.stringify(id)+')';
  await c.waitFor('Boolean('+row+')');
  const button=row+'?.querySelector("button")';
  assert.equal(await c.evaluate(button+'.getAttribute("aria-label")'),(await c.evaluate('document.documentElement.lang')).startsWith('en')?'View task · '+id:'查看任务 · '+id);
  await c.clickExpression(button);
  await c.waitFor('Boolean(document.querySelector(".gov-panel .xy-ui-drawer[role=dialog]"))');
  if(wait)await waitTask(id);
};
const taskValue=async label=>{
  const entry=`[...document.querySelectorAll('.gov-task-detail .gov-task-summary .gov-fact')].find(e=>e.querySelector('dt')?.textContent===${JSON.stringify(label)})?.querySelector('dd')`;
  assert(await c.evaluate(`Boolean(${entry}?.getClientRects().length)&&!${entry}?.closest('details:not([open])')`),'Task value is not visible: '+label);
  const value=await c.evaluate(`(() => {const value=${entry},pair=value.querySelector('.gov-cost-pair');return pair?[...pair.children].map(e=>e.innerText).join(' '):value.innerText;})()`);
  return value.replace(/\s+/g,' ').trim();
};
const disabled=async(label,value=true)=>{
  const actual=await c.evaluate(`${expression(label)}?.matches(':disabled')`);
  assert.notEqual(actual,undefined,'Missing button '+label);assert.equal(actual,value,label);
};
const field=async label=>{
  const id=await c.evaluate(`[...${page}.querySelectorAll('label')].find(e=>e.textContent.trim()===${JSON.stringify(label)})?.htmlFor`);
  assert(id,'Missing field '+label);
  assert(await c.evaluate(`(() => {const e=document.getElementById(${JSON.stringify(id)}),dialog=${page}.querySelector('[role="dialog"]');return e.getClientRects().length>0&&!e.closest('details:not([open])')&&(!dialog||dialog.contains(e));})()`),'Field is not accessible '+label);
  return '[id='+JSON.stringify(id)+']';
};
const fill=async(label,value)=>c.fill(await field(label),String(value));
const check=async(label,value=true)=>{
  const e=`[...${page}.querySelectorAll('label')].find(e=>e.textContent.trim()===${JSON.stringify(label)})?.querySelector('input[type="checkbox"]')`;
  const current=await c.evaluate(e+'?.checked');assert.notEqual(current,undefined,'Missing checkbox '+label);
  if(current!==value)await c.clickExpression(e);
};
const confirm=(word,value=word)=>fill('输入确认文字 · '+word,value);
async function noOverflow(){const m=await c.evaluate('({width:innerWidth,root:document.documentElement.scrollWidth,body:document.body.scrollWidth})');assert(m.root<=m.width&&m.body<=m.width,JSON.stringify(m));}
async function reset({width=1280,language='zh-CN',mobile=false,running=false,budget=false,theme='light',discard=false,setup}={}){
  await closeDialog();
  if(await c.evaluate(`Boolean(document.querySelector('${taskSwitcher}'))`)){
    await navigationBusy(false);await returnHome({discard});
  }
  await c.evaluate(`localStorage.setItem('xy-governance-qa:language',${JSON.stringify(language)});localStorage.setItem('xy-switch:language',${JSON.stringify(language)});localStorage.setItem('xy-governance-qa:mobile','${mobile}');localStorage.setItem('xy-governance-qa:running','${running}');localStorage.setItem('xy-governance-qa:budget','${budget}');localStorage.setItem('xy-switch:theme','${theme}');`);
  await c.viewport(width,width<500?844:960);await c.navigate(base+'/',{readySelector:mobile?'.workspace-route-notice':'h1'});
  if(setup)await c.evaluate(setup);
  await c.workspace('governance');
  await c.waitFor(`document.querySelectorAll('.gov-task-index nav > button').length===${mobile?2:taskNames.length}`);
  if(mobile)await c.waitFor(`Boolean(${page}?.querySelector(':scope > .gov-warning'))`);
  taskEntries=await homeEntries();assert.deepEqual(taskEntries.map(entry=>entry.value),mobile?['monitor','audit']:taskNames);
}
async function mountPreview(){
  await openTask('mounts');await click('选择项目目录');await navigationBusy(false);await field('挂载目标');await click('继续选择来源');await visibleSection('.gov-source-picker','选择挂载来源');await disabled('只预览，不写入');
  assert.equal((await calls('preview_explicit_mount')).length,0);await check('代码审查');await disabled('只预览，不写入',false);await click('只预览，不写入');
  await reviewDiff('QA:/selected/project/AGENTS.md');
}
async function run(name,work){
  if(only&&!only.includes(name))return;
  const start=c.exceptions.length;console.log('RUN '+name);
  try{await work();assert.equal(c.exceptions.length,start,'Unexpected browser exception');c.assertLocalNetwork();reports.push({name,passed:true});console.log('PASS '+name);}
  catch(error){
    reports.push({name,passed:false,error:String(error)});await c.screenshot(join(output,name+'-failure.png'));await writeFile(join(output,name+'-failure.json'),JSON.stringify({snapshot:await c.snapshot(),exceptions:c.exceptions.slice(start)},null,2));console.error('FAIL '+name+': '+error);
    try{
      if(await c.evaluate(`Boolean(document.querySelector('${leaveDialog}'))`)){await c.click(leaveDialog+' .xy-ui-modal__footer > button:first-child');await c.waitFor(`!document.querySelector('${leaveDialog}')`);}
      await closeDialog();
      if(await c.evaluate(`Boolean(document.querySelector('${taskSwitcher}'))`)){await navigationBusy(false);await returnHome({allowDiscard:true});}
    }catch(cleanupError){reports.at(-1).cleanupError=String(cleanupError);}
  }
  finally{await writeFile(join(output,'results.json'),JSON.stringify(reports,null,2));}
}
let fixtureId;
try{
  ({identifier:fixtureId}=await c.call('Page.addScriptToEvaluateOnNewDocument',{source}));
  await c.navigate(base+'/');
  await run('overview-defaults',async()=>{
    await reset();assert.equal((await categoryLabels()).length,8);
    assert(!(await text()).includes('98.5%'));await disclose('供应商状态');assert((await text()).includes('98.5%'));assert(!(await text()).includes('9850'));
    for(const command of ['run_synthetic_check','save_synthetic_monitor','create_access_token','apply_explicit_mount','apply_config_rollback'])assert.equal((await calls(command)).length,0,command);
    await noOverflow();await c.screenshot(join(output,'desktop-overview.png'));
  });
  await run('access-confirmation-and-token-lifetime',async()=>{
    await reset();await openTask('access');await disabled('保存访问策略');await check('我已检查策略，确认应用此变更');await disabled('保存访问策略',false);
    await visibleSection('.gov-group','请求速率与并发');await fill('每分钟请求上限',60);await disabled('保存访问策略');await check('我已检查策略，确认应用此变更');await click('保存访问策略');
    await c.waitFor("document.body.innerText.includes('访问策略已保存')");assert.equal((await calls('save_access_policy'))[0].args.policy.requestsPerMinute,60);
    await navigationBusy(false);await openTask('tokens');await disclose('创建令牌');await fill('令牌名称','QA read-only');await c.fill(await field('权限范围'),'monitor');await fill('有效天数',30);await click('创建令牌');
    await c.waitFor('Boolean(document.querySelector(".gov-secret"))');assert((await text()).includes('xy_qa_single_display'));
    assert(!(await c.evaluate('JSON.stringify({...localStorage,...sessionStorage})')).includes('xy_qa_single_display'));
    assert.equal((await calls('create_access_token'))[0].args.scope,'monitor');
    assert.equal((await calls('create_access_token'))[0].args.days,30);assert.equal(await c.evaluate('window.__XY_QA__.governance.tokens[0].expiresAt-window.__XY_QA__.governance.tokens[0].createdAt'),30*86400);
    await navigationBusy(false);await click('我已保存，隐藏令牌');assert(!(await text()).includes('xy_qa_single_display'));
    await returnHome({discard:true});await openTask('access');assert(!(await text()).includes('xy_qa_single_display'));
    await openTask('tokens');assert(!(await text()).includes('xy_qa_single_display'));await click('撤销');await disabled('确认撤销');await confirm('REVOKE','revoke');await disabled('确认撤销');await confirm('REVOKE');await click('确认撤销');
    await c.waitFor("window.__XY_QA__.governance.tokens[0].revoked");assert.equal((await calls('revoke_access_token')).length,1);
  });
  await run('running-proxy-locks-sensitive-changes',async()=>{
    await reset({running:true});await openTask('access');await check('我已检查策略，确认应用此变更');await disabled('保存访问策略');
    await openTask('versions',{discard:true});await click('预览回滚差异');await reviewDiff('qa-version-1');await check('我已查看上述 diff');await confirm('ROLLBACK');await disabled('确认回滚配置');
    assert.equal((await calls('save_access_policy')).length,0);assert.equal((await calls('apply_config_rollback')).length,0);
  });
  await run('budget-validation-and-persistent-reservation',async()=>{
    await reset();await openTask('budget');await editBudget();await disabled('保存预算策略');await click('启用硬预算准入');await fill('每日 Token 预留上限',-1);await check('我已检查策略，确认应用此变更');await disabled('保存预算策略');
    await fill('每日 Token 预留上限',20000);await disabled('保存预算策略');await fill('单任务总尝试上限',-1);await check('我已检查策略，确认应用此变更');await disabled('保存预算策略');
    await fill('单任务总尝试上限',3);await fill('单任务费用占用上限（USD）',0.003168);await disabled('保存预算策略');await check('我已检查策略，确认应用此变更');await click('保存预算策略');
    await c.waitFor("document.body.innerText.includes('预算策略已保存')");const call=(await calls('save_hard_budget'))[0];assert.equal(call.args.policy.dailyTokens,20000);assert.equal(call.args.confirmed,true);
    assert.equal(call.args.policy.taskMaxAttempts,3);assert.equal(call.args.policy.taskMaxMicrousd,3168);
    assert.equal(await c.evaluate('window.__XY_QA__.governance.budget.reservedTokens'),4096);assert((await text()).includes('4,096'));
    await selectTask('qa-task-live');assert.equal(await taskValue('任务累计尝试 / 上限'),'2 / 3');assert.equal(await taskValue('任务累计占用 / 上限'),'US$0.002144 / US$0.003168');await noOverflow();
  });
  await run('task-zero-limits-and-no-reset-on-policy-change',async()=>{
    await reset({budget:true});await openTask('budget');await editBudget();await fill('单任务总尝试上限',0);await fill('单任务费用占用上限（USD）',0);await check('我已检查策略，确认应用此变更');await click('保存预算策略');
    await c.waitFor('window.__XY_QA__.governance.budget.policy.taskMaxAttempts===0');const saved=(await calls('save_hard_budget'))[0];assert.equal(saved.args.policy.taskMaxAttempts,0);assert.equal(saved.args.policy.taskMaxMicrousd,0);
    await selectTask('qa-task-live');assert.equal(await taskValue('任务累计尝试 / 上限'),'2 / 0');assert.equal(await taskValue('任务累计占用 / 上限'),'US$0.002144 / US$0.00');
    await closeDialog();await openTask('budget');await editBudget();await fill('单任务总尝试上限',99);await fill('单任务费用占用上限（USD）',10);await check('我已检查策略，确认应用此变更');await click('保存预算策略');
    await c.waitFor('window.__XY_QA__.calls.filter(c=>c.command==="save_hard_budget").length===2');await navigationBusy(false);await editBudget();await click('启用硬预算准入');await check('我已检查策略，确认应用此变更');await click('保存预算策略');
    await c.waitFor('window.__XY_QA__.governance.budget.policy.enabled===false');assert.deepEqual(await c.evaluate('(()=>{const t=window.__XY_QA__.governance.tasks[0].task;return [t.attempts,t.maxAttempts,t.maxMicrousd,t.reservedMicrousd,t.committedMicrousd];})()'),[2,0,0,2112,2144]);
    assert.equal((await calls('run_synthetic_check')).length,0);await noOverflow();
  });
  await run('task-details-overage-unknown-cost-and-physical-sends',async()=>{
    await reset({budget:true});await openTask('tasks');await c.waitFor('document.querySelectorAll('+JSON.stringify(taskRows)+').length===2');assert.equal(await c.evaluate('document.querySelectorAll('+JSON.stringify(taskRows)+').length'),2);assert.equal(await c.evaluate(expression('活动与需核对')+'.getAttribute("aria-pressed")'),'true');await selectTask('qa-task-live');
    assert.equal(await taskValue('任务累计尝试 / 上限'),'2 / 12');assert.equal(await taskValue('任务累计占用 / 上限'),'US$0.002144 / US$0.25');assert.equal(await taskValue('累计预留费用'),'US$0.002112');assert.equal(await taskValue('已知实际费用'),'US$0.001088');assert.equal(await taskValue('费用未结清的尝试'),'1');
    assert.equal(await c.evaluate('document.querySelectorAll(".gov-task-attempts li").length'),2);
    await expandAttempts();for(const value of ['qa-send-1','qa-send-2','qa-client-original','qa-client-review','1001','1002','等待用量结算'])assert((await taskText()).includes(value),value);
    assert(await c.evaluate(`[...document.querySelectorAll('.gov-task-attempts .gov-fact')].some(e=>e.querySelector('dt')?.textContent==='实际费用'&&e.querySelector('dd')?.innerText.trim()==='未知')`),'Unknown actual cost must be displayed');
    await noOverflow();await c.evaluate('document.querySelector(".gov-task-detail").scrollIntoView({block:"start"})');await c.screenshot(join(output,'task-details.png'));
    await c.evaluate('document.querySelector(".gov-task-attempts li:last-child").scrollIntoView({block:"start"})');await c.screenshot(join(output,'task-send-details.png'));
    await selectTask('qa-task-cancelled');await expandAttempts();assert.equal(await taskValue('任务状态'),'已取消');assert.equal(await taskValue('已知实际费用'),'US$0.00');assert((await taskText()).includes('未取得 HTTP'));assert((await taskText()).includes('任务已取消，后续重试已阻断。'));
  });
  await run('task-cancellation-confirmation-and-selection-reset',async()=>{
    await reset({budget:true});await openTask('tasks');await selectTask('qa-task-live');await disabled('取消任务');await check('确认取消此任务的全部请求与后续重试');await disabled('取消任务',false);
    await selectTask('qa-task-cancelled');assert.equal(await c.evaluate('document.querySelector(".gov-task-cancel input[type=checkbox]").checked'),false);await disabled('取消任务');
    await selectTask('qa-task-live');await disabled('取消任务');await check('确认取消此任务的全部请求与后续重试');await c.evaluate('window.__XY_QA__.hold.push("cancel_proxy_task")');await click('取消任务');await disabled('取消任务');
    await navigationBusy();await busyDialog('.gov-panel .xy-ui-drawer[role=dialog]');await assertTask('tasks');assert.equal((await calls('cancel_proxy_task')).length,1);await c.evaluate('window.__XY_QA__.release("cancel_proxy_task")');await c.waitFor('window.__XY_QA__.governance.tasks[0].task.cancelled');await c.waitFor('document.querySelector(".gov-task-cancel input[type=checkbox]")?.matches(":disabled")');await navigationBusy(false);
    assert.deepEqual((await calls('cancel_proxy_task'))[0].args,{id:'qa-task-live',confirmed:true});assert.equal(await taskValue('任务状态'),'已取消');assert.equal(await taskValue('进行中的尝试'),'0');assert.equal(await taskValue('累计预留费用'),'US$0.002112');assert.equal(await taskValue('已知实际费用'),'US$0.001088');
    assert.deepEqual(await c.evaluate('window.__XY_QA__.governance.tasks[0].attempts.map(a=>a.status)'),['completed','cancelled']);assert.equal((await calls('run_synthetic_check')).length,0);await disabled('取消任务');await noOverflow();await c.screenshot(join(output,'task-cancelled.png'));
  });
  await run('task-query-and-cancellation-error-recovery',async()=>{
    await reset({budget:true,setup:'window.__XY_QA__.fail.list_proxy_tasks="QA task list unavailable"'});await openTask('tasks');await c.waitFor('document.querySelector(".gov-panel").innerText.includes("QA task list unavailable")',20000);
    assert.equal(await c.evaluate('document.querySelectorAll('+JSON.stringify(taskRows)+').length'),0);await c.evaluate('delete window.__XY_QA__.fail.list_proxy_tasks');await c.button('重试',taskCard);await c.waitFor('document.querySelectorAll('+JSON.stringify(taskRows)+').length===2');
    await c.evaluate('window.__XY_QA__.fail.get_proxy_task="QA task detail unavailable"');await selectTask('qa-task-live',false);await c.waitFor('document.body.innerText.includes("QA task detail unavailable")',20000);assert.equal(await c.evaluate('Boolean(document.querySelector(".gov-task-detail"))'),false);
    await c.evaluate('delete window.__XY_QA__.fail.get_proxy_task');await c.button('重试',page+'.querySelector("[role=dialog]")');await waitTask('qa-task-live');await c.evaluate('window.__XY_QA__.fail.cancel_proxy_task="QA task cancellation unavailable"');await check('确认取消此任务的全部请求与后续重试');await click('取消任务');
    await c.waitFor('document.body.innerText.includes("QA task cancellation unavailable")');assert.equal(await c.evaluate('window.__XY_QA__.governance.tasks[0].task.cancelled'),false);assert.equal(await taskValue('任务状态'),'执行中');assert(!(await taskText()).includes('任务已取消，后续重试已阻断。'));
    await c.evaluate('delete window.__XY_QA__.fail.cancel_proxy_task');await disabled('取消任务',false);await click('取消任务');await c.waitFor('window.__XY_QA__.governance.tasks[0].task.cancelled');await c.waitFor('document.querySelector(".gov-task-cancel input[type=checkbox]")?.matches(":disabled")');assert.equal((await calls('cancel_proxy_task')).length,2);assert.equal(await taskValue('任务状态'),'已取消');
  });
  await run('task-empty-and-zero-send-records',async()=>{
    await reset({setup:'window.__XY_QA__.governance.tasks=[]'});await openTask('tasks');await c.waitFor('document.body.innerText.includes("暂无任务记录")');assert.equal(await c.evaluate('Boolean(document.querySelector(".gov-task-detail"))'),false);
    await reset({setup:'(()=>{const g=window.__XY_QA__.governance;g.tasks=[{task:{...g.tasks[0].task,id:"qa-task-zero",attempts:0,reservedMicrousd:0,committedMicrousd:0,knownMicrousd:0,unknownCostAttempts:0,activeAttempts:0},attempts:[]}];})()'});await openTask('tasks');await c.waitFor('document.querySelector(".gov-panel").innerText.includes("最近记录中没有活动或需核对的任务")');assert.equal(await c.evaluate('document.querySelectorAll('+JSON.stringify(taskRows)+').length'),0);await click('全部最近任务');assert.equal(await c.evaluate(expression('全部最近任务')+'.getAttribute("aria-pressed")'),'true');await selectTask('qa-task-zero');
    assert((await taskText()).includes('此任务尚未发送上游请求。'));assert.equal(await c.evaluate('document.querySelectorAll(".gov-task-attempts li").length'),0);assert.equal(await taskValue('任务累计尝试 / 上限'),'0 / 12');
  });
  await run('task-english-dark-responsive-details',async()=>{
    await reset({width:390,language:'en',theme:'dark',budget:true});await openTask('tasks');await selectTask('qa-task-live');await expandAttempts();assert.doesNotMatch(await taskText(),/\p{Script=Han}/u);
    const values=await c.evaluate(`[...document.querySelectorAll('.gov-task-detail .gov-fact > dd')].map(value=>{const bounds=value.getBoundingClientRect(),label=value.previousElementSibling.getBoundingClientRect(),list=value.parentElement.getBoundingClientRect();return {text:value.textContent,visible:value.getClientRects().length>0&&!value.closest("details:not([open])"),width:bounds.width,available:list.width,belowLabel:bounds.top>=label.bottom};})`);
    assert(values.length>10);for(const value of values){assert(value.visible,'Task detail value must be visible: '+value.text);assert(value.belowLabel,'Value should follow its label on narrow windows: '+value.text);assert(value.width>=value.available-1,'Value should use the available detail width: '+value.text);}
    assert.equal(await taskValue('Task committed cost / limit'),'$0.002144 / $0.25');assert((await taskText()).includes('Client request ID'));await noOverflow();await c.evaluate('document.querySelector(".gov-task-detail").scrollIntoView({block:"start"})');await c.screenshot(join(output,'task-english-dark-mobile.png'));
    await c.evaluate('document.querySelector(".gov-task-attempts li:last-child").scrollIntoView({block:"start"})');await c.screenshot(join(output,'task-send-english-dark-mobile.png'));
    await selectTask('qa-task-cancelled');await expandAttempts();assert.doesNotMatch(await taskText(),/\p{Script=Han}/u);await disabled('Cancel task');await noOverflow();
  });
  await run('paid-monitor-three-gates-and-single-dispatch',async()=>{
    await reset({budget:true});await openTask('monitor');await subtab('monitor','configuration');await confirm('ALLOW PAID CHECKS');await disabled('执行一次收费验收');assert.equal((await calls('run_synthetic_check')).length,0);
    await reset({running:true,discard:true});await openTask('monitor');await subtab('monitor','configuration');await confirm('ALLOW PAID CHECKS');await disabled('执行一次收费验收');assert.equal((await calls('run_synthetic_check')).length,0);
    await reset({running:true,budget:true,discard:true});await openTask('monitor');await subtab('monitor','configuration');await disabled('执行一次收费验收');await confirm('ALLOW PAID CHECKS','ALLOW');await disabled('执行一次收费验收');
    await confirm('ALLOW PAID CHECKS');await disabled('执行一次收费验收',false);await c.fill(await field('验收模式'),'tools');await disabled('执行一次收费验收');
    await confirm('ALLOW PAID CHECKS');await c.evaluate("window.__XY_QA__.hold.push('run_synthetic_check')");await click('执行一次收费验收');await disabled('执行一次收费验收');
    await navigationBusy();assert.equal(await c.evaluate('document.querySelectorAll(".gov-panel [role=tab]:disabled").length'),2);const runningTask=await taskState();
    await c.clickExpression('[...document.querySelectorAll(".xy-shell-nav-item[data-view=overview]")].find(item=>item.getClientRects().length)');await c.waitFor('Boolean(document.querySelector(".task-navigation-wait[role=status]")?.getClientRects().length)');
    assert.deepEqual(await taskState(),runningTask,'Busy work must block global navigation');assert.equal(await c.evaluate(`Boolean(document.querySelector('${leaveDialog}'))`),false,'Busy work cannot be discarded');
    assert.equal((await calls('run_synthetic_check')).length,1);await c.evaluate("window.__XY_QA__.release('run_synthetic_check')");await c.waitFor('window.__XY_QA__.governance.runs.length===1');
    await c.waitFor('document.querySelector("#monitor-panel-configuration fieldset")?.disabled===false');await navigationBusy(false);await disabled('执行一次收费验收');
    assert.equal(await c.evaluate('document.querySelector('+JSON.stringify(await field('输入确认文字 · ALLOW PAID CHECKS'))+').value'),'');
    assert.deepEqual((await calls('run_synthetic_check'))[0].args,{policy:{enabled:false,providerId:'qa-p1',model:'qa-alpha',mode:'tools',intervalMinutes:30,maxRunsPerDay:10},confirmation:'ALLOW PAID CHECKS'});await subtab('monitor','receipts',{discard:true});await c.waitFor('document.querySelectorAll(".gov-receipts li").length===1');
    await disclose('查看验收证据');assert.deepEqual(await c.evaluate('[...document.querySelectorAll(".gov-receipts .gov-receipt__facts > div")].map(e=>[e.querySelector("dt").innerText,e.querySelector("dd").innerText])'),[['返回模型','qa-alpha'],['HTTP','200'],['Token','4 / 1']]);
    assert.equal((await calls('save_synthetic_monitor')).length,0);await c.screenshot(join(output,'monitor-receipt.png'));
  });
  await run('version-preview-stale-error-and-exact-hashes',async()=>{
    await reset();await openTask('versions');await click('预览回滚差异');await reviewDiff('qa-version-1');assert.equal((await calls('apply_config_rollback')).length,0);
    await disabled('确认回滚配置');await confirm('ROLLBACK');await disabled('确认回滚配置');await check('我已查看上述 diff');await disabled('确认回滚配置',false);
    assert.equal(await c.evaluate('document.querySelectorAll(".gov-diff-lines tbody tr[data-kind=remove]").length'),1);
    assert.deepEqual(await c.evaluate('[...document.querySelectorAll(".gov-diff__counts > div")].map(e=>[e.dataset.kind,e.querySelector("dd").textContent])'),[['add','+1'],['remove','−1']]);
    assert.deepEqual(await c.evaluate('[...document.querySelectorAll(".gov-diff-lines tbody tr[data-kind=remove],.gov-diff-lines tbody tr[data-kind=add]")].map(e=>[e.dataset.kind,e.cells[0].textContent,e.cells[1].textContent,e.querySelector(".gov-diff__sign [aria-hidden]").textContent,e.querySelector("pre").textContent])'),[['remove','2','','−','  "model": "new"'],['add','','2','+','  "model": "old"']]);
    await confirm('ROLLBACK','rollback');await disabled('确认回滚配置');await confirm('ROLLBACK');await disabled('确认回滚配置',false);
    await c.screenshot(join(output,'version-rollback-diff.png'));await disclose('完整修改前内容');await disclose('完整修改后内容');
    assert.deepEqual(await c.evaluate('[...document.querySelectorAll(".gov-diff__complete details[open] pre")].map(e=>e.textContent)'),['{\n  "model": "new"\n}','{\n  "model": "old"\n}']);
    await c.screenshot(join(output,'version-rollback-complete.png'));await disclose('完整修改前内容',false);await disclose('完整修改后内容',false);
    await c.evaluate("window.__XY_QA__.fail.apply_config_rollback='QA stale configuration: preview again'");await click('确认回滚配置');
    await c.waitFor("document.body.innerText.includes('QA stale configuration')");assert(!(await text()).includes('已回滚配置，并创建了回滚前备份'));
    await c.screenshot(join(output,'version-rollback-stale.png'));
    await c.evaluate('delete window.__XY_QA__.fail.apply_config_rollback');await click('确认回滚配置');await c.waitFor("document.body.innerText.includes('已回滚配置，并创建了回滚前备份')");
    const args=(await calls('apply_config_rollback'))[1].args;assert.deepEqual(args,{id:'qa-version-1',expectedCurrentHash:'qa-current-hash',targetHash:'qa-target-hash',confirmation:'ROLLBACK'});
  });
  await run('explicit-mount-preview-expiry-and-atomic-confirmation',async()=>{
    await reset();await openTask('mounts');assert.equal(await c.evaluate(`Boolean(${expression('只预览，不写入')})`),false);assert.equal(await c.evaluate('Boolean(document.querySelector(".gov-source-picker"))'),false);assert.equal((await calls('preview_explicit_mount')).length,0);assert.equal((await calls('apply_explicit_mount')).length,0);await mountPreview();
    assert.equal((await calls('apply_explicit_mount')).length,0);const args=(await calls('preview_explicit_mount'))[0].args;
    assert.deepEqual(args,{rootId:'qa-root-handle',client:'codex',promptIds:['qa-prompt'],skillIds:[]});
    assert((await text()).includes('Keep this block.'));await disabled('备份并应用文件变更');await confirm('WRITE FILE');await disabled('备份并应用文件变更');await check('我已查看上述 diff');await disabled('备份并应用文件变更',false);
    await c.screenshot(join(output,'explicit-mount-diff.png'));
    await c.evaluate('window.__qaNow=Date.now;Date.now=()=>window.__qaNow()+301000');await c.waitFor("document.body.innerText.includes('预览已过期，请重新生成')");await disabled('备份并应用文件变更');
    assert.equal((await calls('apply_explicit_mount')).length,0);await c.screenshot(join(output,'explicit-mount-expired.png'));
    await c.evaluate('Date.now=window.__qaNow;delete window.__qaNow');
    // Let the component clock observe restored time before generating a new preview.
    await c.waitFor("!document.body.innerText.includes('预览已过期，请重新生成')");
    await click('取消');await c.waitFor('!document.querySelector(".governance-page [role=dialog]")');await c.fill(await field('挂载目标'),'gemini');assert.equal(await c.evaluate('Boolean(document.querySelector(".gov-diff"))'),false);
    assert.equal(await c.evaluate('Boolean(document.querySelector(".gov-source-picker"))'),false);await click('继续选择来源');await visibleSection('.gov-source-picker','选择挂载来源');await click('只预览，不写入');await reviewDiff('QA:/selected/project/GEMINI.md');await disabled('备份并应用文件变更');assert((await text()).includes('GEMINI.md'));
    await check('我已查看上述 diff');await confirm('WRITE FILE');await c.evaluate("window.__XY_QA__.hold.push('apply_explicit_mount')");await click('备份并应用文件变更');await disabled('备份并应用文件变更');await disabled('选择项目目录');
    await navigationBusy();assert.equal(await c.evaluate('document.querySelectorAll(".gov-panel [role=tab]:disabled").length'),2);await busyDialog('.gov-review-modal');await assertTask('mounts');
    assert.equal((await calls('apply_explicit_mount')).length,1);assert.deepEqual((await calls('apply_explicit_mount'))[0].args,{id:'qa-preview-2',confirmation:'WRITE FILE'});await c.evaluate("window.__XY_QA__.release('apply_explicit_mount')");await c.waitFor("document.body.innerText.includes('文件已按预览更新，原文备份已保留')");
    assert.equal(await c.evaluate('window.__XY_QA__.governance.backups.length'),1);assert.equal(await c.evaluate('Boolean(document.querySelector(".gov-diff"))'),false);await navigationBusy(false);await returnHome({discard:true});
  });
  await run('backup-preview-and-explicit-deletion',async()=>{
    await reset({setup:"window.__XY_QA__.governance.backups=[{id:'qa-existing-backup',target:'QA:/selected/project/AGENTS.md',createdAt:1790000000}]"});await openTask('mounts');await subtab('mounts','backups');await disabled('预览文件回滚');
    await click('选择项目目录');await navigationBusy(false);await click('预览文件回滚');await navigationBusy(false);await disabled('备份并应用文件变更');assert.equal((await calls('apply_explicit_mount')).length,0);await click('取消');
    await click('删除备份');await disabled('永久删除此备份');await confirm('DELETE BACKUP');await click('永久删除此备份');await c.waitFor('window.__XY_QA__.governance.backups.length===0');
    assert.equal((await calls('apply_explicit_mount')).length,0);await navigationBusy(false);await openTask('versions',{discard:true});await click('管理版本');assert.equal(await c.evaluate(expression('管理版本')+'.getAttribute("aria-pressed")'),'true');await click('删除版本');await disabled('永久删除此版本');await confirm('DELETE VERSION');await click('永久删除此版本');await c.waitFor('window.__XY_QA__.governance.versions.length===0');
    assert.deepEqual((await calls('delete_mount_backup')).map(call=>call.args),[{id:'qa-existing-backup',confirmation:'DELETE BACKUP'}]);assert.deepEqual((await calls('delete_config_version')).map(call=>call.args),[{id:'qa-version-1',confirmation:'DELETE VERSION'}]);await navigationBusy(false);await returnHome({allowDiscard:true});
  });
  await run('mobile-native-is-read-only',async()=>{
    await reset({width:390,mobile:true,running:true,budget:true});assert.equal((await categoryLabels()).length,2);
    assert((await text()).includes('仅显示本机数据'));assert((await text()).includes('尚未连接其他设备'));
    await noOverflow();await c.screenshot(join(output,'mobile-read-only.png'));await openTask('monitor');assert.equal(await c.evaluate(`${page}.querySelector('.gov-panel').querySelectorAll('input,select').length`),0);assert.equal(await c.evaluate('document.querySelectorAll(".gov-panel form,.gov-panel [role=tab]").length'),0);
    assert(!(await text()).includes('执行一次收费验收'));await openTask('audit');await noOverflow();
    assert.equal((await calls('run_synthetic_check')).length,0);assert.equal((await calls('choose_mount_root')).length,0);assert.deepEqual(await writes(),[]);
  });
  await run('english-dark-responsive-all-tabs',async()=>{
    await reset({width:390,language:'en',theme:'dark'});
    assert.deepEqual(await categoryLabels(),['Access policy','Local access tokens','Hard budget','Recent tasks','Acceptance monitoring','Config versions','Explicit mounts','Governance audit']);
    for(const name of taskNames){await openTask(name);await noOverflow();await returnHome();}
    for(const name of taskNames){await openTask(name);await noOverflow();}
    await openTask('mounts');assert((await text()).includes('Choose project directory'));await c.screenshot(join(output,'english-dark-mounts.png'));
  });
  await run('browser-preview-never-pretends-to-save',async()=>{
    await c.call('Page.removeScriptToEvaluateOnNewDocument',{identifier:fixtureId});fixtureId=null;
    await reset();assert((await text()).includes('浏览器中仅展示预览'));assert.equal(await c.evaluate('"__TAURI_INTERNALS__" in window'),false);
    await openTask('budget');await editBudget();await check('我已检查策略，确认应用此变更');await click('保存预算策略');
    await c.waitFor("document.body.innerText.includes('浏览器预览不能保存治理设置')");assert(!(await text()).includes('预算策略已保存，历史预留保持不变'));
    await navigationBusy(false);await openTask('tasks',{discard:true});await c.waitFor('document.querySelector(".gov-panel").innerText.includes("暂无任务记录")');assert((await text()).includes('暂无任务记录'));assert.equal(await c.evaluate('document.querySelectorAll('+JSON.stringify(taskRows)+').length'),0);
    await openTask('monitor');await subtab('monitor','configuration');await confirm('ALLOW PAID CHECKS');await disabled('执行一次收费验收');await noOverflow();await returnHome({discard:true});
  });
}finally{
  try{await writeFile(join(output,'network.json'),JSON.stringify({appUrl:base,requests:c.networkRequests,webSocketUrls:c.webSocketUrls},null,2));}
  finally{await c.close();}
}
console.log(`${reports.filter(r=>r.passed).length}/${reports.length} governance browser checks passed`);
if(reports.length===0||reports.some(r=>!r.passed))process.exitCode=1;
