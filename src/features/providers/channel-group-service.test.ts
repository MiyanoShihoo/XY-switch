import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { setLanguagePreference } from "../../i18n/language";
import { channelGroupErrorText, channelGroupsNative, listChannelGroups, recordedChannelGroupEvents, saveChannelGroup, setChannelGroupEnabled, simulateChannelGroup } from "../../lib/channel-group-service";
import type { ChannelGroupsSnapshot, SaveChannelGroup } from "../../types/channel-group";
import { deterministicChannelTrial } from "./channel-group-form";
import { event, group, snapshot, trial, view } from "./channel-group-test-fixtures";

const { invoke, network } = vi.hoisted(() => ({ invoke: vi.fn(), network: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke }));

beforeEach(async () => {
  vi.unstubAllGlobals();
  await setLanguagePreference("zh-CN");
  vi.resetAllMocks();
  vi.stubGlobal("window", { __TAURI_INTERNALS__: {} });
  vi.stubGlobal("fetch", network.mockRejectedValue(new Error("Network is forbidden in this test")));
});
afterEach(() => { expect(network).not.toHaveBeenCalled(); vi.unstubAllGlobals(); });

describe("offline channel group IPC boundary", () => {
  it("uses all five exact registered commands and their camelCase envelopes", async () => {
    const saved = snapshot();
    const input: SaveChannelGroup = { group: group(), project: null };
    const simulation = deterministicChannelTrial("group-a", saved.generatedAtMs);
    invoke.mockResolvedValueOnce(saved).mockResolvedValueOnce(saved).mockResolvedValueOnce(saved).mockResolvedValueOnce([event()]).mockResolvedValueOnce(trial());
    expect(await listChannelGroups()).toEqual(saved);
    expect(await saveChannelGroup(input)).toEqual(saved);
    expect(await setChannelGroupEnabled("group-a", false, 2)).toEqual(saved);
    expect(await recordedChannelGroupEvents("group-a")).toEqual([event()]);
    expect(await simulateChannelGroup(simulation)).toEqual(trial());
    expect(invoke.mock.calls).toEqual([
      ["channel_groups_list"], ["channel_groups_save", { input }],
      ["channel_groups_set_enabled", { groupId: "group-a", enabled: false, expectedRevision: 2 }],
      ["channel_groups_recorded_events", { groupId: "group-a", limit: 50 }],
      ["channel_groups_simulate", { input: simulation }],
    ]);
  });

  it("rejects every browser operation before IPC and never manufactures an engine result", async () => {
    vi.stubGlobal("window", {});
    expect(channelGroupsNative()).toBe(false);
    for (const operation of [
      () => listChannelGroups(), () => saveChannelGroup({ group: group(), project: null }),
      () => setChannelGroupEnabled("group-a", true, 2), () => recordedChannelGroupEvents("group-a"),
      () => simulateChannelGroup(deterministicChannelTrial("group-a", 0)),
    ]) await expect(operation()).rejects.toThrow("channel_groups_native_required");
    expect(invoke).not.toHaveBeenCalled();
    vi.stubGlobal("window", undefined);
    expect(channelGroupsNative()).toBe(false);
  });

  it("saves references only, strips unrelated secret fields, and preserves both revisions", async () => {
    const canonical: SaveChannelGroup = { group: group({ projectId: "shared", revision: 8 }), project: { id: "shared", dailyMicrousd: 0, revision: 5 } };
    const input = structuredClone(canonical);
    Object.assign(input, { apiKey: "NEVER_SEND_TOP_SECRET" });
    Object.assign(input.group, { provider: { token: "NEVER_SEND_PROVIDER_SECRET" } });
    Object.assign(input.group.channels[0], { credentials: "NEVER_SEND_CHANNEL_SECRET" });
    Object.assign(input.group.channels[0].limits, { secret: "NEVER_SEND_LIMIT_SECRET" });
    Object.assign(input.project!, { secret: "NEVER_SEND_PROJECT_SECRET" });
    invoke.mockResolvedValue(snapshot());
    await saveChannelGroup(input);
    expect(invoke).toHaveBeenCalledExactlyOnceWith("channel_groups_save", { input: canonical });
    expect(JSON.stringify(invoke.mock.calls)).not.toContain("SECRET");
  });

  it.each([-1, 0.25, Number.MAX_SAFE_INTEGER + 1, NaN, Infinity])("rejects an inexact/invalid outgoing number (%s) before IPC", async value => {
    const input: SaveChannelGroup = { group: group(), project: null };
    input.group.channels[0].limits.dailyMicrousd = value;
    await expect(saveChannelGroup(input)).rejects.toThrow("channel_groups_integer_required");
    await expect(setChannelGroupEnabled("group-a", false, value)).rejects.toThrow("channel_groups_integer_required");
    const simulation = deterministicChannelTrial("group-a", 0);
    simulation.steps[0].atMs = value;
    await expect(simulateChannelGroup(simulation)).rejects.toThrow("channel_groups_integer_required");
    expect(invoke).not.toHaveBeenCalled();
  });

  it("retains nullable quota and unknown usage while accepting fractional health observations", async () => {
    const result = snapshot({ channels: [view({ errorRate: 1 / 3, latencyMs: 12.75, tpmUnknownRequests: 2 })] });
    invoke.mockResolvedValue(result);
    expect(await listChannelGroups()).toEqual(result);
    expect(result.channels[0].quota.remainingTokens).toBeNull();
    expect(result.channels[0].recordedUsage.costKnownRequests).toBe(0);
  });

  it.each([
    ["phase", "live"], ["generatedAtMs", Number.MAX_SAFE_INTEGER + 1], ["channels", null],
    ["configuration", { groups: [], projects: null }], ["warnings", [false]],
  ])("rejects a malformed snapshot field %s", async (field, value) => {
    invoke.mockResolvedValue({ ...snapshot(), [field]: value });
    await expect(listChannelGroups()).rejects.toThrow("channel_groups_contract_mismatch");
  });

  it.each(["poolDeclared", "tpmUnknownRequests"] as const)("requires the current ChannelView field %s", async field => {
    const oldView: Record<string, unknown> = { ...view(), poolVerified: true };
    delete oldView[field];
    invoke.mockResolvedValue({ ...snapshot(), channels: [oldView] });
    await expect(listChannelGroups()).rejects.toThrow("channel_groups_contract_mismatch");
  });

  it.each([{ errorRate: 1.1 }, { latencyMs: Infinity }, { tpmUnknownRequests: -1 }, { rpm: 0.1 }])("rejects invalid channel metrics %j", async patch => {
    invoke.mockResolvedValue(snapshot({ channels: [view(patch)] }));
    await expect(listChannelGroups()).rejects.toThrow("channel_groups_contract_mismatch");
  });

  it.each([{ liveEnabled: true }, { billingHold: false }])("requires the offline/billing flags %j", async patch => {
    invoke.mockResolvedValue({ ...snapshot(), ...patch });
    await expect(listChannelGroups()).rejects.toThrow("channel_groups_offline_required");
  });

  it("enforces offline flags on mutation replies as well as list replies", async () => {
    invoke.mockResolvedValue({ ...snapshot(), liveEnabled: true } as unknown as ChannelGroupsSnapshot);
    await expect(saveChannelGroup({ group: group(), project: null })).rejects.toThrow("channel_groups_offline_required");
    await expect(setChannelGroupEnabled("group-a", true, 2)).rejects.toThrow("channel_groups_offline_required");
  });

  it.each([0, 201, 1.5])("rejects recorded-event limit %s without IPC", async limit => {
    await expect(recordedChannelGroupEvents("group-a", limit)).rejects.toThrow();
    expect(invoke).not.toHaveBeenCalled();
  });

  it("accepts unknown recorded fields without converting them to zero", async () => {
    invoke.mockResolvedValue([event()]);
    expect(await recordedChannelGroupEvents("group-a", 200)).toEqual([event()]);
    expect(invoke).toHaveBeenCalledWith("channel_groups_recorded_events", { groupId: "group-a", limit: 200 });
  });

  it.each([{ atMs: undefined }, { costMicrousd: 0.5 }, { tokens: -1 }, { providerId: null }])("rejects malformed recorded event %j", async patch => {
    invoke.mockResolvedValue([{ ...event(), ...patch }]);
    await expect(recordedChannelGroupEvents("group-a")).rejects.toThrow("channel_groups_contract_mismatch");
  });

  it.each([{ liveDispatches: 1 }, { ledgerWrites: 1 }, { decisions: [{ ...trial().decisions[0], dispatchAllowed: true }] }])("rejects unsafe simulation evidence %j", async patch => {
    invoke.mockResolvedValue({ ...trial(), ...patch });
    await expect(simulateChannelGroup(deterministicChannelTrial("group-a", 0))).rejects.toThrow("channel_groups_offline_required");
  });

  it.each([{ source: "browserMock" }, { replayedEventIds: [0.5] }, { decisions: [{ ...trial().decisions[0], excluded: [{}] }] }])("rejects malformed simulation evidence %j", async patch => {
    invoke.mockResolvedValue({ ...trial(), ...patch });
    await expect(simulateChannelGroup(deterministicChannelTrial("group-a", 0))).rejects.toThrow("channel_groups_contract_mismatch");
  });

  it("describes revision conflicts and missing commands without displaying arbitrary backend secrets", () => {
    expect(channelGroupErrorText("project_revision_conflict")).toContain("保留草稿");
    expect(channelGroupErrorText(new Error("group_revision_conflict"))).toContain("刷新后重新编辑");
    expect(channelGroupErrorText("Command channel_groups_list not found")).toContain("尚未提供渠道组命令");
    expect(channelGroupErrorText(new Error("https://private.invalid?apiKey=DO_NOT_RENDER_SECRET"))).not.toContain("DO_NOT_RENDER_SECRET");
    expect(channelGroupErrorText("invalid_channel_weight")).toContain("invalid_channel_weight");
  });
});
