import type { ShadowAuditSetting, ShadowAuditStatus } from "../types/model-audit";

const REASONS: Record<string, string> = {
  "shadow.disabled": "未启用影子采样",
  "shadow.ineligible_history": "历史不符合只追加的纯文本对话条件",
  "shadow.incomplete": "响应未完整结束",
  "shadow.failover": "发生渠道切换，无法沿用前一轮基线",
  "shadow.input_unknown": "缺少输入 token 用量",
  "shadow.cache_unknown": "缓存计数未知，无法排除缓存影响",
  "shadow.cache_present": "存在缓存命中或缓存创建，已排除",
  "shadow.response_unknown": "缺少可匹配的完整回复",
  "shadow.invalid_fingerprint": "历史结构指纹无效",
  "shadow.history_shape": "对话结构发生变化",
  "shadow.additions_too_large": "新增文本超过本地计算上限",
  "shadow.no_parent": "没有可匹配的上一轮（可能是新会话或前缀、配置变化），保留为新基线",
  "shadow.response_mismatch": "追加的助手回复与上一轮响应不一致",
  "shadow.ambiguous_prefix": "存在多个可能前缀，无法唯一配对",
  "shadow.parent_reused": "上一轮已被配对，避免重复计入",
  "shadow.parent_ineligible": "匹配的上一轮不符合采样条件，不能作为有效基线",
  "shadow.forwarded_text_changed": "客户端新增文本与本地实际转发文本不一致，已排除",
  "shadow.forwarded_unsupported_history": "本地实际转发的历史不符合只追加的纯文本条件",
  "shadow.forwarded_history_budget_exceeded": "本地实际转发的历史超过采样上限",
  "shadow.forwarded_request_budget_exceeded": "本地实际转发的请求超过采样上限",
  "shadow.forwarded_text_budget_exceeded": "本地实际转发的新增文本超过计算上限",
  "shadow.forwarded_tools_server_state_or_multiple_choices": "本地实际转发的请求含工具、服务端状态或多个候选，已排除",
  "shadow.forwarded_unsupported_system_content": "本地实际转发的系统消息不是可比较的纯文本",
  "unsupported_history": "客户端历史不符合只追加的纯文本条件",
  "history_budget_exceeded": "客户端历史超过采样上限",
  "request_budget_exceeded": "客户端请求超过采样上限",
  "text_budget_exceeded": "客户端新增文本超过计算上限",
  "tools_server_state_or_multiple_choices": "客户端请求含工具、服务端状态或多个候选，已排除",
  "unsupported_system_content": "客户端系统消息不是可比较的纯文本",
  "shadow.non_positive_delta": "输入 token 增量不为正",
  "shadow.tokenizer_error": "本地分词器计算失败",
  "shadow.clock_regressed": "时间顺序异常",
  "shadow.insufficient_pairs": "有效配对不足，暂不能计算回归",
  "shadow.partition_mixed": "配对分区混杂，无法建立统一基线",
  "shadow.invalid_pairs": "存在无效配对数值",
  "shadow.x_variance_insufficient": "预期增量至少需 5 个不同取值且跨度不小于 32 token",
  "shadow.residuals_unstable": "残差不稳定，无法解释为固定开销",
  "shadow.intercept_unstable": "截距随样本变化，固定角色开销假设不成立",
  "shadow.slope_uncertain": "斜率区间不足以区分稳定偏离",
};

export function shadowReasonLabel(reason: string): string {
  return REASONS[reason] ?? `未完成采样或比较：${reason}`;
}

export function shadowSettingRequest(setting: ShadowAuditSetting | ShadowAuditStatus): ShadowAuditSetting {
  return {
    enabled: setting.enabled,
    providerId: setting.providerId.trim(),
    model: setting.model.trim(),
    tokenizerPath: setting.tokenizerPath.trim(),
    pretokenizer: setting.pretokenizer || null,
  };
}
