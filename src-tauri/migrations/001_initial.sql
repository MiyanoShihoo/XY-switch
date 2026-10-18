CREATE TABLE IF NOT EXISTS providers (
  id TEXT PRIMARY KEY NOT NULL,
  name TEXT NOT NULL,
  provider_type TEXT NOT NULL,
  endpoint TEXT NOT NULL,
  model TEXT NOT NULL,
  app_group TEXT NOT NULL,
  enabled INTEGER NOT NULL DEFAULT 1,
  sort_order INTEGER NOT NULL DEFAULT 0,
  advanced_json TEXT NOT NULL DEFAULT '{}',
  created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
  updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE TABLE IF NOT EXISTS mcp_servers (
  id TEXT PRIMARY KEY NOT NULL,
  name TEXT NOT NULL,
  transport TEXT NOT NULL CHECK (transport IN ('stdio','http')),
  config_json TEXT NOT NULL,
  enabled INTEGER NOT NULL DEFAULT 1,
  created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
  updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE TABLE IF NOT EXISTS mcp_app_states (
  server_id TEXT NOT NULL REFERENCES mcp_servers(id) ON DELETE CASCADE,
  app TEXT NOT NULL CHECK (app IN ('claude','codex','gemini','opencode')),
  enabled INTEGER NOT NULL DEFAULT 0,
  updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
  PRIMARY KEY(server_id, app)
);
CREATE TABLE IF NOT EXISTS prompts (
  id TEXT PRIMARY KEY NOT NULL,
  name TEXT NOT NULL,
  content TEXT NOT NULL,
  description TEXT,
  enabled INTEGER NOT NULL DEFAULT 1,
  target_apps TEXT NOT NULL DEFAULT '[]',
  active INTEGER NOT NULL DEFAULT 0,
  created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
  updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE TABLE IF NOT EXISTS usage_events (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  provider_id TEXT NOT NULL REFERENCES providers(id) ON DELETE CASCADE,
  model TEXT NOT NULL,
  input_tokens INTEGER NOT NULL DEFAULT 0,
  output_tokens INTEGER NOT NULL DEFAULT 0,
  cost_micros INTEGER NOT NULL DEFAULT 0,
  latency_ms INTEGER,
  success INTEGER NOT NULL DEFAULT 1,
  created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE INDEX IF NOT EXISTS idx_usage_provider_created ON usage_events(provider_id, created_at);

CREATE TABLE IF NOT EXISTS app_settings (
  key TEXT PRIMARY KEY NOT NULL,
  value TEXT NOT NULL,
  updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

INSERT OR IGNORE INTO providers(id,name,provider_type,endpoint,model,app_group,enabled,sort_order) VALUES
  ('anthropic','Anthropic','anthropic','https://api.anthropic.com','claude-sonnet','Claude',1,10),
  ('openai','OpenAI','openai','https://api.openai.com/v1','gpt-5','Codex',1,20),
  ('deepseek','DeepSeek','openai_compatible','https://api.deepseek.com','deepseek-chat','Codex',0,40),
  ('glm','GLM','openai_compatible','https://open.bigmodel.cn/api/v1','glm-4','Codex',0,50);

INSERT OR IGNORE INTO app_settings(key,value) VALUES('current_provider','anthropic');

INSERT OR IGNORE INTO mcp_servers(id,name,transport,config_json,enabled) VALUES
  ('memory','Memory','stdio','{"description":"持久化知识图谱记忆","command":"cmd","args":["/c","npx","-y","@modelcontextprotocol/server-memory"],"url":null,"envKeys":[],"headerKeys":[],"builtIn":true}',1),
  ('sequential-thinking','Sequential Thinking','stdio','{"description":"分步推理与问题拆解","command":"cmd","args":["/c","npx","-y","@modelcontextprotocol/server-sequential-thinking"],"url":null,"envKeys":[],"headerKeys":[],"builtIn":true}',1),
  ('context7','Context7','stdio','{"description":"查询最新的库与框架文档","command":"cmd","args":["/c","npx","-y","@upstash/context7-mcp"],"url":null,"envKeys":[],"headerKeys":[],"builtIn":true}',1),
  ('fetch','Fetch','stdio','{"description":"抓取并转换网页内容","command":"uvx","args":["mcp-server-fetch"],"url":null,"envKeys":[],"headerKeys":[],"builtIn":true}',1);

CREATE TABLE IF NOT EXISTS request_events (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  provider_id TEXT NOT NULL REFERENCES providers(id) ON DELETE CASCADE,
  route TEXT NOT NULL,
  status_code INTEGER,
  success INTEGER NOT NULL DEFAULT 0,
  latency_ms INTEGER NOT NULL DEFAULT 0,
  retry_count INTEGER NOT NULL DEFAULT 0,
  error_kind TEXT,
  model TEXT,
  input_tokens INTEGER,
  output_tokens INTEGER,
  cache_creation_tokens INTEGER,
  cache_read_tokens INTEGER,
  cost REAL,
  price_source TEXT,
  price_version TEXT,
  created_at INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_request_events_provider_created ON request_events(provider_id, created_at);

CREATE TABLE IF NOT EXISTS provider_health (
  provider_id TEXT PRIMARY KEY NOT NULL REFERENCES providers(id) ON DELETE CASCADE,
  status TEXT NOT NULL DEFAULT 'unknown',
  latency_ms INTEGER,
  success_rate REAL NOT NULL DEFAULT 0,
  consecutive_failures INTEGER NOT NULL DEFAULT 0,
  circuit_open_until INTEGER,
  last_error TEXT,
  last_checked_at INTEGER NOT NULL DEFAULT 0
);

