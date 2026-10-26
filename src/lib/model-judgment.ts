import { t } from "../i18n/language";

import type { AuditRecord } from "../types/model-audit";
import type { VerificationEvidence } from "../types/model-verification";
import { modelAuditStatus, reportedResponseModels } from "./model-audit";
import { isCompleteVerificationResponse } from "./model-verification";

export type ModelJudgmentKind = "no_evidence" | "transport_failure" | "incomplete" | "claim_conflict"
  | "claim_mismatch" | "route_changed" | "protocol_changed" | "usage_anomaly" | "claim_consistent";
export interface ModelJudgment {
  kind: ModelJudgmentKind;
  label: string;
  tone: "neutral" | "success" | "warning" | "danger";
  summary: string;
  nextStep: string;
  // An operational decision is separate from authentication of model weights.
  weightIdentity: "unverified";
  declaredModels: string[];
  reasonCodes: string[];
}
interface JudgmentInput {
  complete: boolean;
  claim: string;
  declaredModels: string[];
  codes: string[];
  httpStatus?: number | null;
  local?: boolean;
  observed?: boolean;
}
function models(values: string[]): string[] {
  return [...new Set(values.filter(value => typeof value === "string").map(value => value.trim()).filter(value =>
    value.length > 0 && value.length <= 256 && /^[\w./+:-]+$/.test(value)
    && !/^(sk-|bearer)/i.test(value) && !value.includes("://")))].sort();
}
function judge(input: JudgmentInput): ModelJudgment {
  const declaredModels = input.local ? [] : models(input.declaredModels);
  const reasonCodes = [...new Set(input.codes)];
  const has = (...codes: string[]) => codes.some(code => reasonCodes.includes(code));
  const result = (kind: ModelJudgmentKind, label: string, tone: ModelJudgment["tone"], summary: string, nextStep: string): ModelJudgment =>
    ({ kind, label, tone, summary, nextStep, weightIdentity: "unverified", declaredModels, reasonCodes });
  if (input.local || !input.observed) return result("no_evidence", t("尚无上游响应证据"), "neutral",
    input.local ? t("这里只有客户端会话记录，没有原始上游响应。") : t("尚未采集当前请求的响应，不能沿用其他请求的判定。"),
    t("开启被动观察后完成一次正常业务请求；此分析本身不会产生额外 API 请求。"));
  if ((input.httpStatus != null && !(input.httpStatus >= 200 && input.httpStatus < 300)) || has("HTTP_REQUEST_FAILED")) {
    return result("transport_failure", t("请求失败，不能判断型号"), "warning",
      t("错误回包不计为型号匹配，也不作为后续模型切换的基线；失败不等于模型冒充。"),
      t("先检查 HTTP 状态、余额、限流和路由错误；保留未知用量，不自动重试。"));
  }
  if (!input.complete || input.httpStatus === null || has("HTTP_STATUS_UNKNOWN", "RESPONSE_INVALID", "RESPONSE_INCOMPLETE", "STREAM_INCOMPLETE", "MODEL_EVIDENCE_TRUNCATED")) {
    return result("incomplete", t("响应证据不完整"), "neutral",
      t("响应未完整完成、缺少 HTTP 结果或解析有缺口；已观察到的声明和异常仍保留，不能判为一致或假冒。"),
      t("检查超时、流终止和解析记录；等待完整响应后再比较当前路由。"));
  }
  if (declaredModels.length > 1 || has("MODEL_DECLARATIONS_CONFLICT")) return result("claim_conflict", t("单次响应型号声明冲突"), "danger",
    t("同一次响应出现不同型号声明；这是真实的元数据冲突，但尚不能识别实际使用的权重。"),
    t("核对流式分片、网关改写和上游请求追踪记录，不把多个声明简单合并为一个真模型。"));
  if (input.claim === "MISMATCH" || has("MODEL_CLAIM_MISMATCH")) return result("claim_mismatch", t("返回型号声明不符"), "warning",
    t("声明与本次实际转发型号不一致；需区分未确认别名、回退和错误路由。"),
    t("先核对实际转发模型和精确别名，再向供应商核对该次上游调用；不要自动信任返回的新名称。"));
  if (has("MODEL_ROUTE_CHANGED")) return result("route_changed", t("当前路由声明发生变化"), "warning",
    t("同一凭据、配置和采样来源下的成功完整响应出现不同型号声明。"),
    t("核对动态路由和回退策略；跨凭据、跨协议或错误响应不会用于这个比较。"));
  if (has("PROTOCOL_SHAPE_CHANGED")) return result("protocol_changed", t("响应协议形态异常"), "warning",
    t("成功响应的协议形态与本次路线或此前可比响应不一致。协议差异本身不能证明模型被替换。"),
    t("检查 Chat 协议映射及网关封装；不要自动改走 Responses 来掩盖问题。"));
  if (has("USAGE_CONTRADICTORY", "USAGE_BOUND_CONTRADICTION", "OUTPUT_LIMIT_EXCEEDED")) return result("usage_anomaly", t("用量或上限语义异常"), "warning",
    t("存在用量不自洽、字节上界或输出上限差异；这属于计量问题，不是模型身份认证结果。"),
    t("核对 reasoning_tokens 是否已经包含在输出中、上限参数语义及账单；不要重复累计推理 Token。"));
  if (input.claim === "MATCH" && declaredModels.length === 1) return result("claim_consistent", t("型号声明一致（非权重认证）"), "success",
    t("完整响应的型号声明与实际转发型号或已确认别名一致；这确认名称一致，不自动确认工具能力或底层权重。"),
    t("更强归因需要独立来源的同题基线或可核验上游调用记录；模型自称与中转改写字段不会升级为验真。"));
  return result("incomplete", t("缺少可比的型号声明"), "neutral",
    t("没有足够的型号字段来比较本次转发与返回，不能把缺失值当作匹配。"),
    t("保留请求 ID 并核对供应商原始回包；不为补齐名称无限追加收费探测。"));
}

export function judgeVerificationEvidence(evidence: VerificationEvidence): ModelJudgment {
  return judge({ observed: true, complete: isCompleteVerificationResponse(evidence), claim: evidence.claimConsistency,
    declaredModels: evidence.reportedModels, codes: evidence.evidenceCodes, httpStatus: evidence.httpStatus });
}
export function judgeResponseModel(audit?: AuditRecord | null, local = false): ModelJudgment {
  const e = audit?.verification;
  const codes = [...(e?.evidenceCodes ?? [])];
  const mapping: Record<string, string> = { "inv.max_tokens_violated": "OUTPUT_LIMIT_EXCEEDED", "inv.byte_bound": "USAGE_BOUND_CONTRADICTION",
    "inv.usage_selfcontradictory": "USAGE_CONTRADICTORY", "inv.model_changed": "MODEL_DECLARATIONS_CONFLICT" };
  for (const flag of audit?.flags ?? []) if (mapping[flag]) codes.push(mapping[flag]);
  if (audit?.observation.modelEvidenceTruncated) codes.push("MODEL_EVIDENCE_TRUNCATED");
  const claim = { matched: "MATCH", mismatched: "MISMATCH", conflicting: "MISMATCH", unknown: "UNKNOWN" }[modelAuditStatus(audit)];
  return judge({ observed: Boolean(audit), local, complete: Boolean(audit?.observation.complete) && (!e || e.streamCompleted), claim,
    declaredModels: [...reportedResponseModels(audit), ...(e?.reportedModels ?? [])], codes, httpStatus: e?.httpStatus });
}

export function summarizeVerificationRoute(input: VerificationEvidence[], scope: { providerId: string; requestedModel: string }) {
  const eligible = input.filter(e => e.upstreamId === scope.providerId && e.requestedModel === scope.requestedModel && Number.isFinite(e.observedAt))
    .sort((a, b) => b.observedAt - a.observedAt);
  const context = eligible[0]?.contextKey;
  const seen = new Set<string>();
  const samples = eligible.filter(e => e.contextKey === context && !seen.has(e.observationId) && Boolean(seen.add(e.observationId)));
  const complete = samples.filter(isCompleteVerificationResponse);
  const failed = samples.filter(e => e.httpStatus != null && !(e.httpStatus >= 200 && e.httpStatus < 300)).length;
  const incomplete = samples.length - complete.length - failed;
  const judgments = samples.map(judgeVerificationEvidence);
  const anomalyKinds: ModelJudgmentKind[] = ["claim_conflict", "claim_mismatch", "route_changed", "protocol_changed", "usage_anomaly"];
  const anomalies = judgments.filter(j => anomalyKinds.includes(j.kind)).length;
  const count = (n: number | null): n is number => n != null && Number.isSafeInteger(n) && n >= 0;
  const known = samples.filter(e => {
    if (e.usageStatus !== "complete" || !count(e.totalTokens) || !count(e.inputTokens) || !count(e.outputTokens)) return false;
    let total = e.inputTokens + e.outputTokens;
    if (e.protocol === "anthropic") {
      const read = e.cacheReadTokens ?? 0, creation = e.cacheCreationTokens ?? 0;
      if (!count(read) || !count(creation)) return false;
      total += read + creation;
    }
    return Number.isSafeInteger(total) && total === e.totalTokens
      && (e.reasoningTokens == null || (count(e.reasoningTokens) && e.reasoningTokens <= e.outputTokens));
  });
  const tokenSum = known.reduce((sum, e) => sum + e.totalTokens!, 0);
  const consistentClaims = complete.filter(e => e.claimConsistency === "MATCH" && models(e.reportedModels).length === 1
    && !e.evidenceCodes.some(code => ["MODEL_DECLARATIONS_CONFLICT", "MODEL_CLAIM_MISMATCH"].includes(code))).length;
  const missingClaims = complete.filter(e => models(e.reportedModels).length === 0 || ["UNKNOWN", "MISSING"].includes(e.claimConsistency)).length;
  return {
    samples: samples.length, completed: complete.length, failed, incomplete, anomalies,
    passiveSamples: samples.filter(e => e.source === "passive").length,
    activeSamples: samples.filter(e => e.source === "active").length,
    consistentClaims, missingClaims, knownUsageSamples: known.length,
    reportedModels: models(complete.flatMap(e => e.reportedModels)),
    knownReportedTokens: Number.isSafeInteger(tokenSum) ? tokenSum : null,
    unknownUsageSamples: samples.length - known.length,
    label: !samples.length ? t("当前路由尚无证据") : anomalies ? t("发现需要核对的异常") : failed || incomplete ? t("可用性仍有缺口") : missingClaims ? t("型号声明证据不足") : t("当前样本型号声明一致"),
    tone: (!samples.length ? "neutral" : anomalies || failed ? "warning" : "info") as "neutral" | "warning" | "info",
    weightIdentity: "unverified" as const,
    determinism: "not_tested" as const,
  };
}
