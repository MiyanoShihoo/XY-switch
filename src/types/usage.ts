import type { AuditRecord } from "./model-audit";

export type DateRange = "today" | "yesterday" | "1d" | "7d" | "14d" | "30d";

export interface UsageFilter {
  dateRange?: DateRange;
  startTimeMs?: number;
  appType?: string;
  providerId?: string;
  model?: string;
  status?: string;
}

export interface UsageSummary {
  totalRequests: number;
  successCount: number;
  unknownCostRequests: number;
  totalInputTokens: number;
  totalOutputTokens: number;
  totalCacheReadTokens: number;
  totalCacheCreationTokens: number;
  totalCostUsd: string;
  avgLatencyMs: number;
}

export interface UsageLogEntry {
  audit?: AuditRecord | null;
  requestId: string;
  source?: string;
  totalTokens?: number | null;
  reasoningTokens?: number | null;
  providerId: string;
  appType: string;
  model: string;
  inputTokens: number;
  outputTokens: number;
  cacheReadTokens: number;
  cacheCreationTokens: number;
  totalCostUsd: string;
  inputCostUsd: string;
  outputCostUsd: string;
  cacheReadCostUsd: string;
  cacheCreationCostUsd: string;
  costMultiplier: string;
  isStreaming: boolean;
  latencyMs: number;
  statusCode: number;
  success?: boolean;
  errorMessage?: string;
  createdAt: number;
}

export interface UsageTrendPoint {
  timestamp: number;
  date: string;
  label: string;
  inputTokens: number;
  outputTokens: number;
  cacheReadTokens: number;
  cacheCreationTokens: number;
  cost: number | null;
  requests: number;
}

export interface ProviderUsageStat {
  providerId: string;
  totalRequests: number;
  successRate: number;
  inputTokens: number;
  outputTokens: number;
  totalCostUsd: string;
  avgLatencyMs: number;
}

export interface ModelUsageStat {
  model: string;
  providerId: string;
  totalRequests: number;
  inputTokens: number;
  outputTokens: number;
  totalCostUsd: string;
}

export interface ModelPricing {
  id: string;
}

export interface LocalUsageSync {
  codexImported: number;
  harnessImported: number;
  warnings: string[];
}
