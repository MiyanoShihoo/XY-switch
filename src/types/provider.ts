export type AppGroup = "Claude" | "Codex" | "Gemini";
export type UpstreamFormat = "openai_responses" | "openai_chat" | "anthropic";

export interface ProviderModelMapping {
  model: string;
  displayName: string;
  contextWindow: number | null;
}

export interface CodexProviderConfig {
  remoteCompression: boolean;
  applyCommonConfig: boolean;
  context1m: boolean;
  compactionThreshold: number;
  configToml: string;
}

export interface ClaudeProviderConfig {
  /** 开启后，转发前把 Claude Code 的 Artifact 工具整个从 tools 数组里摘掉。 */
  disableArtifactTool: boolean;
}

export interface ProviderAdvancedConfig {
  isFullUrl?: boolean;
  upstreamFormat: UpstreamFormat;
  modelMappings: ProviderModelMapping[];
  customUserAgent: string;
  headerOverrides: Record<string, string>;
  bodyOverrides: Record<string, unknown>;
  codex: CodexProviderConfig;
  claude: ClaudeProviderConfig;
}

export interface Provider {
  id: string;
  name: string;
  group: AppGroup;
  endpoint: string;
  model: string;
  enabled: boolean;
  latency?: number;
  icon: string;
  builtIn?: boolean;
  providerType?: string;
  sortOrder?: number;
  advanced: ProviderAdvancedConfig;
}

export interface StoredProvider {
  id: string;
  name: string;
  providerType: string;
  endpoint: string;
  model: string;
  appGroup: string;
  enabled: boolean;
  sortOrder: number;
  advanced: ProviderAdvancedConfig;
}

export type ProviderHealthStatus = "unknown" | "healthy" | "unhealthy" | "circuit_open";

export interface ProviderHealth {
  providerId: string;
  status: ProviderHealthStatus;
  latencyMs: number | null;
  successRate: number;
  consecutiveFailures: number;
  circuitOpenUntil: number | null;
  lastError: string | null;
  lastCheckedAt: number;
}

export interface RequestSummary {
  totalRequests: number;
  successfulRequests: number;
  averageLatencyMs: number;
}
