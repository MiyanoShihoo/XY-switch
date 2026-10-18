import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { ResponseModelEvidence } from "./features/usage/ResponseModelEvidence";
import { ProviderResponseProbe } from "./features/providers/ProviderResponseProbe";
import { UsagePage } from "./features/usage/UsagePage";
import { responseModelEvidence } from "./lib/response-model";
import { probeProviderResponseModel } from "./lib/response-model-service";
import type { AuditRecord } from "./types/model-audit";
import type { Provider } from "./types/provider";

const invokeMock = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke: invokeMock }));

function audit(upstreamModel: string | null = "expected", extra: Partial<AuditRecord["observation"]> = {}): AuditRecord {
  return { observation: { requestedModel: "expected", forwardedModel: "expected", upstreamModel, inputTokens: 4, outputTokens: 1, totalTokens: 5, reasoningTokens: 0, maxTokens: 64, visibleOutputBytes: 2, toolPayloadBytes: 0, textOnly: true, complete: true, ...extra }, flags: [], gaps: [], modelStatus: "matched", axes: { digit: "not_comparable", han: "not_comparable", space: "not_comparable" } };
}
const provider: Provider = { id: "p", name: "Probe", group: "Codex", endpoint: "https://relay.example", model: "expected", enabled: true, icon: "custom", advanced: { upstreamFormat: "openai_responses", modelMappings: [], customUserAgent: "", headerOverrides: {}, bodyOverrides: {}, codex: { remoteCompression: false, applyCommonConfig: false, context1m: false, compactionThreshold: 0, configToml: "" }, claude: { disableArtifactTool: false } } };

beforeEach(() => { invokeMock.mockReset(); vi.unstubAllGlobals(); });

describe("每条响应单独判断，不把声明当作身份认证", () => {
  it("同供应商的成功、不一致、缺失结果互不继承", () => {
    const first = responseModelEvidence(audit());
    const second = responseModelEvidence(audit("different"));
    const missing = responseModelEvidence(audit(null));
    expect(first.status).toBe("matched");
    expect(second.status).toBe("mismatched");
    expect(second.declaredModels).toEqual(["different"]);
    expect(missing.status).toBe("unknown");
    expect(missing.declaredModels).toEqual([]);
    expect(responseModelEvidence(audit()).declaredModels).toEqual(["expected"]);
  });

  it("以实际转发模型对照，不相信旧版的整体或缓存结论", () => {
    const matched = responseModelEvidence(audit("mapped", { requestedModel: "alias", forwardedModel: "mapped" }));
    expect(matched.status).toBe("matched");
    expect(matched.comparison).toBe("映射后名称一致");
    const wrong = responseModelEvidence(audit("alias", { requestedModel: "alias", forwardedModel: "mapped" }));
    expect(wrong.status).toBe("mismatched");
    expect(responseModelEvidence({ ...audit(), modelStatus: "mismatched" }).status).toBe("matched");
  });

  it("保留同一流中的不同声明，重复声明去重", () => {
    const result = responseModelEvidence(audit("second", { upstreamModels: ["first", "first", "second"] }));
    expect(result.status).toBe("conflicting");
    expect(result.declaredModels).toEqual(["first", "second"]);
    expect(result.verifiedModel).toBeNull();
  });

  it("截断和缺失证据不能变成名称一致，未完成响应保留部分证据", () => {
    expect(responseModelEvidence(audit("expected", { modelEvidenceTruncated: true })).status).toBe("unknown");
    expect(responseModelEvidence(audit(" ", { upstreamModels: ["", "\t"] })).status).toBe("unknown");
    const partial = responseModelEvidence(audit("partial", { complete: false }));
    expect(partial.partial).toBe(true);
    expect(partial.declaredModels).toEqual(["partial"]);
    expect(responseModelEvidence()).toMatchObject({ status: "unknown", partial: false, declaredModels: [] });
  });

  it("本地会话模型不冒充原始上游响应", () => {
    const local = responseModelEvidence(audit(), true);
    expect(local).toMatchObject({ status: "unknown", declaredModels: [], verifiedModel: null, identityStatus: "unverified" });
    expect(local.identityReason).toContain("客户端记录");
  });

  it("模型名相同、模型自述、统计指纹或任意 actual_model 字段均不构成证明", () => {
    const forged = { ...audit(), verifiedModel: "real-model", actual_model: "real-model", identityStatus: "verified", confidence: 1, selfDescription: "I am real-model", fingerprint: { matched: true } };
    for (const evidence of [responseModelEvidence(audit()), responseModelEvidence(forged)]) {
      expect(evidence.verifiedModel).toBeNull();
      expect(evidence.identityStatus).toBe("unverified");
      expect(evidence.identityLabel).toBe("未证实");
    }
  });
});

describe("两个指定入口与手动探测契约", () => {
  it("并排展示上游声明和真实响应模型，渲染不发请求", () => {
    const html = renderToStaticMarkup(createElement(ResponseModelEvidence, { audit: audit("declared-only") }));
    expect(html).toContain("上游声明");
    expect(html).toContain("declared-only");
    expect(html).toContain("真实响应模型");
    expect(html).toContain("未证实");
    expect(html).toContain("本次结果不代表同供应商的其他请求");
    expect(invokeMock).not.toHaveBeenCalled();
  });

  it("配置检查中的一次探测明确告知预算且不自动触发", () => {
    const html = renderToStaticMarkup(createElement(ProviderResponseProbe, { provider, busy: false }));
    expect(html).toContain("整个模块默认关闭");
    expect(html).toContain('aria-checked="false"');
    expect(html).toContain("不插入提示词，不改业务参数");
    expect(html).not.toContain("复核最近一条异常");
    expect(invokeMock).not.toHaveBeenCalled();
  });

  it("用量页移除独立模型审计 Tab，逐行展示各自结果", () => {
    const client = new QueryClient({ defaultOptions: { queries: { retry: false, gcTime: Infinity } } });
    const rows = ["first", "second"].map((model, i) => ({ requestId: String(i), providerId: "p", appType: "model_probe", source: "model_probe", model, inputTokens: 4, outputTokens: 1, cacheReadTokens: 0, cacheCreationTokens: 0, totalCostUsd: "", inputCostUsd: "", outputCostUsd: "", cacheReadCostUsd: "", cacheCreationCostUsd: "", costMultiplier: "", isStreaming: false, latencyMs: 10, statusCode: 200, success: true, createdAt: 1234567890, audit: audit(model) }));
    client.setQueryData(["usage", { dateRange: "today" }, 0], { summary: { totalRequests: 2, successCount: 2, unknownCostRequests: 2, totalInputTokens: 8, totalOutputTokens: 2, totalCacheReadTokens: 0, totalCacheCreationTokens: 0, totalCostUsd: "", avgLatencyMs: 10 }, logs: rows, trends: [], providerStats: [], modelStats: [], sync: { codexImported: 0, harnessImported: 0, warnings: [] } });
    const html = renderToStaticMarkup(createElement(QueryClientProvider, { client }, createElement(UsagePage, { providers: [provider] })));
    expect(html).not.toContain("usage-data-tab-audit");
    expect(html).not.toContain(">模型审计<");
    expect(html.match(/>真实响应模型<\/span>/g)?.length).toBe(2);
    expect(html).toContain(">first</code>");
    expect(html).toContain(">second</code>");
    expect(html).toContain("探测详情");
    expect(invokeMock).not.toHaveBeenCalled();
    client.clear();
  });

  it("普通网页预览不会发送任何网络探测", async () => {
    await expect(probeProviderResponseModel("p", "m")).rejects.toThrow("桌面应用");
    vi.stubGlobal("window", {});
    await expect(probeProviderResponseModel("p", "m")).rejects.toThrow("桌面应用");
    expect(invokeMock).not.toHaveBeenCalled();
  });

  it("桌面环境没有异常 ID 时也不能直接收费请求", async () => {
    vi.stubGlobal("window", { __TAURI_INTERNALS__: {} });
    await expect(probeProviderResponseModel("p", "target")).rejects.toThrow("被动异常");
    expect(invokeMock).not.toHaveBeenCalled();
  });

  it("一次显式操作只调用一次 IPC，不批量、不重试、不传持久化密钥", async () => {
    vi.stubGlobal("window", { __TAURI_INTERNALS__: {} });
    const result = { requestId: "one", audit: audit() };
    invokeMock.mockResolvedValueOnce(result);
    expect(await probeProviderResponseModel("p", "target", "123")).toBe(result);
    expect(invokeMock).toHaveBeenCalledExactlyOnceWith("probe_provider_response_model", { request: { providerId: "p", model: "target", anomalyId: "123" } });
  });

  it("探测失败不会复用上一条成功结果", async () => {
    vi.stubGlobal("window", { __TAURI_INTERNALS__: {} });
    invokeMock.mockResolvedValueOnce({ requestId: "previous" }).mockRejectedValueOnce(new Error("connection failed"));
    await probeProviderResponseModel("p", "target", "123");
    await expect(probeProviderResponseModel("p", "target", "123")).rejects.toThrow("connection failed");
    expect(invokeMock).toHaveBeenCalledTimes(2);
  });
});
