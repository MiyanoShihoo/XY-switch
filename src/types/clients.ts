import type { ManagedApp } from "./takeover";
import type { McpApp } from "./mcp";

export type ClientId = "codex-desktop" | "codex-cli" | "claude-desktop" | "claude-desktop-3p" | "claude-code" | "opencode-desktop" | "opencode-cli" | "gemini-cli";

export interface NativeQuota {
  source: string;
  usedPercent: number | null;
  resetAt: number | null;
  windowMinutes: number | null;
}

export interface NativeAccountSnapshot {
  clientId: ClientId;
  authMode: string;
  plan: string | null;
  checkedAt: number;
  quota: NativeQuota | null;
  quotas: NativeQuota[];
  providers: NativeProviderConnection[];
  poolCandidates: NativeAccountCandidate[];
}

export interface NativeAccountCandidate {
  id: string;
  label: string;
  expiresAt: number;
  boundAccountId: string | null;
}

export interface BindNativeAccountRequest {
  clientId: ClientId;
  candidateId: string;
  alias: string;
  providerId: string;
  approvedDestination: string;
  confirmed: boolean;
}

export interface NativeProviderConnection {
  providerId: string;
  label: string;
  connectionId: string | null;
  disconnectable: boolean;
}

export interface NativeLoginCondition { key: string; op: "eq" | "neq"; value: string; }
export interface NativeLoginPrompt {
  kind: "text" | "select";
  key: string;
  message: string;
  options: Array<{ label: string; value: string }>;
  when: NativeLoginCondition | null;
}
export interface NativeLoginMethod {
  providerId: string;
  providerLabel: string;
  methodId: string;
  label: string;
  prompts: NativeLoginPrompt[];
}

export interface NativeLoginProgress {
  clientId: ClientId;
  status: string;
  verificationUrl: string | null;
  userCode: string | null;
  providerId: string | null;
  instructions: string | null;
  requiresCode: boolean;
}

export interface ClientConnection {
  id: ClientId;
  label: string;
  configOwner: string;
  configPaths: string[];
  configDetected: boolean;
  takeoverTarget: ManagedApp | null;
  mcpTarget: McpApp;
  loginMethod: string;
  loginInstructions: string;
  inferenceMode: string;
  nativeVerification: string;
  setupError: string | null;
  configurationPending: boolean;
  loginStatus: string | null;
  account: NativeAccountSnapshot | null;
}

export interface ClientSettings {
  opencodeConfig: string | null;
  opencodeSchema: "v1" | "v2" | null;
  claudeDesktopProfile: string | null;
  codexBinary: string | null;
  opencodeBinary: string | null;
}
