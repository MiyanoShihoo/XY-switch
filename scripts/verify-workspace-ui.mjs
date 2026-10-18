import assert from 'node:assert/strict';
import { readFile, mkdir, writeFile } from 'node:fs/promises';
import { resolve, join } from 'node:path';
import { connect } from './workspace-qa/cdp.mjs';

const base = process.env.APP_URL ?? 'http://127.0.0.1:1431';
const output = resolve(process.env.QA_OUTPUT ?? 'artifacts/workspace-qa');
await mkdir(output,{recursive:true});
const c = await connect(undefined,{fresh:true});
const reports = [];
const only = process.env.QA_ONLY?.split(',');
let fixtureId;
const pause = ms=>new Promise(r=>setTimeout(r,ms));
const body = ()=>c.evaluate('document.body.innerText');
const nav = label=>c.click('.xy-shell-nav-item[title='+JSON.stringify(label)+']');
const field = async label=>{ const id=await c.evaluate('[...document.querySelectorAll("label")].find(e=>e.textContent.replace(/[*]/g, "").trim() === '+JSON.stringify(label)+')?.htmlFor');assert(id,'field '+label);return '[id='+JSON.stringify(id)+']'; };
const fillField = async(label,value)=>c.fill(await field(label),value);
const contains = async value=>assert((await body()).includes(value),'visible text '+value);
const absent = async value=>assert(!(await body()).includes(value),'unexpected text '+value);
const countCalls = command=>c.evaluate('window.__XY_QA__.calls.filter(c=>c.command === '+JSON.stringify(command)+').length');
const fixture = script=>c.evaluate('(()=>{const q=window.__XY_QA__;'+script+'})()');
async function reset() { await c.viewport(1180,760); await c.navigate(base+'/');await c.waitFor('document.body.innerText.includes("Orion")'); }
async function noOverflow() { const metrics=await c.evaluate('({width:innerWidth,root:document.documentElement.scrollWidth,body:document.body.scrollWidth})');assert(metrics.root<=metrics.width && metrics.body<=metrics.width,JSON.stringify(metrics)); }
async function run(name,work) { if(only && !only.includes(name))return;console.log('RUN '+name);const exceptionStart=c.exceptions.length;try{await work();if(name!=='error-boundary')assert.equal(c.exceptions.length,exceptionStart,'unexpected runtime exception');reports.push({name,passed:true});console.log('PASS '+name);}catch(error){reports.push({name,passed:false,error:String(error)});await c.screenshot(join(output,name+'-failure.png'));await writeFile(join(output,name+'-failure.json'),JSON.stringify({snapshot:await c.snapshot(),exceptions:c.exceptions},null,2));throw error;}finally{await writeFile(join(output,'results.json'),JSON.stringify(reports,null,2));} }
try {
  ({identifier:fixtureId}=await c.call('Page.addScriptToEvaluateOnNewDocument',{source:await readFile(new URL('./workspace-qa/fixture.js',import.meta.url),'utf8')}));
  await run('layout',async()=>{
    await reset();
    const pages=['供应商','MCP 服务器','提示词','Skills','会话','用量与审计','设置'];
    for(const [width,height] of [[1180,760],[820,600]]) {
      await c.viewport(width,height);
      for(const theme of ['light','dark']) {
        await c.evaluate('localStorage.setItem("xy-switch:theme",'+JSON.stringify(theme)+')');
        await c.navigate(base+'/'); await c.waitFor('document.body.innerText.includes("Orion")');
        for(const [i,label] of pages.entries()) {
          await nav(label);await pause(130);await noOverflow();
          assert.equal(await c.evaluate('document.querySelector(".xy-shell-nav-item[aria-current=page]")?.title'),label);
          await c.screenshot(join(output,'layout-'+width+'-'+theme+'-'+i+'.png'));
        }
      }
    }
    await c.viewport(1280,800);await nav('用量与审计');await noOverflow();
    assert.equal(c.exceptions.length,0,'unhandled runtime exceptions');
  });
  await run('providers',async()=>{
    await reset();
    const current=await c.evaluate('window.__XY_QA__.current');
    await c.button('查看 Relay 详情');
    assert.equal(await c.evaluate('window.__XY_QA__.current'),current,'inspection must not activate');
    await c.button('编辑配置');await c.waitFor('Boolean(document.querySelector("[role=dialog] input"))');
    assert.equal(await c.evaluate('document.querySelector("[role=dialog]").contains(document.activeElement)'),true);
    await fillField('供应商名称','Relay Edited');
    await c.key('Escape');
    assert.equal(await c.evaluate('Boolean(document.querySelector("[role=dialog]"))'),false);
    assert.equal(await c.evaluate('document.activeElement.textContent.trim()'),'编辑配置');
    await c.button('编辑配置');await c.waitFor('Boolean(document.querySelector("[role=dialog] input"))');
    await fillField('供应商名称','Relay Edited');
    await c.button('保存配置');
    await c.waitFor('!document.querySelector("[role=dialog]")');
    assert.equal(await c.evaluate('window.__XY_QA__.current'),current,'save must not activate');
    assert.equal(await countCalls('set_provider_api_key'),0,'empty key must preserve existing credential');
    await c.button('删除供应商');
    assert.equal(await c.evaluate('document.activeElement.textContent.trim()'),'取消');
    await c.key('Tab',8);assert.equal(await c.evaluate('document.activeElement.textContent.trim()'),'确认删除');
    await c.key('Tab');assert.equal(await c.evaluate('document.activeElement.textContent.trim()'),'取消');
    await c.key('Escape');assert.equal(await countCalls('delete_provider'),0);
    await fixture('q.fail.delete_provider="QA delete denied";q.hold.push("delete_provider");');
    await c.button('删除供应商');await c.button('确认删除');
    await c.key('Escape');assert.equal(await c.evaluate('Boolean(document.querySelector("[role=alertdialog]"))'),true,'busy Escape blocked');
    assert.equal(await c.evaluate('[...document.querySelectorAll("[role=alertdialog] button")].every(b=>b.disabled)'),true);
    await fixture('q.release("delete_provider");');await c.waitFor('document.body.innerText.includes("QA delete denied")');
    assert.equal(await c.evaluate('Boolean(document.querySelector("[role=alertdialog]"))'),true);
    assert.equal(await c.evaluate('window.__XY_QA__.providers.length'),3);
    await c.button('取消');await c.viewport(820,600);await c.button('查看 Orion 详情');
    assert.equal(await c.evaluate('Boolean(document.querySelector("[role=dialog]"))'),true,'compact inspector drawer');
    await noOverflow();await c.screenshot(join(output,'provider-drawer.png'));await c.key('Escape');
    assert.equal(await c.evaluate('document.activeElement.getAttribute("aria-label")'),'查看 Orion 详情');
  });
  await run('prompts',async()=>{
    await reset();await nav('提示词');await c.waitFor('document.body.innerText.includes("代码审查")');
    await c.button('删除 代码审查');await c.button('取消');assert.equal(await countCalls('delete_prompt'),0);
    await fixture('q.fail.delete_prompt="QA prompt delete failed";');
    await c.button('删除 代码审查');await c.button('确认删除');await c.waitFor('document.body.innerText.includes("QA prompt delete failed")');
    assert.equal(await c.evaluate('Boolean(document.querySelector("[role=alertdialog]"))'),true);
    await fixture('delete q.fail.delete_prompt;');await c.button('确认删除');await c.waitFor('!document.querySelector("[role=alertdialog]")');
    await contains('尚未添加提示词');
  });
  await run('mcp',async()=>{
    await reset();await nav('MCP 服务器');await c.waitFor('document.body.innerText.includes("Memory QA")');
    const mounts=await c.evaluate('[...document.querySelectorAll(".mcp-record [role=switch]")].map(e=>e.getAttribute("aria-label"))');assert.equal(mounts.length,4);for(const app of ['Claude','Codex','Gemini','OpenCode'])assert(mounts.some(label=>label.includes(app))); 
    await c.button('编辑 Memory QA');await fillField('名称','Memory Edited');await c.button('保存配置');await c.waitFor('!document.querySelector("[role=dialog]")');
    const record=await c.evaluate('window.__XY_QA__.mcp[0]');
    assert.deepEqual(record.env,{KEEP:'value'});assert.deepEqual(record.headers,{'X-QA':'preserve'});assert.equal(record.secretsConfigured,true);assert.equal(record.builtIn,false);
    await fixture('q.fail.delete_mcp_server="QA MCP delete failed";');
    await c.button('删除 Memory Edited');await c.button('确认删除');await c.waitFor('document.body.innerText.includes("QA MCP delete failed")');
    assert.equal(await c.evaluate('Boolean(document.querySelector("[role=alertdialog]"))'),true);await c.button('取消');
    const savesBefore=await countCalls('save_mcp_server');await c.button('添加 MCP');await contains('启动命令');await contains('启动参数（JSON 数组）');
    await fillField('名称','HTTP QA');await fillField('传输方式','http');await fillField('服务 URL','not-an-url');await c.button('保存配置');
    assert.equal(await countCalls('save_mcp_server'),savesBefore,'invalid URL must not reach backend');assert.equal(await c.evaluate('Boolean(document.querySelector("[role=dialog]"))'),true);
    await fillField('服务 URL','https://qa.invalid/mcp');await c.click('[role=dialog] details > summary');await fillField('环境变量','PATH_HINT=C:/My Folder\nEQUAL=a=b');await fillField('HTTP Headers','X-QA=preserve');await c.button('保存配置');await c.waitFor('!document.querySelector("[role=dialog]")');
    const added=await c.evaluate('window.__XY_QA__.mcp.find(s=>s.name==="HTTP QA")');assert.equal(added.transport,'http');assert.deepEqual(added.env,{PATH_HINT:'C:/My Folder',EQUAL:'a=b'});assert.deepEqual(added.headers,{'X-QA':'preserve'});assert.equal(added.command,null);assert.deepEqual(added.args,[]);
  });
  await run('skills',async()=>{
    await reset();await nav('Skills');await c.waitFor('document.body.innerText.includes("Review Skill")');
    await fixture('q.emit("tauri://drag-enter",{paths:["QA:/skills/new"],position:{x:100,y:100}});');
    await contains('松开鼠标导入第一个技能文件夹');
    await fixture('q.emit("tauri://drag-drop",{paths:["QA:/skills/new"],position:{x:100,y:100}});');
    await c.waitFor('window.__XY_QA__.calls.some(c=>c.command === "import_skill_folder")');
    assert.equal(await c.evaluate('window.__XY_QA__.calls.find(c=>c.command==="import_skill_folder").args.path'),'QA:/skills/new');
    await c.click('details.skill-record summary');
    await fixture('q.fail.uninstall_skill_unified="QA uninstall failed";');
    await c.button('卸载');await c.button('确认卸载');await c.waitFor('document.body.innerText.includes("QA uninstall failed")');
    assert.equal(await c.evaluate('Boolean(document.querySelector("[role=alertdialog]"))'),true);
    assert.equal(await c.evaluate('window.__XY_QA__.skills.length'),1);await c.button('取消');
  });
  await run('sessions',async()=>{
    await reset();await nav('会话');await c.waitFor('document.body.innerText.includes("请检查界面边界")');
    await fixture('q.deleteCount=0;');await c.button('删除会话');await c.button('确认删除');
    await c.waitFor('document.body.innerText.includes("后端未删除任何会话")');
    assert.equal(await c.evaluate('Boolean(document.querySelector("[role=alertdialog]"))'),true);await c.button('取消');
    await c.viewport(820,600);await c.button('查看会话 工作台界面验收');
    await contains('请检查界面边界');await noOverflow();await c.key('Escape');
  });
  await run('usage',async()=>{
    await reset();await nav('用量与审计');await c.waitFor('document.querySelectorAll("#usage-data-panel-logs tbody tr").length===20');
    await contains('$0.0000');await c.fill('select[aria-label="自动刷新间隔"]','0');
    await c.button('下一页');await c.waitFor('window.__XY_QA__.calls.some(c=>c.command === "get_usage_logs" && c.args.offset===20)');
    await c.fill('input[aria-label="跳转页码"]','3');await c.button('跳转');await c.waitFor('document.querySelectorAll("#usage-data-panel-logs tbody tr").length===5');
    await c.button('查看请求 qa-request-40');await contains('请求详情');await c.key('Escape');
    await c.button('供应商统计');await contains('98.5%');await absent('9850');
    await c.button('请求日志');await fillField('模型 ID','empty');await c.button('应用筛选');await c.waitFor('document.body.innerText.includes("当前筛选范围没有请求")');
    await fillField('模型 ID','slow');await c.button('应用筛选');await c.waitFor('window.__XY_QA__.pending.length===5');
    await fillField('模型 ID','fast');await c.button('应用筛选');await c.waitFor('document.querySelectorAll("#usage-data-panel-logs tbody tr").length===1');
    await fixture('q.release();');await pause(200);assert.equal(await c.evaluate('document.querySelectorAll("#usage-data-panel-logs tbody tr").length'),1,'late old filter ignored');await contains('fast');
    await fixture('q.fail.get_usage_logs="QA usage refresh failed";');await c.button('刷新');
    await c.waitFor('document.body.innerText.includes("QA usage refresh failed")');
    assert.equal(await c.evaluate('document.querySelectorAll("#usage-data-panel-logs tbody tr").length'),1,'retain previous same-filter data');
    await fillField('模型 ID','new-failure');await c.button('应用筛选');await c.waitFor('document.body.innerText.includes("QA usage refresh failed")');
    assert.equal(await c.evaluate('document.querySelectorAll("#usage-data-panel-logs tbody tr").length'),0,'new-filter error must not show unrelated cached data');
    await c.screenshot(join(output,'usage-error.png'));
  });
  await run('settings',async()=>{
    await reset();await fixture('q.fail.get_autostart_enabled="QA registry unavailable";');await nav('设置');
    await c.waitFor('document.body.innerText.includes("状态未知")');
    assert.equal(await c.evaluate('Boolean(document.querySelector("[aria-label=开机自启]"))'),false,'unknown is not an off switch');
    await fixture('delete q.fail.get_autostart_enabled;q.autostartMismatch=true;');await c.button('重试');await c.waitFor('Boolean(document.querySelector("[aria-label=开机自启]"))');
    await c.click('[aria-label="开机自启"]');await c.waitFor('document.body.innerText.includes("回读状态与请求不一致")');
    assert.equal(await c.evaluate('document.querySelector("[aria-label=开机自启]").getAttribute("aria-checked")'),'false');
    await fillField('请求超时（秒）','14');
    assert.equal(await c.evaluate('[...document.querySelectorAll("button")].find(b=>b.textContent.trim()==="保存偏好").disabled'),true);
    await fillField('请求超时（秒）','60');await c.button('保存偏好');await c.waitFor('document.body.innerText.includes("偏好已保存")');
    assert.equal(await c.evaluate('window.__XY_QA__.preferences.requestTimeoutSecs'),60);
    await fillField('颜色主题','light');assert.equal(await c.evaluate('document.documentElement.dataset.theme'),'light');
    await c.click('#settings-tab-general');await c.key('ArrowRight');assert.equal(await c.evaluate('document.activeElement.id'),'settings-tab-routing');
    await c.button('凭据');await c.waitFor('document.body.innerText.includes("已配置")');
    await fixture('q.fail["has_provider_api_key:qa-p2"]="QA credential status failed";');await c.button('刷新状态');
    await c.waitFor('document.body.innerText.includes("QA credential status failed")');await contains('状态未知');
    await c.button('数据');await fixture('q.importedRefreshFailure=true;');
    await c.evaluate('(()=>{const input=document.querySelector("input[type=file]");const transfer=new DataTransfer();transfer.items.add(new File(["{}"],"qa-config.json",{type:"application/json"}));input.files=transfer.files;input.dispatchEvent(new Event("change",{bubbles:true}));})()');
    await c.button('确认导入');await c.waitFor('document.body.innerText.includes("导入已完成，但界面刷新失败")');
    assert.equal(await c.evaluate('Boolean(document.querySelector("[role=alertdialog]"))'),false,'successful import cannot be retried accidentally');assert.equal(await countCalls('import_config'),1);
    await fixture('delete q.fail.list_providers;delete q.fail["has_provider_api_key:qa-p2"];');await c.button('重新刷新工作台');await c.waitFor('document.body.innerText.includes("工作台数据已刷新")');assert.equal(await countCalls('import_config'),1);
    await c.button('关于');await contains('v1.0.0-qa');await contains('QA:/data');await noOverflow();
  });
  await run('routing',async()=>{
    await reset();await nav('设置');await c.button('本地路由');await c.waitFor('document.body.innerText.includes("保存路由配置")');
    await fillField('监听端口','0');assert.equal(await c.evaluate('[...document.querySelectorAll("button")].find(b=>b.textContent.trim()==="保存路由配置").disabled'),true);
    await fillField('监听端口','15722');await c.button('保存路由配置');await c.waitFor('window.__XY_QA__.proxy.settings.port===15722');
    await fixture('q.hold.push("set_proxy_enabled");');await c.button('启动路由');
    await c.waitFor('window.__XY_QA__.pending.some(p=>p.command==="set_proxy_enabled")');
    assert.equal(await c.evaluate('document.querySelector(".xy-shell-runtime button").disabled'),true,'global runtime toggle is serialized');
    await fixture('q.release("set_proxy_enabled");');await c.waitFor('window.__XY_QA__.proxy.running');await contains('运行中');
    await fixture('q.fail.set_proxy_enabled="QA stop failed";q.fail.get_proxy_status="QA status unavailable";');await c.button('停止路由');
    await c.waitFor('document.body.innerText.includes("状态未知")');await contains('QA stop failed');
  });
  await run('audit',async()=>{
    await c.viewport(1180,760);await c.navigate(base+'/scripts/audit-qa/?handoff=1');await c.waitFor('document.querySelectorAll("#usage-data-panel-logs tbody tr").length===3');
    await c.fill('select[aria-label="自动刷新间隔"]','0');await contains('503');
    await c.button('查看请求 0');await c.key('Tab');
    assert.equal(await c.evaluate('document.activeElement.matches(".model-audit-details > summary")'),true,'summary must be keyboard reachable in a dialog');
    await c.key('Enter');await c.waitFor('document.querySelector(".model-audit-details").open');await c.button('分析此响应流式统计');
    const billing=await c.evaluate('Object.fromEntries([...document.querySelectorAll(".audit-stream-billing dl > div")].map(e=>[e.querySelector("dt").textContent,e.querySelector("dd").textContent]))');
    assert.equal(billing['上报可见 token / UTF-8 字节'],'6 / 36');assert.equal(billing['token / UTF-8 字节'],'0.167');assert.equal(billing['UTF-8 字节 / 帧'],'3.000');assert.equal(billing['帧 / token'],'2.000');
    await contains('该比较不是硬证据');await c.screenshot(join(output,'audit-stream-bytes.png'));await c.key('Escape');
    await c.button('查看请求 1');await contains('1 项硬证据');await c.click('.model-audit-details > summary');await contains('inv.byte_bound');await contains('inv.model_mismatch');await contains('cheap-model-v0');await c.key('Escape');
    await c.button('查看请求 2');await contains('未自报 / 未记录');await c.click('.model-audit-details > summary');await contains('model.upstream_missing');await contains('byte_bound.reasoning_unknown');await c.key('Escape');
    await c.button('模型审计');
    assert.deepEqual(await c.evaluate('[...document.querySelectorAll(".audit-counts strong")].map(e=>e.textContent)'),['3','1','1','0','3']);
    await c.button('分析当前筛选范围');await c.waitFor('Boolean(document.querySelector(".audit-passive-report"))');
    const comparisons=await c.evaluate('[...document.querySelectorAll(".audit-billing-comparison")].map(e=>e.innerText)');assert.equal(comparisons.length,2);assert(comparisons[0].includes('2.00×'));assert(comparisons[1].includes('不可比'));
    await c.click('.audit-passive-report .audit-group-details > summary');assert.equal(await c.evaluate('document.querySelectorAll(".audit-passive-report .audit-group-details tbody tr").length'),3);
    await c.screenshot(join(output,'audit-passive.png'));await noOverflow();
    await c.click('.audit-shadow-section > summary');await c.waitFor('document.body.innerText.includes("尚未配置，影子采样默认关闭")');
    await c.button('分析已采集差分');await c.waitFor('document.body.innerText.includes("没有可分析的配对分组")');
    await fillField('渠道 ID（精确匹配）','fixture');await fillField('实际转发模型（精确匹配）','premium-model');await fillField('本地分词器绝对路径','C:/qa/tokenizer.json');await c.click('[aria-label="启用影子采样"]');await c.button('保存配置');
    await c.waitFor('window.__AUDIT_QA__.settings[0]?.enabled===true');await contains('已保存启用配置');
    for(const [scenario,label] of [['consistent','当前样本内未见稳定偏离'],['suspicious','存在统计偏离，需核查'],['insufficient','未测到：尚不满足稳定回归条件']]) {
      await c.evaluate('window.__AUDIT_QA__.setScenario('+JSON.stringify(scenario)+')');await c.button('分析已采集差分');await c.waitFor('document.querySelector(".audit-shadow-results")?.innerText.includes('+JSON.stringify(label)+')');
      await c.viewport(820,600);await noOverflow();await c.screenshot(join(output,'audit-shadow-'+scenario+'.png'));
    }
    await c.button('停用');await c.waitFor('window.__AUDIT_QA__.settings[0]?.enabled===false');
    await c.evaluate('window.__AUDIT_QA__.setScenario("suspicious");window.__AUDIT_QA__.deferAnalysis=true;');await c.button('分析已采集差分');await c.waitFor('Boolean(window.__AUDIT_QA__.pendingAnalysis)');
    await fillField('模型 ID','changed-filter');await c.button('应用筛选');await c.evaluate('window.__AUDIT_QA__.releaseAnalysis()');await pause(150);
    assert.equal(await c.evaluate('Boolean(document.querySelector(".audit-shadow-results"))'),false,'late audit result must not cross filter boundaries');
    await c.click('.audit-shadow-section > summary');await c.button('分析已采集差分');await c.waitFor('Boolean(document.querySelector(".audit-shadow-results"))');
    assert.equal(await c.evaluate('window.__AUDIT_QA__.calls.filter(c=>c.command==="analyze_shadow_audit").at(-1).args.filter.model'),'changed-filter');
    await c.clickExpression('[...document.querySelectorAll("summary")].find(e=>e.textContent.trim()==="离线参照库分析")');await contains('观测指纹 JSON 绝对路径');await c.click('.audit-tokenizer-form > summary');await contains('分词器文件绝对路径');await noOverflow();
  });
  await run('model-editor',async()=>{
    await c.viewport(1180,760);await c.navigate(base+'/?qa=provider-models');await c.button('打开模型编辑器');await c.waitFor('Boolean(document.querySelector("[role=dialog]"))');
    assert.equal(await c.evaluate('document.querySelectorAll(".model-mapping-row").length'),2);
    await fillField('上下文窗口','0');await c.button('保存测试结果');assert.equal(await c.evaluate('Boolean(document.querySelector("[role=dialog]"))'),true,'invalid context must not save');
    await fillField('上下文窗口','160000');await fillField('显示名称','Alpha Edited');await c.button('下移模型 1');
    assert.equal(await c.evaluate('document.querySelector(".model-mapping-row input").value'),'qa-beta');
    await c.button('探索模型');await c.waitFor('document.body.innerText.includes("qa-discovered-39")');
    await fillField('从探索结果添加映射','qa-discovered-39');await fillField('从探索结果添加映射','qa-discovered-39');assert.equal(await c.evaluate('document.querySelectorAll(".model-mapping-row").length'),3,'selected discoveries deduplicate');
    await c.button('导入全部 40 项');await c.button('导入全部 40 项');assert.equal(await c.evaluate('document.querySelectorAll(".model-mapping-row").length'),42,'bulk import preserves originals and deduplicates');
    for(const theme of ['light','dark']) { await c.evaluate('document.documentElement.dataset.theme='+JSON.stringify(theme));await c.viewport(820,600);await pause(220);await noOverflow();
      assert.equal(await c.evaluate('[...document.querySelectorAll(".xy-ui-modal,.xy-ui-modal__body")].every(e=>e.scrollWidth<=e.clientWidth)'),true,'model modal has no horizontal overflow');
      await c.screenshot(join(output,'model-editor-'+theme+'.png'));
    }
    await c.button('保存测试结果');await c.waitFor('Boolean(document.querySelector("[data-testid=qa-model-snapshot]"))');
    const saved=await c.evaluate('JSON.parse(document.querySelector("[data-testid=qa-model-snapshot]").textContent)');assert.equal(saved.modelMappings.length,42);assert.equal(saved.modelMappings[0].model,'qa-beta');assert.equal(saved.modelMappings[1].contextWindow,160000);assert.equal(saved.modelMappings[1].displayName,'Alpha Edited');
    assert.equal(await c.evaluate('document.activeElement.textContent.trim()'),'打开模型编辑器');
    await c.fill('[aria-label="验收客户端"]','Claude');await c.button('打开模型编辑器');await c.clickExpression('[...document.querySelectorAll("summary")].find(e=>e.textContent.trim()==="Claude 配置")');await c.click('[aria-label="禁用 Artifact 工具"]');await c.button('保存测试结果');
    assert.equal(await c.evaluate('JSON.parse(document.querySelector("[data-testid=qa-model-snapshot]").textContent).claude.disableArtifactTool'),true);
  });
  await run('preview',async()=>{
    await c.call('Page.removeScriptToEvaluateOnNewDocument',{identifier:fixtureId});fixtureId=undefined;
    await c.navigate(base+'/');await nav('设置');await c.waitFor('document.body.innerText.includes("浏览器预览")');
    assert.equal(await c.evaluate('document.querySelector("[aria-label=开机自启]").disabled'),true);
    await fillField('颜色主题','dark');assert.equal(await c.evaluate('document.documentElement.dataset.theme'),'dark');
    await c.button('数据');assert.equal(await c.evaluate('[...document.querySelectorAll("button")].find(b=>b.textContent.trim()==="导入配置").disabled'),true);
  });
  await run('error-boundary',async()=>{
    await c.navigate(base+'/scripts/error-boundary-qa/?theme=dark');await contains('界面暂时无法显示');await contains('HANDOFF_QA_EXPECTED_RENDER_ERROR');await c.viewport(820,600);await noOverflow();await c.screenshot(join(output,'error-boundary.png'));
  });
  console.log(JSON.stringify({passed:reports.filter(r=>r.passed).length,output}));
} finally {
  if(fixtureId)await c.call('Page.removeScriptToEvaluateOnNewDocument',{identifier:fixtureId}).catch(()=>{});
  await c.close();
}
