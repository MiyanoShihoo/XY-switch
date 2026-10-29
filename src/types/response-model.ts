import type { AuditRecord } from "./model-audit";

export interface ResponseModelProbe {
  requestId: string;
  providerId: string;
  protocol: string;
  createdAt: number;
  httpStatus: number | null;
  latencyMs: number;
  success: boolean;
  streaming: boolean;
  receivedBytes: number;
  inputTokens: number | null;
  outputTokens: number | null;
  cacheReadTokens: number | null;
  cacheCreationTokens: number | null;
  totalCostUsd: number | null;
  audit: AuditRecord;
  errorMessage: string | null;
}
