// Injected before application modules, inside an isolated Chromium profile only.
// All IPC is intercepted here; no real configuration, filesystem, credentials or upstream requests.
(() => {
  const clone = value => structuredClone(value);
  const advanced = () => ({ isFullUrl: false, upstreamFormat: 'openai_responses', modelMappings: [{ model: 'qa-alpha', displayName: 'Alpha', contextWindow: 128000 }], customUserAgent: '', headerOverrides: {}, bodyOverrides: {}, codex: { remoteCompression: false, applyCommonConfig: false, context1m: false, compactionThreshold: 90, configToml: '' }, claude: { disableArtifactTool: false } });
  const apps = { claude: false, codex: true, gemini: false, grokbuild: false, opencode: false, hermes: false, pi: false };
  const qa = window.__XY_QA__ = {
    calls: [], fail: {}, hold: [], pending: [], callbacks: {}, listeners: [], nextCallback: 1, nextListener: 1,
    providers: ['Orion', 'Relay', 'Claude Lab'].map((name,i) => ({ id: 'qa-p'+(i+1), name, appGroup: i === 2 ? 'Claude' : 'Codex', providerType: 'custom', endpoint: 'https://qa.invalid/v1', model: 'qa-alpha', enabled: true, sortOrder: i, advanced: advanced() })),
    current: 'qa-p1', keys: { 'qa-p1': true },
    proxy: { settings: { enabled: false, showInHome: true, host: '127.0.0.1', port: 15721, routeTargets: ['claude','codex','gemini'] }, running: false, address: 'http://127.0.0.1:15721', lastError: null },
    preferences: { minimizeToTray: true, requestTimeoutSecs: 180, logLevel: 'info' }, autostart: false, autostartMismatch: false,
    prompts: [{ id: 'qa-prompt', name: '代码审查', description: '验收提示词', content: '# Review\n\n检查边界与错误。', enabled: true }],
    mcp: [{ id: 'qa-memory', name: 'Memory QA', description: '合成 MCP 服务', transport: 'stdio', command: 'node', args: ['memory.js'], url: null, env: { KEEP: 'value' }, headers: { 'X-QA': 'preserve' }, apps: { claude: false, codex: true, gemini: false, opencode: false }, builtIn: false, secretsConfigured: true }],
    skills: [{ id: 'qa-skill', name: 'Review Skill', description: '合成技能，本地验收', directory: 'QA:/skills/review', apps, installedAt: 1792195200000, updatedAt: 1792195200000 }],
    sessions: [{ providerId: 'codex', sessionId: 'qa-session', title: '工作台界面验收', projectDir: 'QA:/workspace', createdAt: 1792195200, lastActiveAt: 1792195260, sourcePath: 'QA:/sessions/test.jsonl', resumeCommand: 'codex resume qa-session' }],
    deleteCount: 1, usageCount: 45, importedRefreshFailure: false,
    release(command) { const entries = this.pending.filter(item => !command || item.command === command); this.pending = this.pending.filter(item => !entries.includes(item)); this.hold = this.hold.filter(item => item !== command); for (const item of entries) item.resolve(); },
    emit(event, payload) { for (const listener of this.listeners.filter(item => item.event === event)) this.callbacks[listener.handler]?.({ event, id: listener.id, payload }); },
  };
  const health = id => ({ providerId: id, status: 'healthy', latencyMs: 42, successRate: 98.5, consecutiveFailures: 0, circuitOpenUntil: null, lastError: null, lastCheckedAt: 1792195260000 });
  const takeovers = ['claude','codex','gemini'].map(app => ({ app, label: app === 'claude' ? 'Claude Code' : app === 'gemini' ? 'Gemini CLI' : 'Codex', configPaths: ['QA:/'+app+'/config'], installed: true, active: false, lastError: null }));
  async function invoke(command,args = {}) {
    qa.calls.push({ command, args: clone(args) });
    if (qa.hold.includes(command) || args.filter?.model === 'slow') await new Promise(resolve => qa.pending.push({ command, args, resolve }));
    const failure = qa.fail[command+':'+(args.providerId ?? args.id ?? '')] || qa.fail[command];
    if (failure) throw new Error(failure);
    const filteredCount = args.filter?.model === 'empty' ? 0 : args.filter?.model === 'fast' ? 1 : qa.usageCount;
    switch (command) {
      case 'list_providers': return clone(qa.providers);
      case 'get_current_provider': return clone(qa.providers.find(p => p.id === qa.current) ?? null);
      case 'list_provider_health': return qa.providers.map(p => health(p.id));
      case 'request_summary': return { totalRequests: 45, successfulRequests: 44, averageLatencyMs: 42 };
      case 'save_provider': qa.providers = [...qa.providers.filter(p => p.id !== args.provider.id), clone(args.provider)]; return null;
      case 'delete_provider': qa.providers = qa.providers.filter(p => p.id !== args.id); return null;
      case 'set_current_provider': qa.current = args.id; return null;
      case 'reorder_providers': qa.providers.sort((a,b) => args.providerIds.indexOf(a.id)-args.providerIds.indexOf(b.id)); return null;
      case 'refresh_codex_provider': return null;
      case 'has_provider_api_key': return Boolean(qa.keys[args.providerId]);
      case 'set_provider_api_key': qa.keys[args.providerId] = Boolean(args.apiKey); return null;
      case 'delete_provider_api_key': delete qa.keys[args.providerId]; return null;
      case 'test_provider_health': return health(args.providerId);
      case 'test_all_provider_health': return qa.providers.map(p => health(p.id));
      case 'discover_provider_models': return Array.from({length:40},(_,i)=>({id:'qa-discovered-'+i,name:'测试模型 '+i}));
      case 'get_proxy_status': return clone(qa.proxy);
      case 'set_proxy_enabled': qa.proxy.running = args.enabled; qa.proxy.settings.enabled = args.enabled; return clone(qa.proxy);
      case 'save_proxy_settings': qa.proxy.settings = clone(args.settings); qa.proxy.address = 'http://'+args.settings.host+':'+args.settings.port; return clone(qa.proxy);
      case 'list_app_takeovers': return clone(takeovers);
      case 'set_app_takeover_enabled': { const item = takeovers.find(s => s.app === args.app); item.active = args.enabled; return clone(item); }
      case 'get_app_preferences': return clone(qa.preferences);
      case 'save_app_preferences': qa.preferences = clone(args.preferences); return clone(qa.preferences);
      case 'get_autostart_enabled': return qa.autostart;
      case 'set_autostart_enabled': if (!qa.autostartMismatch) qa.autostart = args.enabled; return qa.autostart;
      case 'app_version': return '1.0.0-qa';
      case 'app_paths': return { dataDir: 'QA:/data', databasePath: 'QA:/data/xy.db', homeDir: 'QA:/home', codexHome: 'QA:/home/.codex' };
      case 'import_config': if (qa.importedRefreshFailure) qa.fail.list_providers = 'QA post-import refresh failed'; return { providers: 1, prompts: 1, mcpServers: 1, proxyImported: true };
      case 'export_config': return JSON.stringify({ version: 1, providers: qa.providers, prompts: qa.prompts, mcp: qa.mcp });
      case 'list_prompts': return clone(qa.prompts);
      case 'save_prompt': qa.prompts = [...qa.prompts.filter(p => p.id !== args.prompt.id), clone(args.prompt)]; return null;
      case 'delete_prompt': qa.prompts = qa.prompts.filter(p => p.id !== args.id); return null;
      case 'set_prompt_enabled': qa.prompts.find(p => p.id === args.id).enabled = args.enabled; return null;
      case 'list_mcp_servers': return clone(qa.mcp);
      case 'list_mcp_apps': return ['claude','codex','gemini','opencode'].map(app=>({app,label:app,installed:true,configPath:'QA:/'+app+'/config'}));
      case 'save_mcp_server': qa.mcp = [...qa.mcp.filter(s=>s.id !== args.server.id), clone(args.server)]; return clone(args.server);
      case 'delete_mcp_server': qa.mcp = qa.mcp.filter(s=>s.id !== args.id); return null;
      case 'set_mcp_app_enabled': { const item = qa.mcp.find(s=>s.id === args.serverId); item.apps[args.app] = args.enabled; return clone(item); }
      case 'import_mcp_from_apps': return { imported: 0, skipped: 1, errors: [] };
      case 'get_installed_skills': return clone(qa.skills);
      case 'discover_available_skills': return [{ key: 'qa-new', name: 'New QA Skill', description: '仓库发现验收', directory: 'new', repoOwner: 'qa', repoName: 'skills', repoBranch: 'main' }];
      case 'check_skill_updates': return [{id:'qa-skill',name:'Review Skill',remoteHash:'qa-remote'}];
      case 'uninstall_skill_unified': qa.skills = qa.skills.filter(s=>s.id !== args.id); return null;
      case 'toggle_skill_app': qa.skills.find(s=>s.id === args.id).apps[args.app] = args.enabled; return null;
      case 'update_skill': return clone(qa.skills.find(s=>s.id === args.id));
      case 'install_skill_unified': { const skill = { ...args.skill, id: args.skill.key, apps, installedAt: Date.now(), updatedAt: Date.now() }; qa.skills.push(skill); return clone(skill); }
      case 'import_skill_folder': return null;
      case 'pick_and_import_skill_folder': return null;
      case 'list_sessions': return clone(qa.sessions);
      case 'get_session_messages': return [{role:'user',content:'请检查界面边界。',ts:1792195200},{role:'assistant',content:'已保留全部审计证据。\nconst result = true;',ts:1792195260}];
      case 'delete_sessions': if (qa.deleteCount > 0) qa.sessions = qa.sessions.filter(s=>!args.ids.some(id=>id.sessionId === s.sessionId && id.providerId === s.providerId)); return qa.deleteCount;
      case 'get_usage_summary': return { totalRequests: filteredCount, successCount: filteredCount, totalInputTokens: 4500, totalOutputTokens: 900, totalCacheReadTokens: 0, totalCacheCreationTokens: 0, totalCostUsd: '0', avgLatencyMs: 42 };
      case 'get_usage_logs': return Array.from({length:Math.max(0,Math.min(args.limit,filteredCount-args.offset))},(_,i)=>({requestId:'qa-request-'+(args.offset+i),providerId:'qa-p1',appType:'codex',model:args.filter?.model ?? 'qa-alpha',inputTokens:100,outputTokens:20,cacheReadTokens:0,cacheCreationTokens:0,totalCostUsd:'0',inputCostUsd:'0',outputCostUsd:'0',cacheReadCostUsd:'0',cacheCreationCostUsd:'0',costMultiplier:'1',isStreaming:true,latencyMs:42,statusCode:200,createdAt:1792195260000-i*1000}));
      case 'get_usage_trends': return filteredCount ? [{ timestamp:1792195200000,date:'2026-10-17',label:'10/17',inputTokens:4500,outputTokens:900,cacheReadTokens:0,cacheCreationTokens:0,cost:0,requests:filteredCount }] : [];
      case 'get_provider_stats': return filteredCount ? [{providerId:'qa-p1',totalRequests:filteredCount,successRate:98.5,inputTokens:4500,outputTokens:900,totalCostUsd:'0',avgLatencyMs:42}] : [];
      case 'get_model_stats': return filteredCount ? [{providerId:'qa-p1',model:args.filter?.model ?? 'qa-alpha',totalRequests:filteredCount,inputTokens:4500,outputTokens:900,totalCostUsd:'0'}] : [];
      case 'analyze_model_audit': return { sampleCount:0, sampleLimit:10000, truncated:false, groups:[], notes:[] };
      case 'get_shadow_audit_settings': return [];
      case 'analyze_shadow_audit': return {sampleCount:0,sampleLimit:10000,pairCount:0,baselineCount:0,excludedCount:0,truncated:false,droppedRequests:0,groups:[],reasons:[],notes:[]};
      case 'plugin:event|listen': { const id=qa.nextListener++; qa.listeners.push({...args,id}); return id; }
      case 'plugin:event|unlisten': qa.listeners=qa.listeners.filter(item=>item.id!==args.eventId); return null;
      default: throw new Error('Unexpected QA IPC: '+command);
    }
  }
  window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener(event, id) { qa.listeners = qa.listeners.filter(item=>item.id!==id); } };
  window.__TAURI_INTERNALS__ = { invoke, metadata: { currentWindow: { label: 'main' }, currentWebview: { label: 'main', windowLabel: 'main' } }, transformCallback(callback) { const id=qa.nextCallback++; qa.callbacks[id]=callback; return id; }, unregisterCallback(id) { delete qa.callbacks[id]; }, convertFileSrc(path) { return path; } };
})();
