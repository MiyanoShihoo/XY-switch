import { invoke } from "@tauri-apps/api/core";
import { t } from "../i18n/language";
import type { Channel, ChannelGroupRecordedEvent, ChannelGroupsSnapshot, ChannelGroupTrialInput, ChannelGroupTrialResult, SaveChannelGroup } from "../types/channel-group";

export const channelGroupsNative = () => typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

export function channelGroupErrorText(error: unknown): string {
  const message = typeof error === "string" ? error : error instanceof Error ? error.message : "";
  if (message === "channel_groups_native_required") return t("渠道组需要桌面端本地引擎；浏览器预览不执行模拟或保存。");
  if (message === "channel_groups_offline_required") return t("桌面端未确认离线隔离或计费暂停，渠道组操作已停止。");
  if (message === "channel_groups_contract_mismatch") return t("桌面渠道组接口与当前前端不匹配，请更新桌面端后重试。");
  if (/^Command .*channel_groups_.* not found$/i.test(message)) return t("当前桌面版本尚未提供渠道组命令。");
  if (/^[a-z_]*revision_conflict$/.test(message)) return t("渠道组或共享项目预算已被其他操作修改。请保留草稿，刷新后重新编辑。");
  if (message === "channel_groups_integer_required") return t("计数、时间和金额必须是可精确表示的非负整数。");
  return t("渠道组操作失败，草稿已保留。") + (/^[a-z][a-z0-9_]{1,95}$/.test(message) ? ` (${message})` : "");
}

async function call<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  if (!channelGroupsNative()) throw new Error("channel_groups_native_required");
  return args ? invoke<T>(command, args) : invoke<T>(command);
}

function assertIntegers(value: unknown): void {
  if (typeof value === "number" && (!Number.isSafeInteger(value) || value < 0)) throw new Error("channel_groups_integer_required");
  if (Array.isArray(value)) value.forEach(assertIntegers);
  else if (value && typeof value === "object") Object.values(value).forEach(assertIntegers);
}

const isRecord = (value: unknown): value is Record<string, unknown> => Boolean(value) && typeof value === "object" && !Array.isArray(value);
const isInteger = (value: unknown): value is number => typeof value === "number" && Number.isSafeInteger(value) && value >= 0;
const isString = (value: unknown): value is string => typeof value === "string";
const isNumber = (value: unknown): value is number => typeof value === "number" && Number.isFinite(value) && value >= 0;
const nullable = (value: unknown, valid: (value: unknown) => boolean) => value === null || valid(value);
const strings = (value: unknown): value is string[] => Array.isArray(value) && value.every(isString);
const integers = (value: Record<string, unknown>, keys: string[]) => keys.every(key => isInteger(value[key]));

function usage(value: unknown): boolean {
  return isRecord(value) && integers(value, ["requests", "knownTokens", "tokenKnownRequests", "knownMicrousd", "costKnownRequests"]);
}

function channelViews(value: unknown): boolean {
  return Array.isArray(value) && value.every(channel => isRecord(channel)
    && ["groupId", "channelId", "providerId", "poolId", "health"].every(key => isString(channel[key]))
    && typeof channel.poolDeclared === "boolean" && nullable(channel.cooldownReason, isString) && nullable(channel.recoveryAtMs, isInteger)
    && integers(channel, ["rpm", "tpmReserved", "tpmUnknownRequests", "concurrent", "unknownResults"])
    && nullable(channel.errorRate, value => isNumber(value) && value <= 1) && nullable(channel.latencyMs, isNumber)
    && isRecord(channel.quota) && ["remainingRequests", "remainingTokens", "requestsResetAtMs", "tokensResetAtMs"].every(key => nullable((channel.quota as Record<string, unknown>)[key], isInteger))
    && nullable(channel.quota.source, isString) && usage(channel.recordedUsage) && usage(channel.simulatedUsage));
}

function configuration(value: unknown): boolean {
  return isRecord(value) && Array.isArray(value.groups) && Array.isArray(value.projects)
    && value.projects.every(project => isRecord(project) && isString(project.id) && isInteger(project.revision) && nullable(project.dailyMicrousd, isInteger))
    && value.groups.every(group => isRecord(group) && ["id", "name", "virtualModel"].every(key => isString(group[key]))
      && typeof group.enabled === "boolean" && ["roundRobin", "weightedRoundRobin", "leastLoad", "latencyFirst", "quotaAware"].includes(String(group.strategy))
      && nullable(group.projectId, isString) && integers(group, ["revision", "affinityTtlSeconds"]) && Array.isArray(group.channels)
      && group.channels.every(channel => isRecord(channel) && ["id", "providerId", "upstreamModel"].every(key => isString(channel[key]))
        && nullable(channel.accountId, isString) && nullable(channel.quotaPoolId, isString)
        && typeof channel.enabled === "boolean" && typeof channel.quotaIndependent === "boolean" && integers(channel, ["priority", "weight"])
        && isRecord(channel.limits) && ["rpm", "tpm", "concurrency", "dailyMicrousd"].every(key => nullable((channel.limits as Record<string, unknown>)[key], isInteger))
        && isRecord(channel.cooldown) && integers(channel.cooldown, ["rateLimitSeconds", "failureSeconds", "maxBackoffSeconds"])));
}

function canonicalChannel(channel: Channel): Channel {
  return {
    id: channel.id, providerId: channel.providerId, accountId: channel.accountId,
    upstreamModel: channel.upstreamModel, enabled: channel.enabled, priority: channel.priority, weight: channel.weight,
    quotaPoolId: channel.quotaPoolId, quotaIndependent: channel.quotaIndependent,
    limits: { rpm: channel.limits.rpm, tpm: channel.limits.tpm, concurrency: channel.limits.concurrency, dailyMicrousd: channel.limits.dailyMicrousd },
    cooldown: { rateLimitSeconds: channel.cooldown.rateLimitSeconds, failureSeconds: channel.cooldown.failureSeconds, maxBackoffSeconds: channel.cooldown.maxBackoffSeconds },
  };
}

function snapshot(value: ChannelGroupsSnapshot): ChannelGroupsSnapshot {
  if (!value || value.phase !== "offlinePilot" || !configuration(value.configuration)
    || !channelViews(value.channels) || !strings(value.warnings) || !isInteger(value.generatedAtMs)) {
    throw new Error("channel_groups_contract_mismatch");
  }
  if (value.liveEnabled !== false || value.billingHold !== true) throw new Error("channel_groups_offline_required");
  return value;
}

export async function listChannelGroups(): Promise<ChannelGroupsSnapshot> {
  return snapshot(await call<ChannelGroupsSnapshot>("channel_groups_list"));
}

export async function saveChannelGroup(input: SaveChannelGroup): Promise<ChannelGroupsSnapshot> {
  const { group, project } = input;
  const payload: SaveChannelGroup = {
    group: {
      id: group.id, name: group.name, virtualModel: group.virtualModel, enabled: group.enabled, strategy: group.strategy,
      projectId: group.projectId, affinityTtlSeconds: group.affinityTtlSeconds, channels: group.channels.map(canonicalChannel), revision: group.revision,
    },
    project: project ? { id: project.id, dailyMicrousd: project.dailyMicrousd, revision: project.revision } : null,
  };
  assertIntegers(payload);
  return snapshot(await call<ChannelGroupsSnapshot>("channel_groups_save", { input: payload }));
}

export async function setChannelGroupEnabled(groupId: string, enabled: boolean, expectedRevision: number): Promise<ChannelGroupsSnapshot> {
  assertIntegers(expectedRevision);
  return snapshot(await call<ChannelGroupsSnapshot>("channel_groups_set_enabled", { groupId, enabled, expectedRevision }));
}

export async function recordedChannelGroupEvents(groupId: string, limit = 50): Promise<ChannelGroupRecordedEvent[]> {
  assertIntegers(limit);
  if (limit < 1 || limit > 200) throw new Error("invalid_event_limit");
  const events = await call<ChannelGroupRecordedEvent[]>("channel_groups_recorded_events", { groupId, limit });
  if (!Array.isArray(events) || events.some(event => !isRecord(event) || !isInteger(event.id) || !isString(event.providerId)
    || !isInteger(event.atMs) || !["status", "latencyMs", "tokens", "costMicrousd"].every(key => nullable(event[key], isInteger))
    || !nullable(event.errorKind, isString))) throw new Error("channel_groups_contract_mismatch");
  return events;
}

export async function simulateChannelGroup(input: ChannelGroupTrialInput): Promise<ChannelGroupTrialResult> {
  assertIntegers(input);
  const result = await call<ChannelGroupTrialResult>("channel_groups_simulate", { input });
  if (!result || !Array.isArray(result.decisions) || !channelViews(result.channels) || !Array.isArray(result.replayedEventIds)
    || !result.replayedEventIds.every(isInteger) || !strings(result.warnings) || result.source !== "offlineSimulation"
    || result.decisions.some(decision => !isRecord(decision) || !isString(decision.requestId) || !isString(decision.reason)
      || !nullable(decision.channelId, isString) || !nullable(decision.upstreamModel, isString) || !Array.isArray(decision.excluded)
      || decision.excluded.some(exclusion => !isRecord(exclusion) || !isString(exclusion.channelId) || !isString(exclusion.reason) || !nullable(exclusion.retryAtMs, isInteger)))) {
    throw new Error("channel_groups_contract_mismatch");
  }
  if (result.liveDispatches !== 0 || result.ledgerWrites !== 0 || result.decisions.some(decision => decision.dispatchAllowed !== false)) {
    throw new Error("channel_groups_offline_required");
  }
  return result;
}
