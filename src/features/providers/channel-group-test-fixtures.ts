import { createProviderAdvanced } from "../../lib/providers";
import type { Channel, ChannelGroup, ChannelGroupRecordedEvent, ChannelGroupsSnapshot, ChannelGroupTrialResult, ChannelView } from "../../types/channel-group";
import type { Provider } from "../../types/provider";
import { newChannelGroupDraft } from "./channel-group-form";

export function channel(change: Partial<Channel> = {}): Channel {
  return { id: "channel-a", providerId: "relay", accountId: null, upstreamModel: "vendor/model:2026", enabled: true,
    priority: 0, weight: 1, quotaPoolId: null, quotaIndependent: false,
    limits: { rpm: null, tpm: null, concurrency: null, dailyMicrousd: null },
    cooldown: { rateLimitSeconds: 60, failureSeconds: 30, maxBackoffSeconds: 3600 }, ...change };
}

export function group(change: Partial<ChannelGroup> = {}): ChannelGroup {
  return { id: "group-a", name: "Research", virtualModel: "xy-group/research", enabled: true, strategy: "roundRobin",
    projectId: null, affinityTtlSeconds: 1800, channels: [channel()], revision: 2, ...change };
}

export function view(change: Partial<ChannelView> = {}): ChannelView {
  return { groupId: "group-a", channelId: "channel-a", providerId: "relay", poolId: "origin:relay", poolDeclared: false,
    health: "unobserved", cooldownReason: null, recoveryAtMs: null, rpm: 0, tpmReserved: 0, tpmUnknownRequests: 0,
    concurrent: 0, errorRate: null, latencyMs: null,
    quota: { remainingRequests: null, remainingTokens: null, requestsResetAtMs: null, tokensResetAtMs: null, source: null },
    recordedUsage: { requests: 3, knownTokens: 128, tokenKnownRequests: 1, knownMicrousd: 0, costKnownRequests: 0 },
    simulatedUsage: { requests: 0, knownTokens: 0, tokenKnownRequests: 0, knownMicrousd: 0, costKnownRequests: 0 }, unknownResults: 0, ...change };
}

export function snapshot(change: Partial<ChannelGroupsSnapshot> = {}): ChannelGroupsSnapshot {
  return { phase: "offlinePilot", liveEnabled: false, billingHold: true, configuration: { groups: [group()], projects: [] },
    channels: [view()], warnings: [], generatedAtMs: 1_800_000_000_000, ...change };
}

export function trial(change: Partial<ChannelGroupTrialResult> = {}): ChannelGroupTrialResult {
  return { source: "offlineSimulation", liveDispatches: 0, ledgerWrites: 0, replayedEventIds: [], channels: [view()], warnings: [],
    decisions: [{ requestId: "offline-1", channelId: "channel-a", upstreamModel: "vendor/model:2026", reason: "selected_for_simulation", excluded: [], dispatchAllowed: false }], ...change };
}

export function event(change: Partial<ChannelGroupRecordedEvent> = {}): ChannelGroupRecordedEvent {
  return { id: 7, providerId: "relay", atMs: 1_800_000_000_000, status: null, latencyMs: null, tokens: null, costMicrousd: null, errorKind: null, ...change };
}

export function draft() {
  const value = newChannelGroupDraft("group-new", "channel-new");
  value.name = "Research"; value.modelName = "research";
  value.channels[0].providerId = "relay"; value.channels[0].upstreamModel = "vendor/model:2026";
  return value;
}

export function provider(): Provider {
  return { id: "relay", name: "Relay", group: "Codex", endpoint: "https://qa.invalid/v1", model: "vendor/model:2026", enabled: true,
    icon: "/app-icon.png", advanced: { ...createProviderAdvanced("openai_chat"),
      headerOverrides: { Authorization: "DO_NOT_RENDER_HEADER_SECRET" }, bodyOverrides: { key: "DO_NOT_RENDER_BODY_SECRET" } } };
}
