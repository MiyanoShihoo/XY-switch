// Supplements fixture.js. Only synthetic data is used, inside an isolated CDP target.
(() => {
  const qa = window.__XY_QA__, clone = value => structuredClone(value), now = () => Math.floor(Date.now() / 1000);
  qa.platform.mobile = localStorage.getItem('xy-governance-qa:mobile') === 'true';
  if (qa.platform.mobile) Object.assign(qa.platform, {os:'android',systemTray:false,autostart:false,clientTakeover:false});
  qa.proxy.running = localStorage.getItem('xy-governance-qa:running') === 'true';
  const g = qa.governance = {
    access:{requireToken:false,requestsPerMinute:120,maxInFlight:8}, tokens:[],
    budget:{policy:{enabled:localStorage.getItem('xy-governance-qa:budget') === 'true',dailyTokens:100000,dailyMicrousd:1000000,inputReservation:4096,maxOutputTokens:512},dayUtc:Math.floor(Date.now()/86400000),reservedTokens:4096,reservedMicrousd:5000,attempts:1},
    monitor:{enabled:false,providerId:'qa-p1',model:'qa-alpha',mode:'chat',intervalMinutes:30,maxRunsPerDay:10},
    runs:[], versions:[{id:'qa-version-1',label:'Before edits',hash:'qa-target-hash',createdAt:now()}],
    backups:[], events:[], sequence:0,
    root:{id:'qa-root-handle',path:'QA:/selected/project'},
  };
  function event(kind,detail) {g.events.unshift({kind,detail,createdAt:now()});}
  function run(mode) {return {id:'qa-run-'+(++g.sequence),providerId:'qa-p1',model:'qa-alpha',mode,status:'passed',latencyMs:123,detail:'response_schema_finish_and_exact_text_verified',createdAt:now(),evidence:{httpStatus:200,reportedModel:'qa-alpha',inputTokens:4,outputTokens:1,totalTokens:5}};}
  function preview(client,rollback=false) {
    const before='# Handwritten project rules\n\nKeep this block.\n';
    const after=before+'\n<!-- XY SWITCH BEGIN -->\n# Review\nCheck errors.\n<!-- XY SWITCH END -->\n';
    return {id:'qa-preview-'+(++g.sequence),target:g.root.path+'/'+({codex:'AGENTS.md',claude:'CLAUDE.md',gemini:'GEMINI.md'}[client]||'AGENTS.md'),before:rollback?after:before,after:rollback?before:after,createsFile:false,removesFile:false,expiresAt:now()+300};
  }
  const handlers = {
    get_access_policy:()=>g.access, list_access_tokens:()=>g.tokens,
    save_access_policy:({policy,confirmed})=>{if(!confirmed)throw Error('QA confirmation missing');g.access=clone(policy);event('access_policy_changed','QA');},
    create_access_token:({label,scope,days})=>{const info={id:'qa-token-'+(++g.sequence),label,scope,createdAt:now(),expiresAt:now()+days*86400,revoked:false};g.tokens.unshift(info);return {info,token:'xy_qa_single_display'};},
    revoke_access_token:({id,confirmed})=>{if(!confirmed)throw Error('QA confirmation missing');g.tokens.find(t=>t.id===id).revoked=true;},
    get_hard_budget:()=>g.budget,
    save_hard_budget:({policy,confirmed})=>{if(!confirmed)throw Error('QA confirmation missing');g.budget.policy=clone(policy);event('budget_policy_changed','QA');},
    get_synthetic_monitor:()=>g.monitor,
    save_synthetic_monitor:({policy,confirmation})=>{if(policy.enabled&&confirmation!=='ALLOW PAID CHECKS')throw Error('QA paid confirmation missing');g.monitor=clone(policy);},
    run_synthetic_check:({policy,confirmation})=>{if(confirmation!=='ALLOW PAID CHECKS')throw Error('QA paid confirmation missing');const r=run(policy.mode);g.runs.unshift(r);return r;},
    list_synthetic_runs:()=>g.runs,
    get_governance_status:()=>({time:now(),budget:g.budget,requests:{totalRequests:10,successfulRequests:9,failedRequests:1,inputTokens:100,outputTokens:20,totalTokens:120,totalCost:0.01,successRate:90,averageLatencyMs:42},providers:[{providerId:'qa-p1',status:'healthy',latencyMs:42,successRate:98.5,lastCheckedAt:Date.now()}],checks:g.runs}),
    governance_events:()=>g.events,
    list_config_versions:()=>g.versions,
    create_config_version:({label})=>{const v={id:'qa-version-'+(++g.sequence+1),label,hash:'qa-created-hash',createdAt:now()};g.versions.unshift(v);return v;},
    preview_config_rollback:({id})=>({versionId:id,expectedCurrentHash:'qa-current-hash',targetHash:'qa-target-hash',before:'{\n  "model": "new"\n}',after:'{\n  "model": "old"\n}'}),
    apply_config_rollback:({confirmation})=>{if(confirmation!=='ROLLBACK')throw Error('QA confirmation missing');return {id:'qa-auto-backup',label:'Automatic pre-rollback backup',hash:'qa-current-hash',createdAt:now()};},
    delete_config_version:({id,confirmation})=>{if(confirmation!=='DELETE VERSION')throw Error('QA confirmation missing');g.versions=g.versions.filter(v=>v.id!==id);},
    choose_mount_root:()=>g.root,
    preview_explicit_mount:({rootId,client})=>{if(rootId!==g.root.id)throw Error('QA native root handle missing');return preview(client);},
    preview_mount_rollback:()=>preview('codex',true),
    apply_explicit_mount:({confirmation})=>{if(confirmation!=='WRITE FILE')throw Error('QA confirmation missing');const b={id:'qa-backup-'+(++g.sequence),target:g.root.path+'/AGENTS.md',createdAt:now()};g.backups.unshift(b);event('file_written',b.id);return b;},
    list_mount_backups:()=>g.backups,
    delete_mount_backup:({id,confirmation})=>{if(confirmation!=='DELETE BACKUP')throw Error('QA confirmation missing');g.backups=g.backups.filter(b=>b.id!==id);},
  };
  const invoke = window.__TAURI_INTERNALS__.invoke;
  window.__TAURI_INTERNALS__.invoke = async (command,args={}) => {
    if (!Object.hasOwn(handlers,command)) return invoke(command,args);
    qa.calls.push({command,args:clone(args)});
    if(qa.hold.includes(command)) await new Promise(resolve=>qa.pending.push({command,args,resolve}));
    if(qa.fail[command]) throw Error(qa.fail[command]);
    const value=handlers[command](args);
    return value===undefined?null:clone(value);
  };
})();
