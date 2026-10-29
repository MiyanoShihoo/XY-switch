import type { VerificationEvidence } from "./model-verification";
export type ModelAuditStatus = "matched" | "mismatched" | "conflicting" | "unknown";
export type AuditAxisStatus = "contradiction" | "consistent" | "not_comparable";

export interface StreamSummary {
  textDeltaCount: number;
  visibleOutputChars: number;
  maxDigitRun: number;
  maxHanRun: number;
  maxSpaceRun: number;
  medianDeltaChars: number;
  ttfbMs: number | null;
  digitChars: number;
  hanChars: number;
  spaceChars: number;
  arrivalOffsetsMs: number[];
  batched: boolean;
  minimumSamplesMet: boolean;
  samplesTruncated: boolean;
}

export interface AuditObservation {
  requestedModel?: string | null;
  forwardedModel?: string | null;
  upstreamModel?: string | null;
  upstreamModels?: string[];
  modelEvidenceTruncated?: boolean;
  inputTokens?: number | null;
  outputTokens?: number | null;
  totalTokens?: number | null;
  reasoningTokens?: number | null;
  maxTokens?: number | null;
  visibleOutputBytes?: number | null;
  toolPayloadBytes?: number | null;
  complete: boolean;
  textOnly: boolean;
  stream?: StreamSummary | null;
}

export interface AuditRecord {
  verification?: VerificationEvidence | null;
  observation: AuditObservation;
  flags: string[];
  gaps: string[];
  modelStatus: ModelAuditStatus;
  axes: { digit: AuditAxisStatus; han: AuditAxisStatus; space: AuditAxisStatus };
  streamBilling?: StreamBillingReport | null;
}

export interface StreamBillingReport {
  status: AuditAxisStatus;
  textFrames: number;
  visibleTokens: number | null;
  visibleBytes: number | null;
  tokensPerByte: number | null;
  bytesPerFrame: number | null;
  framesPerToken: number | null;
  gaps: string[];
  limitations: string[];
}

export interface PassiveAuditGroup {
  providerId: string;
  requestedModel: string;
  windowStart: number;
  composition: string;
  samples: number;
  comparableSamples: number;
  billingSamples: number;
  meanTokensPerByte: number | null;
  meanBytesPerFrame: number | null;
  meanDeltaChars: number | null;
  meanTtfbMs: number | null;
  meanDigitRun: number | null;
  meanHanRun: number | null;
  meanSpaceRun: number | null;
}

export interface PassiveAuditComparison {
  kind: "peer" | "history";
  left: string;
  right: string;
  requestedModel: string;
  composition: string;
  status: "similar" | "changed" | "insufficient";
  samplesLeft: number;
  samplesRight: number;
  billingSamplesLeft: number;
  billingSamplesRight: number;
  billingStatus: "similar" | "changed" | "insufficient";
  tokenByteRatio: number | null;
  frameByteRatio: number | null;
  deltaRatio: number | null;
  ttfbRatio: number | null;
}

export interface PassiveAuditReport {
  sampleLimit: number;
  sampleCount: number;
  truncated: boolean;
  auditedRequests: number;
  evidenceRequests: number;
  modelMismatchRequests: number;
  gapRequests: number;
  untestedRequests: number;
  groups: PassiveAuditGroup[];
  comparisons: PassiveAuditComparison[];
  notes: string[];
}

export type Pretokenizer = "cl100k" | "o200k" | "gpt2";

export interface ModelFingerprint {
  schemaVersion: number;
  corpusId: string;
  model: string;
  sourceSha256: string;
  tokenizerKind: string;
  pretokenizer: string;
  authority: string;
  groups: { id: string; chars: number; d1: number; d2: number; stable: boolean }[];
  limitations: string[];
}

export interface ReferenceCandidate {
  model: string;
  probability: number;
  comparable: number;
  matched: number;
  outside: boolean;
}

export interface ReferenceReport {
  status: "ranked" | "untested";
  candidates: ReferenceCandidate[];
  comparedGroups: string[];
  limitations: string[];
}

export interface ReferenceAnalysisRequest {
  observation: ModelFingerprint;
  candidates: ModelFingerprint[];
  config?: { pHit: number; outsideEdge: number; familyCap: number; minimumGroups: number };
}

export interface ShadowAuditSetting {
  enabled: boolean;
  providerId: string;
  model: string;
  tokenizerPath: string;
  pretokenizer: Pretokenizer | null;
}

export interface ShadowAuditStatus extends ShadowAuditSetting {
  sourceSha256: string | null;
  error: string | null;
  droppedRequests: number;
}

export interface ShadowRegressionReport {
  status: "untested" | "consistent" | "suspicious";
  sampleCount: number;
  minimumSamples: number;
  slope: number | null;
  intercept: number | null;
  slopeCiLow: number | null;
  slopeCiHigh: number | null;
  residualRmse: number | null;
  xSpan: number | null;
  interceptDrift: number | null;
  gaps: string[];
  limitations: string[];
}

export interface ShadowAuditReport {
  sampleLimit: number;
  sampleCount: number;
  pairCount: number;
  baselineCount: number;
  excludedCount: number;
  truncated: boolean;
  groups: {
    providerId: string;
    model: string;
    tokenizerSha256: string;
    partitionId: string;
    regression: ShadowRegressionReport;
  }[];
  reasons: { reason: string; count: number }[];
  droppedRequests: number;
  notes: string[];
}
