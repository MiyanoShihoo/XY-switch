import type { PluginClient, ToolCallPreview } from "./plugins";
import type { McpApp } from "./mcp";
import type { PrivacyReport } from "./privacy";

export type SharedMcpOwner = "codex" | "claude" | "claude-desktop" | "claude-desktop-3p" | "opencode" | "gemini";

export const sharedMcpOwners: ReadonlyArray<{ id: SharedMcpOwner; label: string; clients: readonly PluginClient[] }> = [
  { id: "codex", label: "Codex Desktop / CLI", clients: ["codex-desktop", "codex-cli"] },
  { id: "claude-desktop", label: "Claude Desktop", clients: ["claude-desktop", "claude-desktop-3p"] },
  { id: "claude-desktop-3p", label: "Claude Desktop 3P", clients: ["claude-desktop", "claude-desktop-3p"] },
  { id: "opencode", label: "OpenCode Desktop / CLI", clients: ["opencode-desktop", "opencode-cli"] },
  { id: "claude", label: "Claude Code", clients: ["claude-code"] },
  { id: "gemini", label: "Gemini CLI", clients: ["gemini-cli"] },
];

export const sharedClientLabels: Record<PluginClient, string> = {
  "codex-desktop": "Codex Desktop", "codex-cli": "Codex CLI", "claude-desktop": "Claude Desktop",
  "claude-desktop-3p": "Claude Desktop 3P",
  "claude-code": "Claude Code", "opencode-desktop": "OpenCode Desktop", "opencode-cli": "OpenCode CLI", "gemini-cli": "Gemini CLI",
};

export function sharedMcpConfigTarget(owner: SharedMcpOwner): McpApp {
  return owner === "claude-desktop-3p" ? "claude-desktop" : owner;
}

export function sharedMcpScopeNeedsReview(connection: SharedMcpConnection): boolean {
  const scope = sharedMcpOwners.find(candidate => candidate.id === connection.owner);
  return !scope || connection.clients.length !== scope.clients.length
    || connection.clients.some((client, index) => client !== scope.clients[index]);
}

export interface SharedMcpConnection {
  id: string;
  owner: SharedMcpOwner;
  clients: PluginClient[];
  serverId: string;
  active: boolean;
}

export interface SharedMcpStatus {
  enabled: boolean;
  running: boolean;
  address: string;
  versions: string[];
  connections: SharedMcpConnection[];
  pendingCount: number;
  lastError: string | null;
}

export interface SharedToolApproval {
  id: string;
  owner: SharedMcpOwner;
  clients: PluginClient[];
  preview: ToolCallPreview;
  arguments: unknown;
  privacy: PrivacyReport;
}
