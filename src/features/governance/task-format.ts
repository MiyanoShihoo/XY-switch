import { t } from "../../i18n/language";

const sources: Record<string, string> = { codex_turn: "Codex 会话轮次", task_header: "任务标识请求头", idempotency_key: "幂等键", synthetic: "合成验收", request_only: "仅本次请求" };
const states: Record<string, string> = { sending: "发送中", response_pending: "等待用量结算", completed: "已完成", cancelled: "已取消", interrupted: "已中断", timeout: "请求超时", http_error: "上游 HTTP 失败", connect: "连接失败", request: "发送失败" };
const usageStates: Record<string, string> = { pending: "待结算", complete: "完整", partial: "部分", missing: "缺失" };
const costStates: Record<string, string> = { pending: "待结算", known: "已知", unknown: "未知" };
const budgetReasons: Record<string, string> = {
  task_attempt_budget_exhausted: "单任务总尝试上限已耗尽。",
  task_cost_budget_exhausted: "本次预留会超过单任务费用占用上限。",
  task_unreserved_cost: "此任务存在未预留的历史尝试，无法安全启用费用保护。",
  task_identity_required: "缺少稳定任务标识，无法累计客户端重试。",
  task_identity_ambiguous: "任务标识重复或相互冲突。",
  task_identity_invalid: "任务标识格式无效。",
  hard_token_budget_exhausted: "每日 Token 预留上限已耗尽。",
  hard_cost_budget_exhausted: "每日费用预留上限已耗尽。",
  budget_exact_custom_price_required: "实际派发模型缺少精确自定义价格。",
  task_cancelled: "任务已取消，后续重试已阻断。",
  request_cancelled: "任务已取消，后续重试已阻断。",
  stream_cancelled: "客户端在响应完成前关闭了接收。",
  process_restart: "应用重启前的尝试已中断，未知费用继续占用预留。",
  upstream_status: "供应商返回了失败的 HTTP 状态。",
  timeout: "上游请求超时。",
  connect: "无法连接供应商。",
  request: "上游请求未能完成。",
  stream_interrupted: "响应流在完成前中断。",
};

export const taskIdentityLabel = (source: string) => t(sources[source] ?? "未知");
export const taskAttemptLabel = (status: string) => t(states[status] ?? "未知");
export const taskUsageLabel = (state: string) => t(usageStates[state] ?? "未知");
export const taskCostLabel = (state: string) => t(costStates[state] ?? "未知");
export const budgetReasonLabel = (code: string) => budgetReasons[code] ? t(budgetReasons[code]) : code;
