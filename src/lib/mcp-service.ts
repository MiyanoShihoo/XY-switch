import { t } from "../i18n/language";
import { invoke } from "@tauri-apps/api/core";
import type { McpApp, McpAppStatus, McpImportResult, McpServer } from "../types/mcp";
import { requireUserManagedMcpServer } from "./mcp-validation";

const previewServers: McpServer[] = [
  {
    id: "memory",
    name: "Memory",
    description: "持久化知识图谱记忆",
    transport: "stdio",
    command: "cmd",
    args: ["/c", "npx", "-y", "@modelcontextprotocol/server-memory"],
    url: null,
    env: {},
    headers: {},
    apps: { claude: false, codex: false, gemini: false, opencode: false, "claude-desktop": false },
    builtIn: true,
    secretsConfigured: false,
  },
];

const previewApps: McpAppStatus[] = [
  { app: "claude", label: "Claude Code", installed: false, configPath: "~/.claude.json", lastError: null },
  { app: "codex", label: "Codex", installed: false, configPath: "~/.codex/config.toml", lastError: null },
  { app: "gemini", label: "Gemini CLI", installed: false, configPath: "~/.gemini/settings.json", lastError: null },
  { app: "opencode", label: "OpenCode", installed: false, configPath: "~/.config/opencode/opencode.json", lastError: null },
  { app: "claude-desktop", label: "Claude Desktop", installed: false, configPath: "Claude/claude_desktop_config.json", lastError: null },
];

function isDesktopRuntime(): boolean {
  return "__TAURI_INTERNALS__" in window;
}

export async function loadMcpServers(): Promise<McpServer[]> {
  if (!isDesktopRuntime()) return previewServers;
  return invoke<McpServer[]>("list_mcp_servers");
}

export async function loadMcpApps(): Promise<McpAppStatus[]> {
  if (!isDesktopRuntime()) return previewApps;
  return invoke<McpAppStatus[]>("list_mcp_apps");
}

export async function saveMcpServer(server: McpServer): Promise<McpServer> {
  requireUserManagedMcpServer(server.id);
  if (!isDesktopRuntime()) throw new Error(t("浏览器预览模式不能修改 MCP 配置"));
  return invoke<McpServer>("save_mcp_server", { server });
}

export async function deleteMcpServer(id: string): Promise<void> {
  requireUserManagedMcpServer(id);
  if (!isDesktopRuntime()) throw new Error(t("浏览器预览模式不能删除 MCP 配置"));
  await invoke("delete_mcp_server", { id });
}

export async function setMcpAppEnabled(
  serverId: string,
  app: McpApp,
  enabled: boolean,
): Promise<McpServer> {
  requireUserManagedMcpServer(serverId);
  if (!isDesktopRuntime()) throw new Error(t("浏览器预览模式不能同步 MCP 配置"));
  return invoke<McpServer>("set_mcp_app_enabled", { serverId, app, enabled });
}

export async function importMcpFromApps(): Promise<McpImportResult> {
  if (!isDesktopRuntime()) throw new Error(t("浏览器预览模式不能导入 MCP 配置"));
  return invoke<McpImportResult>("import_mcp_from_apps");
}
