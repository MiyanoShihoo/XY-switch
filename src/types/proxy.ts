export const routeTargets = ["claude", "codex", "gemini", "grok", "opencode"] as const;

export type RouteTarget = (typeof routeTargets)[number];

export const routeTargetLabels: Record<RouteTarget, string> = {
  claude: "Claude",
  codex: "Codex",
  gemini: "Gemini",
  grok: "Grok",
  opencode: "OpenCode",
};

export interface ProxySettings {
  enabled: boolean;
  showInHome: boolean;
  host: string;
  port: number;
  routeTargets: RouteTarget[];
}

export interface ProxyStatus {
  settings: ProxySettings;
  running: boolean;
  address: string;
  lastError: string | null;
}
