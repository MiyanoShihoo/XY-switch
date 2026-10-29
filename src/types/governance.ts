import type { RequestSummary } from "./provider";

export interface AccessPolicy { requireToken: boolean; requestsPerMinute: number; maxInFlight: number; }
export interface AccessTokenInfo { id: string; label: string; scope: "proxy" | "monitor"; createdAt: number; expiresAt: number; revoked: boolean; }
export interface IssuedAccessToken { token: string; info: AccessTokenInfo; }
export interface BudgetPolicy { enabled: boolean; dailyTokens: number; dailyMicrousd: number; inputReservation: number; maxOutputTokens: number; }
export interface BudgetStatus { policy: BudgetPolicy; dayUtc: number; reservedTokens: number; reservedMicrousd: number; attempts: number; }
export type CheckMode = "chat" | "stream" | "tools" | "responses" | "anthropic" | "gemini";
export interface MonitorPolicy { enabled: boolean; providerId: string; model: string; mode: CheckMode; intervalMinutes: number; maxRunsPerDay: number; }
export interface CheckEvidence { httpStatus: number | null; reportedModel: string | null; inputTokens: number | null; outputTokens: number | null; totalTokens: number | null; }
export interface SyntheticRun { id: string; providerId: string; model: string; mode: CheckMode; status: "running" | "passed" | "failed"; latencyMs: number; detail: string; createdAt: number; evidence: CheckEvidence; }
export interface ConfigVersion { id: string; label: string; hash: string; createdAt: number; }
export interface VersionPreview { versionId: string; expectedCurrentHash: string; targetHash: string; before: string; after: string; }
export interface MountRoot { id: string; path: string; }
export type MountClient = "claude" | "codex" | "gemini";
export interface MountPreview { id: string; target: string; before: string; after: string; createsFile: boolean; removesFile: boolean; expiresAt: number; }
export interface MountBackup { id: string; target: string; createdAt: number; }
export interface GovernanceEvent { kind: string; detail: string; createdAt: number; }
export interface GovernanceOverview {
  time: number;
  budget: BudgetStatus;
  requests: RequestSummary;
  providers: { providerId: string; status: string; latencyMs: number | null; successRate: number; lastCheckedAt: number; }[];
  checks: Pick<SyntheticRun, "id" | "providerId" | "mode" | "status" | "latencyMs" | "createdAt">[];
}
