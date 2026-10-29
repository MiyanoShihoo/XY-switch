import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { afterEach, describe, expect, it, vi } from "vitest";
import { DEFAULT_VERIFICATION_SETTINGS, VERIFICATION_TOKEN_BUDGET_RANGE, type VerificationEvidence, type VerificationStatus } from "./types/model-verification";
import { confirmVerificationCost, getVerificationStatus, saveVerificationSettings } from "./lib/model-verification-service";
import { eligibleEvidence } from "./lib/model-verification";
import { VerificationDetails } from "./features/usage/VerificationDetails";

const { invokeMock } = vi.hoisted(() => ({ invokeMock: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: invokeMock }));
afterEach(() => { vi.unstubAllGlobals(); invokeMock.mockReset(); });

function evidence(change: Partial<VerificationEvidence> = {}): VerificationEvidence {
  return { observationId: "11", upstreamId: "fixture", requestedModel: "Logical-A", forwardedModel: "upstream-20261018",
    reportedModels: ["upstream-20261018"], credentialFingerprint: "PRIVATE_HMAC", contextKey: "PRIVATE_CONTEXT",
    protocol: "chat", protocolShape: "chat", claimConsistency: "MATCH", identityAssurance: "UNVERIFIED",
    evidenceCodes: ["CLAIM_MATCH"], explanation: "本条响应声明一致，身份未独立验证。", riskLevel: "info",
    observedAt: 1792278000000, usageStatus: "complete", inputTokens: 4, outputTokens: 1, reasoningTokens: null,
    totalTokens: 5, cacheReadTokens: null, cacheCreationTokens: null, isStreaming: true, streamCompleted: true,
    httpStatus: 200, finishReason: "stop", responseId: null, providerRequestId: null,
    evidenceSource: "untrusted_upstream_metadata", source: "passive", rollingObservedCount: 1,
    routeChangeCount: 0, repeatedAnomaly: false, newEvidence: true, activeProbeState: "disabled", activeProbeTokens: null, ...change };
}
function status(): VerificationStatus {
  return { settings: { ...DEFAULT_VERIFICATION_SETTINGS, aliases: [] }, recent: [evidence()], observedResponses: 1,
    probeRequestsToday: 0, probeTokensToday: 0, budgetUsedToday: 0, capabilityConfirmed: false,
    droppedObservations: 0, storageError: false };
}
const render = (e: VerificationEvidence) => renderToStaticMarkup(createElement(VerificationDetails, { evidence: e }));

describe("低 Token 验证开关与 IPC", () => {
  it("两个开关默认关闭，限额使用低成本默认值", () => {
    expect(DEFAULT_VERIFICATION_SETTINGS).toMatchObject({ enabled: false, activeProbeEnabled: false,
      maxRequestsPerDay: 1, softTokenBudgetPerDay: 100, maxOutputTokens: 8, timeoutSeconds: 15,
      cooldownHours: 24, storeDiagnosticIds: false, aliases: [] });
  });
  it("浏览器预览只有独立的空状态，不发送 IPC 或网络请求", async () => {
    const fetch = vi.fn(); vi.stubGlobal("fetch", fetch); vi.stubGlobal("window", {});
    const first = await getVerificationStatus("p", "m");
    first.settings.aliases.push({ providerId: "p", requestedModel: "m", reportedModel: "other", userConfirmed: true, allowRouteChange: false });
    const second = await getVerificationStatus("p", "m");
    expect(second.settings.aliases).toEqual([]);
    expect(second).toMatchObject({ recent: [], observedResponses: 0, probeRequestsToday: 0, capabilityConfirmed: false });
    expect(invokeMock).not.toHaveBeenCalled(); expect(fetch).not.toHaveBeenCalled();
  });
  it("网页不能启用开关或确认收费", async () => {
    vi.stubGlobal("window", {});
    await expect(saveVerificationSettings({ ...DEFAULT_VERIFICATION_SETTINGS, enabled: true })).rejects.toThrow("桌面应用");
    await expect(confirmVerificationCost("p", "m", true)).rejects.toThrow("桌面应用");
    expect(invokeMock).not.toHaveBeenCalled();
  });
  it("桌面读取只调用本地状态 IPC，保留费用未知与凭据错误", async () => {
    vi.stubGlobal("window", { __TAURI_INTERNALS__: {} });
    const expected = { ...status(), probeTokensToday: null, contextError: "凭据暂不可用" };
    invokeMock.mockResolvedValueOnce(expected);
    expect(await getVerificationStatus("p", "Logical-A")).toEqual(expected);
    expect(invokeMock).toHaveBeenCalledExactlyOnceWith("model_verification_status", { providerId: "p", model: "Logical-A" });
  });
  it("别名保留大小写、版本和显式用户确认，不执行前缀猜测", async () => {
    vi.stubGlobal("window", { __TAURI_INTERNALS__: {} });
    const settings = { ...DEFAULT_VERIFICATION_SETTINGS, enabled: true, aliases: [{ providerId: "p", requestedModel: "Model-A", reportedModel: "Model-A-20261018", userConfirmed: true, allowRouteChange: false }] };
    invokeMock.mockResolvedValueOnce(settings);
    expect(await saveVerificationSettings(settings)).toEqual(settings);
    expect(invokeMock).toHaveBeenCalledExactlyOnceWith("set_model_verification_settings", { settings });
  });
  it("费用边界确认本身只调用确认 IPC，不发送探测", async () => {
    vi.stubGlobal("window", { __TAURI_INTERNALS__: {} }); invokeMock.mockResolvedValueOnce(undefined);
    await confirmVerificationCost("p", "m", true);
    expect(invokeMock).toHaveBeenCalledExactlyOnceWith("confirm_model_verification_cost", { providerId: "p", model: "m", confirmed: true });
  });
  it("本地失败原样传播，不伪造已开启或验证成功", async () => {
    vi.stubGlobal("window", { __TAURI_INTERNALS__: {} }); invokeMock.mockRejectedValueOnce(new Error("storage unavailable"));
    await expect(saveVerificationSettings(DEFAULT_VERIFICATION_SETTINGS)).rejects.toThrow("storage unavailable");
    expect(invokeMock).toHaveBeenCalledTimes(1);
  });
});

describe("逐响应证据与异常资格", () => {
  it.each(["MODEL_CLAIM_MISMATCH", "MODEL_ROUTE_CHANGED", "PROTOCOL_SHAPE_CHANGED"])("完整被动异常 %s 可进入后端预算检查", code => {
    expect(eligibleEvidence(evidence({ evidenceCodes: [code] }))).toBe(true);
  });
  it.each<Partial<VerificationEvidence>>([{ source: "active" }, { streamCompleted: false }, { httpStatus: null }, { httpStatus: 429 }, { httpStatus: 500 }])("拒绝不完整、错误或主动样本 %j", change => {
    expect(eligibleEvidence(evidence({ evidenceCodes: ["MODEL_CLAIM_MISMATCH"], ...change }))).toBe(false);
  });
  it("正常匹配、缺失字段、已确认合法回退都不靠按钮产生请求", () => {
    for (const code of ["CLAIM_MATCH", "MODEL_FIELD_MISSING", "USAGE_MISSING", "ALLOWED_ROUTE_CHANGE"]) {
      expect(eligibleEvidence(evidence({ evidenceCodes: [code] }))).toBe(false);
    }
  });
  it("MATCH 仍显示未验证，不展示内部指纹或上下文散列", () => {
    const html = render(evidence());
    for (const text of ["声明匹配", "真实身份未验证", "声明匹配不是模型身份认证", "无额外请求"]) expect(html).toContain(text);
    for (const text of ["PRIVATE_HMAC", "PRIVATE_CONTEXT", "真实模型已证实"]) expect(html).not.toContain(text);
  });
  it("不自洽统计和未知探测用量不会显示成免费或零成本", () => {
    const html = render(evidence({ usageStatus: "contradictory", evidenceCodes: ["USAGE_CONTRADICTORY"], activeProbeState: "failed_or_unknown", activeProbeTokens: null }));
    expect(html).toContain("统计不自洽，费用未知"); expect(html).toContain("保留预留"); expect(html).toContain("用量 未知");
    expect(html).not.toContain("免费");
  });
  it("不同响应独立显示，不复用同供应商上一条结论", () => {
    const html = render(evidence()) + render(evidence({ observationId: "12", reportedModels: ["other-model"], claimConsistency: "MISMATCH", evidenceCodes: ["MODEL_CLAIM_MISMATCH"] }));
    expect(html).toContain('data-observation-id="11"'); expect(html).toContain('data-observation-id="12"');
    expect(html).toContain("声明匹配"); expect(html).toContain("声明不匹配"); expect(html).toContain("other-model");
    expect(invokeMock).not.toHaveBeenCalled();
  });
  it("缺失与冲突显示证据不足，诊断字段只在提供时展示并转义", () => {
    const html = render(evidence({ reportedModels: [], claimConsistency: "MISSING", identityAssurance: "INCONCLUSIVE", responseId: "<script>fixture</script>" }));
    expect(html).toContain("声明缺失"); expect(html).toContain("身份证据不足"); expect(html).toContain("未返回");
    expect(html).toContain("&lt;script&gt;fixture&lt;/script&gt;"); expect(html).not.toContain("<script>");
    expect(render(evidence())).not.toContain("响应 ID");
  });
});


describe("可调整每日 Token 预算", () => {
  it("默认 100，用户范围包含 0 和 10000", () => {
    expect(VERIFICATION_TOKEN_BUDGET_RANGE).toEqual({ min: 0, max: 10000 });
    expect(DEFAULT_VERIFICATION_SETTINGS.softTokenBudgetPerDay).toBe(100);
  });
  it.each([0, 1, 10000])("保存合法预算 %i，不自动启用收费复核", async budget => {
    vi.stubGlobal("window", { __TAURI_INTERNALS__: {} });
    const settings = { ...DEFAULT_VERIFICATION_SETTINGS, softTokenBudgetPerDay: budget };
    invokeMock.mockResolvedValueOnce(settings);
    expect(await saveVerificationSettings(settings)).toEqual(settings);
    expect(invokeMock).toHaveBeenCalledExactlyOnceWith("set_model_verification_settings", { settings });
  });
  it.each([-1, 10001, 0.5, Number.NaN, Number.POSITIVE_INFINITY])("拒绝非法预算 %s，不发送 IPC", async budget => {
    vi.stubGlobal("window", { __TAURI_INTERNALS__: {} });
    await expect(saveVerificationSettings({ ...DEFAULT_VERIFICATION_SETTINGS, softTokenBudgetPerDay: budget })).rejects.toThrow("0–10000");
    expect(invokeMock).not.toHaveBeenCalled();
  });
});
