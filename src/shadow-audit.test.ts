import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { ShadowAuditPanel, ShadowAuditResults, ShadowRegressionDetails } from "./features/usage/ShadowAuditPanel";
import { StreamBillingDetails } from "./features/usage/StreamBillingDetails";
import { ModelAuditCell } from "./features/usage/ModelAuditCell";
import { analyzeShadowAudit, getShadowAuditSettings, saveShadowAuditSetting } from "./lib/model-audit-service";
import { shadowReasonLabel, shadowSettingRequest } from "./lib/shadow-audit";
import { HARD_AUDIT_FLAGS } from "./lib/model-audit";
import type { AuditRecord, ShadowAuditReport, ShadowAuditStatus, ShadowRegressionReport, StreamBillingReport } from "./types/model-audit";

const invokeMock = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke: invokeMock }));

const regression: ShadowRegressionReport = {
  status: "consistent", sampleCount: 28, minimumSamples: 3,
  slope: 1, intercept: 5, slopeCiLow: 0.98, slopeCiHigh: 1.02,
  residualRmse: 0.5, xSpan: 60, interceptDrift: 0.1,
  gaps: [], limitations: ["相邻轮次的样本相关性尚未排除"],
};

describe("影子审计配置与分析契约", () => {
  beforeEach(() => { invokeMock.mockReset(); vi.stubGlobal("window", { __TAURI_INTERNALS__: {} }); });
  afterEach(() => vi.unstubAllGlobals());

  it("默认关闭，呈现页面不读取配置、不保存也不分析", () => {
    const html = renderToStaticMarkup(createElement(ShadowAuditPanel, { filter: {}, logs: [] }));
    expect(invokeMock).not.toHaveBeenCalled();
    expect(html).toContain("被动影子差分（默认关闭）");
    expect(html).not.toContain('checked=""');
    expect(html).toContain("正文不持久化");
    expect(html).toContain("历史正文不能补算");
    expect(html).toContain("不发送探针");
    expect(html).toContain("无法据此证明远端没有改写系统提示词");
  });

  it("按实际模型保存设置，自动规则为 null，响应元数据不写回", async () => {
    const setting: ShadowAuditStatus = {
      providerId: " fixture-provider ", model: " routed-model ", enabled: true,
      tokenizerPath: " C:\\audit\\o200k_base.tiktoken ", pretokenizer: null,
      sourceSha256: "a".repeat(64), error: null, droppedRequests: 3,
    };
    const request = shadowSettingRequest(setting);
    expect(request).toEqual({ providerId: "fixture-provider", model: "routed-model", enabled: true, tokenizerPath: "C:\\audit\\o200k_base.tiktoken", pretokenizer: null });
    invokeMock.mockResolvedValue(setting);
    expect(await saveShadowAuditSetting(request)).toEqual(setting);
    expect(invokeMock).toHaveBeenLastCalledWith("save_shadow_audit_setting", { request });
    await saveShadowAuditSetting({ ...request, enabled: false });
    expect(invokeMock).toHaveBeenLastCalledWith("save_shadow_audit_setting", { request: { ...request, enabled: false } });
  });

  it("设置读取与统计分析使用独立命令，保留用户筛选", async () => {
    invokeMock.mockResolvedValue([]);
    expect(await getShadowAuditSettings()).toEqual([]);
    expect(invokeMock).toHaveBeenLastCalledWith("get_shadow_audit_settings");
    const filter = { startTimeMs: 123, providerId: "p", model: "effective-model" };
    await analyzeShadowAudit(filter);
    expect(invokeMock).toHaveBeenLastCalledWith("analyze_shadow_audit", { filter });
  });
});

describe("影子回归与流式比较的诚实状态", () => {
  it("回归的未测、一致与偏离有不同文字和颜色，缺口不会消失", () => {
    const untested = renderToStaticMarkup(createElement(ShadowRegressionDetails, { regression: { ...regression, status: "untested", sampleCount: 2, gaps: ["shadow.insufficient_pairs", "shadow.x_variance_insufficient"] } }));
    const consistent = renderToStaticMarkup(createElement(ShadowRegressionDetails, { regression }));
    const suspicious = renderToStaticMarkup(createElement(ShadowRegressionDetails, { regression: { ...regression, status: "suspicious", slope: 1.2 } }));
    expect(untested).toContain("audit-untested");
    expect(untested).toContain("这不等于没问题");
    expect(untested).toContain("跨度不小于 32 token");
    expect(consistent).toContain("当前样本内未见稳定偏离");
    expect(consistent).toContain("audit-clear");
    expect(suspicious).toContain("存在统计偏离，需核查");
    expect(suspicious).toContain("audit-statistical");
    expect(suspicious).toContain("条件性约 95% 区间");
    expect(suspicious).not.toContain("硬证据");
  });

  it("无分组显示未测，缓存原因保留并说明统计限制", () => {
    const report: ShadowAuditReport = { sampleLimit: 10000, sampleCount: 5, pairCount: 0, baselineCount: 2, excludedCount: 3, truncated: false, droppedRequests: 1, groups: [], reasons: [{ reason: "shadow.cache_present", count: 3 }], notes: [] };
    const html = renderToStaticMarkup(createElement(ShadowAuditResults, { report }));
    expect(html).toContain("没有可分析的配对分组");
    expect(html).toContain("存在缓存命中或缓存创建，已排除");
    expect(html).toContain("不能直接认定虚报或模型替换");
    expect(shadowReasonLabel("shadow.future_reason")).toContain("shadow.future_reason");
    expect(shadowReasonLabel("shadow.forwarded_text_changed")).toBe("客户端新增文本与本地实际转发文本不一致，已排除");
    expect(shadowReasonLabel("shadow.parent_ineligible")).toContain("不能作为有效基线");
    expect(shadowReasonLabel("shadow.forwarded_unsupported_system_content")).toContain("本地实际转发");
  });

  it("旧审计可缺 streamBilling；新增整段比较不进入硬证据并保留边界混淆", () => {
    const oldAudit: AuditRecord = { observation: { requestedModel: "m", forwardedModel: "m", upstreamModel: "m", complete: true, textOnly: true, stream: { textDeltaCount: 12, visibleOutputChars: 12, maxDigitRun: 0, maxHanRun: 0, maxSpaceRun: 0, medianDeltaChars: 1, ttfbMs: 50, digitChars: 0, hanChars: 0, spaceChars: 0, arrivalOffsetsMs: [], batched: false, minimumSamplesMet: true, samplesTruncated: false } }, modelStatus: "matched", flags: [], gaps: [], axes: { digit: "not_comparable", han: "not_comparable", space: "not_comparable" } };
    const oldHtml = renderToStaticMarkup(createElement(ModelAuditCell, { audit: oldAudit, legacyModel: "m" }));
    expect(oldHtml).toContain("分析此响应流式统计");
    expect(oldHtml).not.toContain("整段输出与计费比较");
    const unknown = renderToStaticMarkup(createElement(StreamBillingDetails, {}));
    expect(unknown).toContain("此记录没有整段比较数据");
    const billing: StreamBillingReport = { status: "contradiction", textFrames: 12, visibleTokens: 6, visibleBytes: 12, tokensPerByte: 0.5, bytesPerFrame: 1, framesPerToken: 2, gaps: [], limitations: [] };
    const html = renderToStaticMarkup(createElement(StreamBillingDetails, { billing }));
    expect(html).toContain("存在条件性差异");
    expect(html).toContain("SSE 可能把一个 token 拆成多帧");
    expect(html).toContain("不是硬证据");
    expect(HARD_AUDIT_FLAGS.has("inv.stream_billing")).toBe(false);
  });

  it("汉字使用 UTF-8 字节显示整段比率，避免字符单位造成三倍误差", () => {
    const text = "汉".repeat(12);
    const visibleBytes = new TextEncoder().encode(text).byteLength;
    expect(visibleBytes).toBe(36);
    const billing: StreamBillingReport = { status: "contradiction", textFrames: 12, visibleTokens: 6, visibleBytes, tokensPerByte: 6 / visibleBytes, bytesPerFrame: visibleBytes / 12, framesPerToken: 2, gaps: [], limitations: [] };
    // A stale producer may still include the obsolete values; never label these as bytes.
    Object.assign(billing, { visibleChars: text.length, tokensPerChar: 0.5, charsPerFrame: 1 });
    const html = renderToStaticMarkup(createElement(StreamBillingDetails, { billing }));
    expect(html).toContain("上报可见 token / UTF-8 字节</dt><dd>6 / 36</dd>");
    expect(html).toContain("token / UTF-8 字节</dt><dd>0.167</dd>");
    expect(html).toContain("UTF-8 字节 / 帧</dt><dd>3.000</dd>");
    expect(html).not.toContain("token / 字符");
    expect(html).not.toContain("字符 / 帧");
    expect(html).toContain("跨渠道比较前须按语言构成分桶");
  });

  it("旧字符报告或缺失字节数不能被当作有效的字节比较", () => {
    const legacy = { status: "consistent", textFrames: 12, visibleTokens: 6, visibleChars: 12, tokensPerChar: 0.5, charsPerFrame: 1, framesPerToken: 2, gaps: [], limitations: [] } as unknown as StreamBillingReport;
    const legacyHtml = renderToStaticMarkup(createElement(StreamBillingDetails, { billing: legacy }));
    expect(legacyHtml).toContain("未测到 / 不可比");
    expect(legacyHtml).toContain("旧字符计数不能替代字节计数");
    expect(legacyHtml).not.toContain("在边界假设下未见反向矛盾");
    expect(legacyHtml).toContain("token / UTF-8 字节</dt><dd>未测到</dd>");
    const missing = { ...legacy, status: "not_comparable" as const, visibleBytes: null, tokensPerByte: null, bytesPerFrame: null, gaps: ["scv.bytes_missing"] };
    const html = renderToStaticMarkup(createElement(StreamBillingDetails, { billing: missing }));
    expect(html).toContain("缺少可见文本 UTF-8 字节数");
    expect(html).not.toContain("NaN");
  });
});
