export type ManagedApp = "claude" | "codex" | "gemini";

export interface AppTakeoverStatus {
  app: ManagedApp;
  label: string;
  configPaths: string[];
  installed: boolean;
  active: boolean;
  lastError: string | null;
}
