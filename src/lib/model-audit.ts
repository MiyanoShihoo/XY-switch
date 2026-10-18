import type { AuditRecord, ModelAuditStatus, ReferenceCandidate, ReferenceReport } from "../types/model-audit";
import { isCompleteVerificationResponse } from "./model-verification";

export const MODEL_STATUS_LABELS: Record<ModelAuditStatus, string> = {
  matched: "自报一致",
  mismatched: "自报不一致",
  conflicting: "单次响应声明冲突",
  unknown: "无法判断",
};

export const AUDIT_FLAG_LABELS: Record<string, string> = {
  "inv.byte_bound": "输出 token 超出字节上界",
  "inv.usage_selfcontradictory": "用量总数不自洽",
  "inv.model_mismatch": "上游自报模型不一致",
  "inv.model_changed": "同一响应出现不同模型声明",
  "inv.max_tokens_violated": "输出超出请求 token 上限",
};

export const HARD_AUDIT_FLAGS = new Set(["inv.byte_bound", "inv.usage_selfcontradictory", "inv.max_tokens_violated"]);

const GAP_LABELS: Record<string, string> = {
  "byte_bound.reasoning_unknown": "未上报推理 token，无法排除隐藏推理",
  "byte_bound.output_missing": "缺少输出 token 数",
  "byte_bound.payload_unmeasured": "工具调用载荷未完整测量",
  "byte_bound.incomplete": "响应不完整，字节上界检查未执行",
  "byte_bound.non_text": "含有非文本输出，不能应用文本字节上界",
  "byte_bound.invalid_reasoning": "推理 token 数无效，无法扣除",
  "usage.fields_missing": "缺少用量字段，无法检查总数自洽",
  "model.upstream_missing": "上游未自报模型",
  "model.requested_missing": "缺少请求模型",
  "model.evidence_truncated": "模型声明过长或过多，证据已截断",
  "max_tokens.missing": "缺少请求 token 上限",
  "scv.no_stream": "非流式响应，无切分观测",
  "scv.batched": "检测到帧批处理，切分结论不可用",
  "scv.insufficient_samples": "流式样本不足，切分结论不可用",
  "scv.billing_axis_uncalibrated": "缺少可比的计费分词基线",
  "scv.token_boundaries_unverified": "SSE 边界未证实等同于 token 边界，切分结果仅为统计线索",
  "scv.incomplete": "响应不完整，无法比较整段输出",
  "scv.non_text": "含有非文本输出，无法比较整段计费",
  "scv.reasoning_unknown": "推理 token 未知，无法计算上报可见 token",
  "scv.output_missing": "缺少输出 token 用量",
  "scv.invalid_reasoning": "推理 token 数超过输出 token，无法扣除",
  "scv.tools_present_or_unknown": "存在工具载荷或载荷未知，无法只按文本帧比较",
  "scv.no_text": "没有可见文本，无法计算整段比率",
  "scv.bytes_missing": "缺少可见文本 UTF-8 字节数，无法计算整段比率",
};

/** Compare only evidence belonging to this response, including legacy records. */
export function reportedResponseModels(audit?: AuditRecord | null): string[] {
  const observation = audit?.observation;
  const models = [...(observation?.upstreamModels ?? []), observation?.upstreamModel];
  return [...new Set(models.filter((value): value is string => typeof value === "string" && Boolean(value.trim())))];
}

export function modelAuditStatus(audit?: AuditRecord | null): ModelAuditStatus {
  if (audit?.verification) {
    const e = audit.verification;
    if (!isCompleteVerificationResponse(e) || !audit.observation.complete || audit.observation.modelEvidenceTruncated) return "unknown";
    const models = new Set([...e.reportedModels, ...reportedResponseModels(audit)].map(model => model.trim()).filter(Boolean));
    if (models.size > 1 || e.evidenceCodes.includes("MODEL_DECLARATIONS_CONFLICT")) return "conflicting";
    if (!models.size) return "unknown";
    if (e.evidenceCodes.includes("MODEL_CLAIM_MISMATCH")) return "mismatched";
    return ({ MATCH: "matched", MISMATCH: "mismatched", MISSING: "unknown", UNKNOWN: "unknown" } as const)[audit.verification.claimConsistency] ?? "unknown";
  }
  const observation = audit?.observation;
  const models = reportedResponseModels(audit);
  if (models.length > 1) return "conflicting";
  if (!observation?.complete || observation.modelEvidenceTruncated) return "unknown";
  const expected = observation?.forwardedModel?.trim() ? observation.forwardedModel : observation?.requestedModel;
  if (!expected?.trim() || !models.length) return "unknown";
  return models[0] === expected ? "matched" : "mismatched";
}

export function auditGapLabel(id: string): string {
  return GAP_LABELS[id] ?? `未完成检查：${id}`;
}

export function auditEvidenceState(audit?: AuditRecord | null): "evidence" | "untested" | "clear" {
  if (audit?.flags?.length) return "evidence";
  if (!audit || audit.gaps.length > 0 || !audit.observation.complete) return "untested";
  return "clear";
}

export function byteBoundExplanation(audit: AuditRecord): string {
  const o = audit.observation;
  const value = (n: number | null | undefined) => n == null ? "未测量" : n.toLocaleString();
  return `输出 ${value(o.outputTokens)} token − 推理 ${value(o.reasoningTokens)} token；可见文本 ${value(o.visibleOutputBytes)} UTF-8 字节 + 工具载荷 ${value(o.toolPayloadBytes)} 字节 + 特殊 token 余量 4。`;
}

/** Validate the entire distribution. Never renormalize away the outside class. */
export function normalizeReferenceReport(raw: unknown): ReferenceReport {
  if (!raw || typeof raw !== "object") throw new Error("参照分析返回格式无效");
  const report = raw as Partial<ReferenceReport>;
  if (report.status !== "ranked" && report.status !== "untested") throw new Error("参照分析缺少有效状态");
  if (!Array.isArray(report.candidates) || !Array.isArray(report.comparedGroups) || !Array.isArray(report.limitations)) {
    throw new Error("参照分析缺少候选、比较组或限制说明");
  }
  if (!report.comparedGroups.every((v) => typeof v === "string") || !report.limitations.every((v) => typeof v === "string")) {
    throw new Error("参照分析说明格式无效");
  }
  const candidates = report.candidates.map((candidate): ReferenceCandidate => {
    if (!candidate || typeof candidate.model !== "string" || typeof candidate.outside !== "boolean"
      || !Number.isFinite(candidate.probability) || candidate.probability < 0 || candidate.probability > 1
      || !Number.isInteger(candidate.comparable) || candidate.comparable < 0
      || !Number.isInteger(candidate.matched) || candidate.matched < 0 || candidate.matched > candidate.comparable) {
      throw new Error("参照分析候选或概率无效");
    }
    return { ...candidate };
  });
  if (report.status === "ranked") {
    const total = candidates.reduce((sum, candidate) => sum + candidate.probability, 0);
    if (candidates.filter((candidate) => candidate.outside).length !== 1 || Math.abs(total - 1) > 0.00001) {
      throw new Error("参照分析概率分布不完整，必须保留唯一库外候选且概率总和为 1");
    }
  } else if (candidates.length > 0) {
    throw new Error("未测到时不能显示候选概率");
  }
  return { status: report.status, candidates: candidates.sort((a, b) => b.probability - a.probability), comparedGroups: report.comparedGroups, limitations: report.limitations };
}
