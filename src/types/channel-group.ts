export type ChannelGroupStrategy = "roundRobin" | "weightedRoundRobin" | "leastLoad" | "latencyFirst" | "quotaAware";

export interface ChannelLimits {
  rpm: number | null;
  tpm: number | null;
  concurrency: number | null;
  dailyMicrousd: number | null;
}

export interface ChannelCooldownPolicy {
  rateLimitSeconds: number;
  failureSeconds: number;
  maxBackoffSeconds: number;
}

export interface Channel {
  id: string;
  providerId: string;
  accountId: string | null;
  upstreamModel: string;
  enabled: boolean;
  priority: number;
  weight: number;
  quotaPoolId: string | null;
  quotaIndependent: boolean;
  limits: ChannelLimits;
  cooldown: ChannelCooldownPolicy;
}

export interface ChannelGroup {
  id: string;
  name: string;
  virtualModel: string;
  enabled: boolean;
  strategy: ChannelGroupStrategy;
  projectId: string | null;
  affinityTtlSeconds: number;
  channels: Channel[];
  revision: number;
}

export interface ChannelGroupProjectBudget {
  id: string;
  dailyMicrousd: number | null;
  revision: number;
}

export interface ChannelGroupConfiguration {
  groups: ChannelGroup[];
  projects: ChannelGroupProjectBudget[];
}

export interface SaveChannelGroup {
  group: ChannelGroup;
  project: ChannelGroupProjectBudget | null;
}

export interface ChannelUsageTotals {
  requests: number;
  knownTokens: number;
  tokenKnownRequests: number;
  knownMicrousd: number;
  costKnownRequests: number;
}

export interface ChannelGroupRecordedEvent {
  id: number;
  providerId: string;
  atMs: number;
  status: number | null;
  latencyMs: number | null;
  tokens: number | null;
  costMicrousd: number | null;
  errorKind: string | null;
}

export interface ChannelRouteRequest {
  id: string;
  groupId: string;
  reserveTokens: number;
  reserveMicrousd: number | null;
  stateful: boolean;
  sessionKey: string | null;
  previousResponseId: string | null;
  conversationId: string | null;
}

export interface ChannelCompletion {
  outcome: "http" | "timeout" | "transportUnknown" | "cancelled" | "notSent";
  status: number | null;
  usage: { tokens: number | null; microusd: number | null };
  headers: Record<string, string>;
  responseId: string | null;
  conversationId: string | null;
}

export type ChannelTrialStep =
  | { kind: "request"; atMs: number; request: ChannelRouteRequest }
  | { kind: "streamStarted"; atMs: number; requestId: string }
  | { kind: "complete"; atMs: number; requestId: string; result: ChannelCompletion };

export interface ChannelGroupTrialInput {
  groupId: string;
  replayEventIds: number[];
  steps: ChannelTrialStep[];
}

export interface ChannelExclusion {
  channelId: string;
  reason: string;
  retryAtMs: number | null;
}

export interface ChannelDecision {
  requestId: string;
  channelId: string | null;
  upstreamModel: string | null;
  reason: string;
  excluded: ChannelExclusion[];
  dispatchAllowed: boolean;
}

export interface ChannelQuotaView {
  remainingRequests: number | null;
  remainingTokens: number | null;
  requestsResetAtMs: number | null;
  tokensResetAtMs: number | null;
  source: string | null;
}

export interface ChannelView {
  groupId: string;
  channelId: string;
  providerId: string;
  poolId: string;
  poolDeclared: boolean;
  health: string;
  cooldownReason: string | null;
  recoveryAtMs: number | null;
  rpm: number;
  tpmReserved: number;
  tpmUnknownRequests: number;
  concurrent: number;
  errorRate: number | null;
  latencyMs: number | null;
  quota: ChannelQuotaView;
  recordedUsage: ChannelUsageTotals;
  simulatedUsage: ChannelUsageTotals;
  unknownResults: number;
}

export interface ChannelGroupsSnapshot {
  phase: "offlinePilot";
  liveEnabled: false;
  billingHold: true;
  configuration: ChannelGroupConfiguration;
  channels: ChannelView[];
  warnings: string[];
  generatedAtMs: number;
}

export interface ChannelGroupTrialResult {
  source: "offlineSimulation";
  liveDispatches: 0;
  ledgerWrites: 0;
  decisions: ChannelDecision[];
  replayedEventIds: number[];
  channels: ChannelView[];
  warnings: string[];
}
