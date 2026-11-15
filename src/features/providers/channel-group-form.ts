import { t } from "../../i18n/language";
import type { Channel, ChannelGroup, ChannelGroupConfiguration, ChannelGroupStrategy, ChannelGroupTrialInput, ChannelGroupRecordedEvent, SaveChannelGroup } from "../../types/channel-group";

export interface ChannelDraft {
  id: string;
  providerId: string;
  accountId: string;
  upstreamModel: string;
  enabled: boolean;
  priority: string;
  weight: string;
  quotaMode: "provider" | "shared" | "independent";
  quotaPoolId: string;
  rpm: string;
  tpm: string;
  concurrency: string;
  dailyMicrousd: string;
  rateLimitSeconds: string;
  failureSeconds: string;
  maxBackoffSeconds: string;
}

export interface ChannelGroupDraft {
  id: string;
  name: string;
  modelName: string;
  enabled: boolean;
  strategy: ChannelGroupStrategy;
  projectId: string;
  projectDailyMicrousd: string;
  projectRevision: number;
  affinityTtlSeconds: string;
  revision: number;
  channels: ChannelDraft[];
}

export const CHANNEL_GROUP_STRATEGIES = ["roundRobin", "quotaAware", "weightedRoundRobin", "leastLoad", "latencyFirst"] as const;
const configurationId = /^[A-Za-z0-9._-]{1,128}$/;
const identity = (value: string) => Boolean(value) && value.trim() === value && new TextEncoder().encode(value).length <= 256 && !/[\u0000-\u001f\u007f-\u009f]/u.test(value);

export function newChannelDraft(id: string): ChannelDraft {
  return { id, providerId: "", accountId: "", upstreamModel: "", enabled: true, priority: "0", weight: "1", quotaMode: "provider", quotaPoolId: "",
    rpm: "", tpm: "", concurrency: "", dailyMicrousd: "", rateLimitSeconds: "60", failureSeconds: "30", maxBackoffSeconds: "3600" };
}

export function newChannelGroupDraft(id: string, channelId: string): ChannelGroupDraft {
  return { id, name: "", modelName: "", enabled: true, strategy: "roundRobin", projectId: "", projectDailyMicrousd: "", projectRevision: 0,
    affinityTtlSeconds: "1800", revision: 0, channels: [newChannelDraft(channelId)] };
}

const inputNumber = (value: number | null) => value === null ? "" : String(value);

export function editChannelGroupDraft(group: ChannelGroup, configuration: ChannelGroupConfiguration): ChannelGroupDraft {
  const project = configuration.projects.find(item => item.id === group.projectId);
  return {
    id: group.id, name: group.name, modelName: group.virtualModel.slice("xy-group/".length), enabled: group.enabled, strategy: group.strategy, projectId: group.projectId ?? "",
    projectDailyMicrousd: inputNumber(project?.dailyMicrousd ?? null), projectRevision: project?.revision ?? 0,
    affinityTtlSeconds: String(group.affinityTtlSeconds), revision: group.revision,
    channels: group.channels.map(channel => ({ id: channel.id, providerId: channel.providerId, accountId: channel.accountId ?? "", upstreamModel: channel.upstreamModel,
      enabled: channel.enabled, priority: String(channel.priority), weight: String(channel.weight),
      quotaMode: channel.quotaPoolId ? "shared" : channel.quotaIndependent ? "independent" : "provider", quotaPoolId: channel.quotaPoolId ?? "",
      rpm: inputNumber(channel.limits.rpm), tpm: inputNumber(channel.limits.tpm), concurrency: inputNumber(channel.limits.concurrency), dailyMicrousd: inputNumber(channel.limits.dailyMicrousd),
      rateLimitSeconds: String(channel.cooldown.rateLimitSeconds), failureSeconds: String(channel.cooldown.failureSeconds), maxBackoffSeconds: String(channel.cooldown.maxBackoffSeconds),
    })),
  };
}

export function changeDraftProject(draft: ChannelGroupDraft, projectId: string, configuration: ChannelGroupConfiguration): ChannelGroupDraft {
  const project = configuration.projects.find(item => item.id === projectId.trim());
  return { ...draft, projectId, projectDailyMicrousd: inputNumber(project?.dailyMicrousd ?? null), projectRevision: project?.revision ?? 0 };
}

export type ChannelGroupFormErrors = Record<string, string>;
export type ChannelGroupFormResult = { input: SaveChannelGroup; errors: null } | { input: null; errors: ChannelGroupFormErrors };

export function buildChannelGroupSave(draft: ChannelGroupDraft, configuration: ChannelGroupConfiguration): ChannelGroupFormResult {
  const errors: ChannelGroupFormErrors = {};
  const integer = (value: string, path: string, optional = false, min = 0, max = 1_000_000_000_000): number | null => {
    const trimmed = value.trim();
    if (optional && !trimmed) return null;
    const parsed = Number(trimmed);
    if (!/^\d+$/.test(trimmed) || !Number.isSafeInteger(parsed) || parsed < min || parsed > max) {
      errors[path] = t("请输入 {min} 至 {max} 之间的整数。", { min, max });
      return null;
    }
    return parsed;
  };
  const name = draft.name.trim();
  const modelName = draft.modelName.trim();
  const otherGroups = configuration.groups.filter(group => group.id !== draft.id);
  const otherChannelIds = new Set(otherGroups.flatMap(group => group.channels.map(channel => channel.id)));
  if (!configurationId.test(draft.id)) errors.form = t("组与渠道 ID 须为 1–128 个 ASCII 字母、数字、点、下划线或连字符。");
  if (!Number.isSafeInteger(draft.revision) || draft.revision < 0 || draft.revision >= 999_999_999_999
    || !Number.isSafeInteger(draft.projectRevision) || draft.projectRevision < 0 || draft.projectRevision >= 999_999_999_999) errors.form = t("配置修订号无效，请刷新后重新编辑。");
  if (otherGroups.length >= 64 || otherChannelIds.size + draft.channels.length > 512) errors.form = t("配置最多包含 64 个组、128 个项目和 512 个渠道。");
  if (!identity(name)) errors.name = t("请填写组名，最多 256 字节且不含控制字符。");
  if (!/^[A-Za-z0-9._-]{1,128}$/.test(modelName)) errors.modelName = t("模型别名须为 1–128 个字母、数字、点、下划线或连字符。");
  if (configuration.groups.some(group => group.id !== draft.id && group.virtualModel === `xy-group/${modelName}`)) errors.modelName = t("此虚拟模型已存在。");
  if (!CHANNEL_GROUP_STRATEGIES.includes(draft.strategy)) errors.strategy = t("请选择支持的渠道策略。");
  const projectId = draft.projectId.trim();
  if (projectId && !/^[A-Za-z0-9._-]{1,128}$/.test(projectId)) errors.projectId = t("项目 ID 须为 1–128 个字母、数字、点、下划线或连字符。");
  if (projectId && !configuration.projects.some(project => project.id === projectId) && configuration.projects.length >= 128) errors.projectId = t("配置最多包含 64 个组、128 个项目和 512 个渠道。");
  const projectDaily = projectId ? integer(draft.projectDailyMicrousd, "projectDailyMicrousd", true) : null;
  const affinity = integer(draft.affinityTtlSeconds, "affinityTtlSeconds", false, 60, 2_592_000);
  if (!draft.channels.length) errors.channels = t("至少添加一个渠道引用。");
  if (draft.channels.length > 32) errors.channels = t("每个渠道组最多包含 32 个渠道。");
  const channels: Channel[] = draft.channels.map((channel, index) => {
    const key = `channels.${index}.`;
    if (!configurationId.test(channel.id)) errors.form = t("组与渠道 ID 须为 1–128 个 ASCII 字母、数字、点、下划线或连字符。");
    if (otherChannelIds.has(channel.id)) errors.channels = t("渠道 ID 已存在，请为每个引用使用不同 ID。");
    otherChannelIds.add(channel.id);
    if (!identity(channel.providerId.trim())) errors[key + "providerId"] = t("请选择供应商引用。");
    if (channel.accountId && !identity(channel.accountId.trim())) errors[key + "accountId"] = t("账号引用无效，请重新选择。");
    if (!identity(channel.upstreamModel.trim())) errors[key + "upstreamModel"] = t("请填写上游模型 ID，最多 256 字节且不含控制字符。");
    if (channel.upstreamModel.trim().startsWith("xy-group/")) errors[key + "upstreamModel"] = t("渠道必须引用上游模型，不能嵌套渠道组。");
    if (channel.quotaMode === "shared" && !/^[A-Za-z0-9._-]{1,128}$/.test(channel.quotaPoolId.trim())) errors[key + "quotaPoolId"] = t("额度池 ID 须为 1–128 个字母、数字、点、下划线或连字符。");
    if (!["provider", "shared", "independent"].includes(channel.quotaMode)) errors[key + "quotaMode"] = t("请选择额度共享方式。");
    const priority = integer(channel.priority, key + "priority", false, 0, 65535);
    const weight = integer(channel.weight, key + "weight", false, 1, 1000);
    const rateLimit = integer(channel.rateLimitSeconds, key + "rateLimitSeconds", false, 1, 86_400);
    const failure = integer(channel.failureSeconds, key + "failureSeconds", false, 1, 86_400);
    const maxBackoff = integer(channel.maxBackoffSeconds, key + "maxBackoffSeconds", false, 1, 604_800);
    if (maxBackoff !== null && maxBackoff < (rateLimit ?? 0)) errors[key + "maxBackoffSeconds"] = t("最大退避不能小于限流冷却时间。");
    return {
      id: channel.id, providerId: channel.providerId.trim(), accountId: channel.accountId.trim() || null, upstreamModel: channel.upstreamModel.trim(), enabled: channel.enabled,
      priority: priority ?? 0, weight: weight ?? 1, quotaPoolId: channel.quotaMode === "shared" ? channel.quotaPoolId.trim() : null, quotaIndependent: channel.quotaMode === "independent",
      limits: { rpm: integer(channel.rpm, key + "rpm", true, 1), tpm: integer(channel.tpm, key + "tpm", true, 1), concurrency: integer(channel.concurrency, key + "concurrency", true, 1, 100_000), dailyMicrousd: integer(channel.dailyMicrousd, key + "dailyMicrousd", true) },
      cooldown: { rateLimitSeconds: rateLimit ?? 0, failureSeconds: failure ?? 0, maxBackoffSeconds: maxBackoff ?? 0 },
    };
  });
  if (Object.keys(errors).length) return { input: null, errors };
  return { errors: null, input: {
    group: { id: draft.id, name, virtualModel: `xy-group/${modelName}`, enabled: draft.enabled, strategy: draft.strategy, projectId: projectId || null, affinityTtlSeconds: affinity!, channels, revision: draft.revision },
    project: projectId ? { id: projectId, dailyMicrousd: projectDaily, revision: draft.projectRevision } : null,
  } };
}

export function deterministicChannelTrial(groupId: string, snapshotAtMs: number, replay: readonly ChannelGroupRecordedEvent[] = []): ChannelGroupTrialInput {
  const ordered = [...replay].sort((left, right) => left.atMs - right.atMs || left.id - right.id);
  const atMs = Math.max(snapshotAtMs, ...ordered.map(event => event.atMs + 1));
  if (!Number.isSafeInteger(atMs) || atMs < 0 || !Number.isSafeInteger(atMs + 5000) || ordered.some(event => !Number.isSafeInteger(event.id) || event.id < 0)) {
    throw new Error("channel_groups_integer_required");
  }
  return {
    groupId, replayEventIds: [...new Set(ordered.map(event => event.id))],
    steps: Array.from({ length: 6 }, (_, index) => ({ kind: "request", atMs: atMs + index * 1000, request: {
      id: `offline-${index + 1}`, groupId, reserveTokens: 128, reserveMicrousd: 1000, stateful: false, sessionKey: null, previousResponseId: null, conversationId: null,
    } })),
  };
}
