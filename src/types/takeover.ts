export type ManagedApp = "claude" | "codex" | "gemini" | "opencode" | "claude-desktop-3p";

export interface AppTakeoverStatus {
  app: ManagedApp;
  label: string;
  configPaths: string[];
  installed: boolean;
  active: boolean;
  lastError: string | null;
}
