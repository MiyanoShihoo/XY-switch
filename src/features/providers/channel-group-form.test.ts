import { describe, expect, it } from "vitest";
import type { ChannelGroupConfiguration } from "../../types/channel-group";
import { buildChannelGroupSave, changeDraftProject, CHANNEL_GROUP_STRATEGIES, deterministicChannelTrial, editChannelGroupDraft, type ChannelDraft, type ChannelGroupDraft } from "./channel-group-form";
import { channel, draft, event, group } from "./channel-group-test-fixtures";

const empty: ChannelGroupConfiguration = { groups: [], projects: [] };

describe("channel group save form", () => {
  it("builds a reference-only model alias and keeps absent limits nullable", () => {
    const result = buildChannelGroupSave(draft(), empty);
    expect(result.errors).toBeNull();
    expect(result.input?.group).toMatchObject({ id: "group-new", virtualModel: "xy-group/research", revision: 0, strategy: "roundRobin", projectId: null });
    expect(result.input?.project).toBeNull();
    expect(result.input?.group.channels[0]).toMatchObject({ providerId: "relay", accountId: null, upstreamModel: "vendor/model:2026",
      limits: { rpm: null, tpm: null, concurrency: null, dailyMicrousd: null }, quotaPoolId: null, quotaIndependent: false });
  });

  it.each(CHANNEL_GROUP_STRATEGIES)("preserves the supported %s strategy", strategy => {
    expect(buildChannelGroupSave({ ...draft(), strategy }, empty).input?.group.strategy).toBe(strategy);
  });

  it("distinguishes a missing budget from a zero budget on both project and channel", () => {
    const value = { ...draft(), projectId: "shared" };
    expect(buildChannelGroupSave(value, empty).input?.project?.dailyMicrousd).toBeNull();
    value.projectDailyMicrousd = "0"; value.channels[0].dailyMicrousd = "0";
    const result = buildChannelGroupSave(value, empty);
    expect(result.errors).toBeNull();
    expect(result.input?.project?.dailyMicrousd).toBe(0);
    expect(result.input?.group.channels[0].limits.dailyMicrousd).toBe(0);
  });

  it.each([
    ["priority", "0", "65535", "65536"], ["weight", "1", "1000", "0"],
    ["rpm", "1", "1000000000000", "0"], ["tpm", "1", "1000000000000", "1000000000001"],
    ["concurrency", "1", "100000", "100001"], ["dailyMicrousd", "0", "1000000000000", "1000000000001"],
    ["rateLimitSeconds", "1", "86400", "86401"], ["failureSeconds", "1", "86400", "0"],
    ["maxBackoffSeconds", "60", "604800", "604801"],
  ] as const)("checks native bounds for %s without rounding", (field, minimum, maximum, invalid) => {
    for (const valid of [minimum, maximum]) {
      const value = draft(); value.channels[0].maxBackoffSeconds = "604800"; value.channels[0][field] = valid;
      expect(buildChannelGroupSave(value, empty).errors, `${field}=${valid}`).toBeNull();
    }
    const value = draft(); value.channels[0][field] = invalid;
    expect(buildChannelGroupSave(value, empty).errors?.[`channels.0.${field}`]).toBeTruthy();
  });

  it.each(["1.5", "1e3", "-1", "NaN", "Infinity", "9007199254740993"])("rejects noninteger text %s", amount => {
    const value = draft(); value.channels[0].dailyMicrousd = amount;
    expect(buildChannelGroupSave(value, empty).errors?.["channels.0.dailyMicrousd"]).toBeTruthy();
  });

  it("enforces affinity TTL and shared project monetary bounds", () => {
    for (const ttl of ["60", "2592000"]) expect(buildChannelGroupSave({ ...draft(), affinityTtlSeconds: ttl }, empty).errors).toBeNull();
    for (const ttl of ["59", "2592001"]) expect(buildChannelGroupSave({ ...draft(), affinityTtlSeconds: ttl }, empty).errors?.affinityTtlSeconds).toBeTruthy();
    const value = { ...draft(), projectId: "shared", projectDailyMicrousd: "1000000000000" };
    expect(buildChannelGroupSave(value, empty).errors).toBeNull();
    value.projectDailyMicrousd = "1000000000001";
    expect(buildChannelGroupSave(value, empty).errors?.projectDailyMicrousd).toBeTruthy();
  });

  it("does not allow maximum backoff shorter than rate-limit cooldown", () => {
    const value = draft(); value.channels[0].rateLimitSeconds = "61"; value.channels[0].maxBackoffSeconds = "60";
    expect(buildChannelGroupSave(value, empty).errors?.["channels.0.maxBackoffSeconds"]).toBeTruthy();
  });

  it.each(["模型", "model/name", "model:name", "name with space", "x".repeat(129)])("rejects an invalid virtual-model suffix %s", modelName => {
    expect(buildChannelGroupSave({ ...draft(), modelName }, empty).errors?.modelName).toBeTruthy();
  });

  it("accepts the full ASCII suffix alphabet but rejects nested group references", () => {
    const value = { ...draft(), modelName: "A_z.9-".repeat(20) };
    expect(buildChannelGroupSave(value, empty).errors).toBeNull();
    value.channels[0].upstreamModel = "xy-group/another";
    expect(buildChannelGroupSave(value, empty).errors?.["channels.0.upstreamModel"]).toBeTruthy();
  });

  it("enforces UTF-8 byte lengths and Unicode control rejection on ordinary identities", () => {
    const value = draft(); value.name = "名".repeat(85); value.channels[0].upstreamModel = "m".repeat(256);
    expect(buildChannelGroupSave(value, empty).errors).toBeNull();
    value.name += "名"; value.channels[0].upstreamModel += "m";
    expect(buildChannelGroupSave(value, empty).errors).toHaveProperty("name");
    expect(buildChannelGroupSave(value, empty).errors?.["channels.0.upstreamModel"]).toBeTruthy();
    for (const key of ["providerId", "accountId", "upstreamModel"] as const) {
      const control = draft(); control.channels[0][key] = "a\u0085b";
      expect(buildChannelGroupSave(control, empty).errors?.[`channels.0.${key}`]).toBeTruthy();
    }
  });

  it.each(["id", "projectId"] as const)("validates the ASCII and length constraints for %s", field => {
    for (const invalid of ["bad/id", "项目", "x".repeat(129)]) {
      const value = { ...draft(), [field]: invalid };
      expect(buildChannelGroupSave(value, empty).errors?.[field === "id" ? "form" : field]).toBeTruthy();
    }
    expect(buildChannelGroupSave({ ...draft(), [field]: "x".repeat(128) }, empty).errors).toBeNull();
  });

  it("enforces channel count and unique channel IDs across the entire configuration", () => {
    const value = draft();
    expect(buildChannelGroupSave({ ...value, channels: [] }, empty).errors?.channels).toBeTruthy();
    value.channels = Array.from({ length: 32 }, (_, index) => ({ ...value.channels[0], id: `channel-${index}` }));
    expect(buildChannelGroupSave(value, empty).errors).toBeNull();
    expect(buildChannelGroupSave({ ...value, channels: [...value.channels, { ...value.channels[0], id: "extra" }] }, empty).errors?.channels).toBeTruthy();
    expect(buildChannelGroupSave({ ...value, channels: [value.channels[0], value.channels[0]] }, empty).errors?.channels).toBeTruthy();
    const configuration = { ...empty, groups: [group({ virtualModel: "xy-group/other", channels: [channel({ id: value.channels[0].id })] })] };
    expect(buildChannelGroupSave(value, configuration).errors?.channels).toBeTruthy();
  });

  it("rejects duplicate model aliases while permitting an unchanged saved alias", () => {
    const saved = group(); const configuration = { ...empty, groups: [saved] };
    expect(buildChannelGroupSave(draft(), configuration).errors?.modelName).toBeTruthy();
    expect(buildChannelGroupSave(editChannelGroupDraft(saved, configuration), configuration).errors).toBeNull();
  });

  it("enforces total configuration limits without blocking edits at the group limit", () => {
    const groups = Array.from({ length: 64 }, (_, index) => group({ id: `g-${index}`, virtualModel: `xy-group/m-${index}`, channels: [channel({ id: `c-${index}` })] }));
    const configuration = { ...empty, groups };
    expect(buildChannelGroupSave(draft(), configuration).errors?.form).toBeTruthy();
    expect(buildChannelGroupSave(editChannelGroupDraft(groups[0], configuration), configuration).errors).toBeNull();
    const projects = Array.from({ length: 128 }, (_, index) => ({ id: `p-${index}`, revision: 1, dailyMicrousd: null }));
    expect(buildChannelGroupSave({ ...draft(), projectId: "extra" }, { ...empty, projects }).errors?.projectId).toBeTruthy();
    const fullChannels = Array.from({ length: 16 }, (_, index) => group({ id: `g-${index}`, virtualModel: `xy-group/m-${index}`,
      channels: Array.from({ length: 32 }, (_, subIndex) => channel({ id: `c-${index}-${subIndex}` })) }));
    expect(buildChannelGroupSave(draft(), { ...empty, groups: fullChannels }).errors?.form).toBeTruthy();
  });

  it.each(["provider", "shared", "independent"] as const)("writes mutually exclusive quota flags for %s mode", quotaMode => {
    const value = draft(); value.channels[0].quotaMode = quotaMode; value.channels[0].quotaPoolId = "declared-pool";
    const saved = buildChannelGroupSave(value, empty).input?.group.channels[0];
    expect(saved?.quotaPoolId).toBe(quotaMode === "shared" ? "declared-pool" : null);
    expect(saved?.quotaIndependent).toBe(quotaMode === "independent");
    expect(saved).not.toHaveProperty("poolVerified");
  });

  it("validates declared quota-pool IDs and invalid option values", () => {
    const value = draft(); value.channels[0].quotaMode = "shared";
    for (const quotaPoolId of ["", "bad/id", "x".repeat(129)]) {
      value.channels[0].quotaPoolId = quotaPoolId;
      expect(buildChannelGroupSave(value, empty).errors?.["channels.0.quotaPoolId"]).toBeTruthy();
    }
    value.channels[0].quotaMode = "unexpected" as ChannelDraft["quotaMode"];
    value.strategy = "unexpected" as ChannelGroupDraft["strategy"];
    expect(buildChannelGroupSave(value, empty).errors?.["channels.0.quotaMode"]).toBeTruthy();
    expect(buildChannelGroupSave(value, empty).errors?.strategy).toBeTruthy();
  });

  it("loads the current shared policy and keeps saved revisions, zero budget and model alias", () => {
    const saved = group({ name: "Display name", virtualModel: "xy-group/stable", projectId: "shared", revision: 8 });
    const configuration = { groups: [saved], projects: [{ id: "shared", dailyMicrousd: 0, revision: 5 }] };
    const edited = editChannelGroupDraft(saved, configuration);
    expect(edited).toMatchObject({ name: "Display name", modelName: "stable", projectId: "shared", projectDailyMicrousd: "0", projectRevision: 5, revision: 8 });
    edited.name = "Renamed display name";
    expect(buildChannelGroupSave(edited, configuration).input).toMatchObject({ group: { virtualModel: "xy-group/stable", revision: 8 }, project: { id: "shared", dailyMicrousd: 0, revision: 5 } });
    const associated = changeDraftProject(draft(), "shared", configuration);
    expect(associated).toMatchObject({ projectDailyMicrousd: "0", projectRevision: 5 });
    expect(changeDraftProject(associated, "new-project", configuration)).toMatchObject({ projectDailyMicrousd: "", projectRevision: 0 });
  });

  it("leaves space for the backend revision increment", () => {
    expect(buildChannelGroupSave({ ...draft(), revision: 999_999_999_998 }, empty).errors).toBeNull();
    for (const revision of [-1, 0.5, 999_999_999_999]) {
      expect(buildChannelGroupSave({ ...draft(), revision }, empty).errors?.form).toBeTruthy();
      expect(buildChannelGroupSave({ ...draft(), projectRevision: revision }, empty).errors?.form).toBeTruthy();
    }
  });
});

describe("deterministic offline trial input", () => {
  it("orders and deduplicates selected provider events without mutating them", () => {
    const replay = [event({ id: 9, atMs: 200 }), event({ id: 8, atMs: 100 }), event({ id: 7, atMs: 100 }), event({ id: 8, atMs: 100 })];
    const original = structuredClone(replay);
    const input = deterministicChannelTrial("group-a", 150, replay);
    expect(input.replayEventIds).toEqual([7, 8, 9]);
    expect(input.steps.map(step => step.atMs)).toEqual([201, 1201, 2201, 3201, 4201, 5201]);
    expect(deterministicChannelTrial("group-a", 150, [...replay].reverse())).toEqual(input);
    expect(replay).toEqual(original);
  });

  it("creates six pending reservations with no fabricated completion or upstream payload", () => {
    const input = deterministicChannelTrial("group-a", 500);
    expect(input.replayEventIds).toEqual([]);
    expect(input.steps).toHaveLength(6);
    expect(new Set(input.steps.map(step => step.kind))).toEqual(new Set(["request"]));
    expect(input.steps[0]).toEqual({ kind: "request", atMs: 500, request: { id: "offline-1", groupId: "group-a", reserveTokens: 128, reserveMicrousd: 1000,
      stateful: false, sessionKey: null, previousResponseId: null, conversationId: null } });
    expect(input.steps[5]).toMatchObject({ kind: "request", atMs: 5500, request: { id: "offline-6" } });
    expect(JSON.stringify(input)).not.toMatch(/prompt|endpoint|apiKey|credentials/);
  });

  it("uses the snapshot time when newer and rejects unsafe millisecond arithmetic", () => {
    expect(deterministicChannelTrial("group-a", 500, [event({ atMs: 100 })]).steps[0].atMs).toBe(500);
    for (const timestamp of [-1, NaN, 0.5, Number.MAX_SAFE_INTEGER - 4999]) expect(() => deterministicChannelTrial("group-a", timestamp)).toThrow("channel_groups_integer_required");
    expect(() => deterministicChannelTrial("group-a", 0, [event({ atMs: Number.MAX_SAFE_INTEGER })])).toThrow("channel_groups_integer_required");
  });
});
