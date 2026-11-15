import { localeTag, t } from "../../i18n/language";
import type { ChannelGroup, ChannelGroupStrategy, ChannelView } from "../../types/channel-group";

export function channelStrategyLabel(strategy: ChannelGroupStrategy): string {
  switch (strategy) {
    case "roundRobin": return t("轮询");
    case "quotaAware": return t("额度感知");
    case "weightedRoundRobin": return t("加权轮询");
    case "leastLoad": return t("最小负载");
    case "latencyFirst": return t("延迟优先");
  }
}

export function channelStrategyDescription(strategy: ChannelGroupStrategy): string {
  switch (strategy) {
    case "roundRobin": return t("在同优先级的可用渠道间依次轮转。");
    case "quotaAware": return t("优先有已知剩余额度的渠道；缺少额度证据时保持未知。");
    case "weightedRoundRobin": return t("按渠道权重分配同优先级的选择机会。");
    case "leastLoad": return t("比较当前预留并发与渠道并发上限。");
    case "latencyFirst": return t("参考本地观测延迟；没有观测的渠道保留未知状态。");
  }
}

export function channelNumber(value: number | null | undefined): string {
  return value == null || !Number.isFinite(value) ? t("未知") : value.toLocaleString(localeTag());
}

export function channelTime(atMs: number | null | undefined): string {
  if (atMs == null || !Number.isFinite(atMs) || !Number.isFinite(new Date(atMs).getTime())) return t("未知");
  return new Date(atMs).toLocaleString(localeTag(), { hour12: false });
}

export function channelMoney(microusd: number | null | undefined): string {
  return microusd == null ? t("未知") : `${channelNumber(microusd)} μUSD`;
}

export function recordedProviderUsage(group: ChannelGroup, channels: ChannelView[]) {
  return [...new Set(group.channels.map(channel => channel.providerId))].map(providerId => ({
    providerId,
    usage: channels.find(channel => channel.groupId === group.id && channel.providerId === providerId)?.recordedUsage,
  }));
}

export function channelReason(reason: string): string {
  const labels: Record<string, string> = {
    group_disabled: t("组已停用"), channel_disabled: t("渠道已停用"), provider_disabled: t("供应商已停用"), provider_missing: t("供应商引用缺失"),
    account_missing: t("账号引用缺失"), account_unavailable: t("账号不可用"), cooling_down: t("冷却中"), quota_exhausted: t("额度耗尽"),
    unobserved: t("尚无观测"), unknown_result: t("结果未知"), degraded: t("存在失败观测"), ready: t("本地条件可用"),
    group_missing: t("渠道组不存在"), no_eligible_channel: t("没有符合条件的渠道"), selected: t("已选中（仅模拟）"),
    rpm_limit: t("达到请求频率限制"), tpm_limit: t("达到 Token 频率限制"), concurrency_limit: t("达到并发限制"),
    project_budget: t("达到共享项目预算"), channel_budget: t("达到渠道日预算"), affinity_unavailable: t("亲和渠道不可用"),
    selected_for_simulation: t("已选中（仅模拟）"), session_affinity: t("沿用会话亲和渠道（仅模拟）"),
    model_not_available_to_account: t("账号未声明支持该模型"), explicit_account_required: t("此供应商需要明确选择账号"),
    token_window_unknown: t("Token 窗口包含用量未知的请求"), request_quota_exhausted: t("请求额度耗尽"), token_quota_exhausted: t("Token 额度不足"),
    channel_budget_exhausted: t("达到渠道日预算"), project_budget_exhausted: t("达到共享项目预算"),
    channel_cost_unknown: t("渠道存在金额未知的请求"), project_cost_unknown: t("共享项目存在金额未知的请求"),
    request_cost_unknown: t("请求预留金额未知"), rate_limited: t("触发限流冷却"), upstream_result_unknown: t("上游结果未知，采用失败冷却"),
    account_cooling_down: t("已记录账号冷却"), authentication_disabled: t("历史认证失败"),
    cost_reservation_required: t("预算检查需要明确的预留金额"), unresolved_ledger_cost: t("账本存在金额未知的请求"),
    stateful_identity_required: t("有状态请求缺少会话标识"), affinity_expired: t("会话亲和已过期"), affinity_conflict: t("会话亲和冲突"),
    affinity_unknown: t("会话亲和未知"), request_already_considered: t("请求已参与过选择"),
    offlineSimulation: t("桌面本地离线模拟"), recordedAccountSnapshot: t("已记录账号额度快照"),
    recordedResponse: t("已记录供应商响应"), simulatedResponse: t("合成响应"),
  };
  return labels[reason] ?? reason;
}

export function channelWarning(warning: string): string {
  const labels: Record<string, string> = {
    same_origin_shared_conservatively: t("相同来源的渠道按共享额度保守处理。"),
    quota_relationship_unknown_shared_conservatively: t("额度关系未知，按共享额度保守处理。"),
    account_usage_attributed_to_provider_conservatively: t("账号用量按供应商级账本保守计算，无法归属到具体渠道。"),
    provider_history_is_not_group_attributed: t("历史记录属于供应商，不能归因到渠道或渠道组。"),
    routing_metrics_are_replay_and_simulation_not_live_counters: t("路由计数来自回放与模拟，不是实时请求计数。"),
    superapi_525_billing_unresolved_live_dispatch_disabled: t("superapi 525 计费问题尚未解决，计费测试已暂停，实时派发已禁用。"),
    health_history_limited_to_latest_2048_events: t("健康状态只回放最近 2,048 条事件；账本用量包含全部已记录请求。"),
    recorded_headers_unavailable_configured_cooldown_used: t("历史记录不含响应头，限流恢复时间使用配置的冷却策略。"),
  };
  return labels[warning] ?? warning;
}
