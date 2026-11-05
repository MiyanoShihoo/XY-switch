CREATE TABLE mcp_app_states_updated (
  server_id TEXT NOT NULL REFERENCES mcp_servers(id) ON DELETE CASCADE,
  app TEXT NOT NULL CHECK (app IN ('claude','codex','gemini','opencode','claude-desktop')),
  enabled INTEGER NOT NULL DEFAULT 0,
  updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
  PRIMARY KEY(server_id, app)
);
INSERT INTO mcp_app_states_updated(server_id,app,enabled,updated_at)
SELECT server_id,app,enabled,updated_at FROM mcp_app_states;
DROP TABLE mcp_app_states;
ALTER TABLE mcp_app_states_updated RENAME TO mcp_app_states;
