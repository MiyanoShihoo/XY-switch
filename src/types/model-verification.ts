export interface VerificationAlias { providerId: string; requestedModel: string; reportedModel: string; userConfirmed: boolean; allowRouteChange: boolean }
export interface VerificationSettings {
  enabled: boolean; activeProbeEnabled: boolean; maxRequestsPerDay: number; softTokenBudgetPerDay: number;
  maxOutputTokens: number; timeoutSeconds: number; cooldownHours: number; rollingWindowSize: number;
  repeatedAnomalyThreshold: number; storeDiagnosticIds: boolean; aliases: VerificationAlias[];
}
export const VERIFICATION_TOKEN_BUDGET_RANGE = { min: 0, max: 10000 } as const;
export const DEFAULT_VERIFICATION_SETTINGS: VerificationSettings = {
  enabled: false, activeProbeEnabled: false, maxRequestsPerDay: 1, softTokenBudgetPerDay: 100,
  maxOutputTokens: 8, timeoutSeconds: 15, cooldownHours: 24, rollingWindowSize: 10,
  repeatedAnomalyThreshold: 2, storeDiagnosticIds: false, aliases: [],
};
export interface VerificationEvidence {
  observationId: string; upstreamId: string; requestedModel: string; forwardedModel: string | null;
  reportedModels: string[]; credentialFingerprint: string; contextKey: string;
  protocol: string; protocolShape: string | null; claimConsistency: "MATCH" | "MISMATCH" | "MISSING" | "UNKNOWN";
  identityAssurance: "UNVERIFIED" | "INCONCLUSIVE"; evidenceCodes: string[]; explanation: string; riskLevel: string;
  observedAt: number; usageStatus: string; inputTokens: number | null; outputTokens: number | null;
  reasoningTokens: number | null; totalTokens: number | null; cacheReadTokens: number | null; cacheCreationTokens: number | null;
  isStreaming: boolean; streamCompleted: boolean; httpStatus: number | null; finishReason: string | null;
  responseId: string | null; providerRequestId: string | null; evidenceSource: string; source: "active" | "passive";
  rollingObservedCount: number; routeChangeCount: number; repeatedAnomaly: boolean; newEvidence: boolean;
  activeProbeState: string; activeProbeTokens: number | null;
}
export interface VerificationStatus {
  settings: VerificationSettings;
  contextError?: string | null; recent: VerificationEvidence[]; observedResponses: number; probeRequestsToday: number;
  probeTokensToday: number | null; budgetUsedToday: number; capabilityConfirmed: boolean;
  droppedObservations: number; storageError: boolean;
}
