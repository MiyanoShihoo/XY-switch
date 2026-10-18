import type { VerificationEvidence } from "../types/model-verification";
export const CLAIM_LABELS = { MATCH: "声明匹配", MISMATCH: "声明不匹配", MISSING: "声明缺失", UNKNOWN: "声明证据不足" };
export const CODE_LABELS: Record<string, string> = {
  MODEL_EVIDENCE_TRUNCATED: "型号证据已截断，不能作完整响应比较",
  HTTP_REQUEST_FAILED: "HTTP 请求失败，不作为型号变化基线", HTTP_STATUS_UNKNOWN: "未取得 HTTP 结果",
  OUTPUT_LIMIT_EXCEEDED: "报告输出超出请求上限，需核对推理与上限语义", USAGE_BOUND_CONTRADICTION: "输出用量与已测载荷上界存在差异",
  CLAIM_MATCH: "声明与转发模型一致 / 已确认别名", MODEL_CLAIM_MISMATCH: "本条声明不一致", MODEL_DECLARATIONS_CONFLICT: "单条响应中出现冲突声明",
  MODEL_FIELD_MISSING: "缺少模型字段", ALIAS_UNRESOLVED: "尚未确认别名关系", USAGE_MISSING: "用量缺失或非最终统计",
  USAGE_CONTRADICTORY: "用量总数不自洽，无法按已知费用核销", RESPONSE_INCOMPLETE: "响应不完整或证据不足", STREAM_INCOMPLETE: "流未完整结束",
  RESPONSE_INVALID: "响应解析异常", MODEL_ROUTE_CHANGED: "同一路由出现模型声明变化", ALLOWED_ROUTE_CHANGE: "命中用户允许的回退 / 路由变化",
  PROTOCOL_SHAPE_CHANGED: "响应协议形态变化",
};
export const PROBE_LABELS: Record<string, string> = {
  disabled: "收费复核关闭", not_needed: "没有需要复核的异常", eligible: "异常符合本地检查条件", cost_boundary_unconfirmed: "未确认费用边界，已跳过",
  running: "复核中（已预留额度）", completed: "复核完成，仍非身份证明", cancelled: "已取消，可能仍产生用量", failed_or_unknown: "复核失败或用量不明，保留预留",
  daily_request_limit: "达到当天次数上限", daily_token_budget: "当天剩余额度不足", cooldown: "冷却中", probe_in_flight: "此模型已有复核在途",
  anomaly_already_checked: "此异常已复核，不重复请求", anomaly_stale_or_ineligible: "异常过期或路由已变化", active_probing_disabled: "收费复核未开启",
  cost_unknown_reasoning: "检测到无法约束的推理用量，已跳过",
};
const INCOMPLETE_RESPONSE_CODES = new Set([
  "HTTP_REQUEST_FAILED", "HTTP_STATUS_UNKNOWN", "RESPONSE_INVALID", "RESPONSE_INCOMPLETE",
  "STREAM_INCOMPLETE", "MODEL_EVIDENCE_TRUNCATED",
]);
export function isCompleteVerificationResponse(e: VerificationEvidence): boolean {
  return e.streamCompleted && e.httpStatus != null && e.httpStatus >= 200 && e.httpStatus < 300
    && !e.evidenceCodes.some(code => INCOMPLETE_RESPONSE_CODES.has(code));
}
export function eligibleEvidence(e: VerificationEvidence): boolean {
  return e.source === "passive" && isCompleteVerificationResponse(e)
    && e.evidenceCodes.some(c => ["MODEL_CLAIM_MISMATCH", "MODEL_ROUTE_CHANGED", "PROTOCOL_SHAPE_CHANGED"].includes(c));
}
