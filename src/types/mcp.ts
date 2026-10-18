export type McpTransport = "stdio" | "http";
export type McpApp = "claude" | "codex" | "gemini" | "opencode";

export interface McpServer {
  id: string;
  name: string;
  description: string;
  transport: McpTransport;
  command: string | null;
  args: string[];
  url: string | null;
  env: Record<string, string>;
  headers: Record<string, string>;
  apps: Record<McpApp, boolean>;
  builtIn: boolean;
  secretsConfigured: boolean;
}

export interface McpAppStatus {
  app: McpApp;
  label: string;
  installed: boolean;
  configPath: string;
}

export interface McpImportResult {
  imported: number;
  errors: string[];
}
