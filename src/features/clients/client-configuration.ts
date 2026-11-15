import type { ClientConnection } from "../../types/clients";

export interface ClientConfigurationGroup {
  id: string;
  owner: string | null;
  rows: ClientConnection[];
}

export function groupClientConfigurations(rows: ClientConnection[]): ClientConfigurationGroup[] {
  const groups = new Map<string, ClientConfigurationGroup>();
  for (const row of rows) {
    const owner = row.configOwner?.trim() ? row.configOwner : null;
    const id = owner ? "owner:" + owner : "client:" + row.id;
    const group = groups.get(id);
    if (group) group.rows.push(row);
    else groups.set(id, { id, owner, rows: [row] });
  }
  return [...groups.values()];
}

export function clientConfigurationLabel(group: ClientConfigurationGroup): string {
  const labels: Record<string, string> = { codex: "Codex", opencode: "OpenCode", "claude-desktop": "Claude Desktop", "claude-desktop-3p": "Claude Desktop 3P", claude: "Claude Code", gemini: "Gemini CLI" };
  return group.owner ? labels[group.owner] ?? group.owner : group.rows[0].label;
}
