import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { afterEach, describe, expect, it } from "vitest";
import { forwardingStatusLabel } from "./features/usage/RequestDiagnosticsPanel";
import { UsagePage, UsageRequestDetails } from "./features/usage/UsagePage";
import { setLanguagePreference, t } from "./i18n/language";
import type { UsageLogEntry } from "./types/usage";

const rejection: UsageLogEntry = {
  requestId: "unassigned:1", providerId: "", appType: "codex", model: "", source: "proxy",
  inputTokens: 0, outputTokens: 0, cacheReadTokens: 0, cacheCreationTokens: 0,
  totalCostUsd: "0", inputCostUsd: "0", outputCostUsd: "0", cacheReadCostUsd: "0", cacheCreationCostUsd: "0",
  costMultiplier: "", isStreaming: false, latencyMs: 2, statusCode: 503, success: false,
  errorMessage: "provider_not_selected", createdAt: 1_800_000_000_000,
  diagnostics: { failureStage: "provider_selection", failureReason: "No provider is selected.", sendAttempts: 0, upstreamResponseReceived: false, attempts: [] },
};
const details = (row: UsageLogEntry) => renderToStaticMarkup(createElement(UsageRequestDetails, { row, providerName: t("未选择供应商") }));

afterEach(async () => { await setLanguagePreference("system"); });

describe("request forwarding evidence presentation", () => {
  it("shows local rejection in the production log table without claiming an upstream HTTP response", () => {
    const client = new QueryClient({ defaultOptions: { queries: { retry: false, gcTime: Infinity, staleTime: Infinity } } });
    client.setQueryData(["usage", { dateRange: "today" }, 0], {
      summary: { totalRequests: 1, successCount: 0, unknownCostRequests: 0, totalInputTokens: 0, totalOutputTokens: 0, totalCacheReadTokens: 0, totalCacheCreationTokens: 0, totalCostUsd: "0", avgLatencyMs: 2 },
      logs: [rejection], trends: [], providerStats: [], modelStats: [], sync: { codexImported: 0, harnessImported: 0, warnings: [] },
    });
    const html = renderToStaticMarkup(createElement(QueryClientProvider, { client }, createElement(UsagePage, { providers: [] })));
    expect(html).toContain("未选择供应商");
    expect(html).toContain("本地拒绝 · 503");
    expect(html).not.toContain("HTTP 503");
    client.clear();
  });

  it("renders actual attempt and response evidence in the production request detail", () => {
    const html = details({ ...rejection, statusCode: 200, errorMessage: "stream_interrupted", diagnostics: {
      failureStage: "response_stream", failureReason: "The response stream ended before completion.",
      sendAttempts: 2, upstreamResponseReceived: true,
      attempts: [{ statusCode: 503, errorKind: "upstream_status" }, { statusCode: 200, errorKind: null }],
    } });
    expect(html).toContain("HTTP 200 · 未完成");
    expect(html).toContain("响应流在完成前中断。");
    expect(html).toContain("发送尝试次数</dt><dd>2");
    expect(html).toContain("收到过上游 HTTP 响应</dt><dd>是");
    expect(html).toContain("尝试 1 · HTTP 503");
    expect(html).toContain("尝试 2 · HTTP 200");
    expect(html).not.toContain("stream_interrupted");
  });

  it("keeps old evidence unknown and imported session records free of invented HTTP evidence", () => {
    const legacy = { ...rejection, diagnostics: undefined, errorMessage: "old_error" };
    expect(details(legacy)).toContain("发送次数和上游响应状态未知");
    expect(details(legacy)).toContain("old_error");
    expect(forwardingStatusLabel(legacy)).toBe("HTTP 503 · 未完成");
    const html = details({ ...legacy, providerId: "local:codex", source: "codex_session", statusCode: 200 });
    expect(html).toContain("会话状态 / 耗时");
    expect(html).not.toContain('aria-label="请求链路"');
  });

  it("translates stages and reasons and escapes unknown text", async () => {
    await setLanguagePreference("en");
    expect(details(rejection)).toContain("No provider is selected.");
    expect(details(rejection)).toContain("Send attempts");
    expect(details(rejection)).not.toContain("选择供应商");
    const html = details({ ...rejection, errorMessage: "unknown_future_error", diagnostics: { ...rejection.diagnostics!, failureReason: "<script>untrusted()</script>" } });
    expect(html).toContain("&lt;script&gt;");
    expect(html).not.toContain("<script>");
  });
});
