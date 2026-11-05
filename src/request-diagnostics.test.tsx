import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { afterEach, describe, expect, it } from "vitest";
import { forwardingStatusLabel } from "./features/usage/RequestDiagnosticsPanel";
import { UsagePage, UsageRequestDetails } from "./features/usage/UsagePage";
import { setLanguagePreference, t } from "./i18n/language";
import type { UsageLogEntry } from "./types/usage";
import type { ProxyTask } from "./types/governance";

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
  it("shows zero-send privacy rejection with count-only diagnostics in both languages", async () => {
    const row = { ...rejection, statusCode: 422, errorMessage: "privacy_sensitive_structure", diagnostics: { ...rejection.diagnostics!, failureStage: "privacy", privacy: { secretRedactions: 2, personalRedactions: 1, protectedFields: 1, uninspectedAttachments: 0 } } };
    const html = details(row);
    expect(html).toContain("本地拒绝 · 422");
    expect(html).toContain("隐私检查计数");
    expect(html).toContain("凭据替换数</dt><dd>2</dd>");
    expect(html).toContain("可执行字段或结构字段含有敏感信息，发送已阻止。");
    expect(html).toContain("不保存匹配到的正文");
    expect(html).not.toContain("HTTP 422");
    await setLanguagePreference("en");
    const english = details(row);
    expect(english).toContain("Privacy inspection counts");
    expect(english).toContain("sending was blocked");
    expect(english).not.toContain("隐私检查");
  });
  it("explains response privacy stops while preserving the actual upstream evidence", async () => {
    const row = { ...rejection, statusCode: 200, errorMessage: "privacy_response_sensitive_structure", diagnostics: {
      ...rejection.diagnostics!, failureStage: "response_privacy", sendAttempts: 1, upstreamResponseReceived: true,
      privacy: { secretRedactions: 1, personalRedactions: 0, protectedFields: 1, uninspectedAttachments: 0 },
    } };
    const chinese = details(row);
    expect(chinese).toContain("响应隐私检查");
    expect(chinese).toContain("已停止返回");
    expect(chinese).toContain("HTTP 200 · 未完成");
    expect(chinese).not.toContain("发送已阻止");
    await setLanguagePreference("en");
    for (const code of ["privacy_response_attachment_uninspected", "privacy_response_scan_limit", "privacy_response_scan_failed"]) {
      expect(details({ ...row, errorMessage: code })).toContain("delivery was stopped");
    }
    const english = details(row);
    expect(english).toContain("Response privacy inspection");
    expect(english).toContain("delivery was stopped");
    expect(english).not.toContain("privacy_response_sensitive_structure");
  });
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

  it("labels task diagnostics as a record snapshot and explains zero-send task rejection", async () => {
    const task: ProxyTask = { id: "task-history", identitySource: "task_header", maxAttempts: 3, maxMicrousd: 3168, attempts: 3, reservedMicrousd: 3168, committedMicrousd: 3168, knownMicrousd: 6, unknownCostAttempts: 2, unreservedAttempts: 0, activeAttempts: 0, cancelled: false, createdAt: 1_800_000_000, updatedAt: 1_800_000_001 };
    const row = { ...rejection, statusCode: 402, errorMessage: "budget_rejected", diagnostics: { ...rejection.diagnostics!, failureStage: "budget", budgetReason: "task_attempt_budget_exhausted", taskRequestId: "client-retry", task } };
    const html = details(row);
    expect(html).toContain("本地拒绝 · 402");
    expect(html).toContain("单任务总尝试上限已耗尽。");
    expect(html).toContain("任务累计与上限（记录快照）");
    expect(html).toContain("最新累计和状态请查看硬预算中的最近任务");
    expect(html).toContain("任务累计尝试 / 上限</dt><dd>3 / 3");
    expect(html).toContain("client-retry");
    expect(html).not.toContain("HTTP 402");
    await setLanguagePreference("en");
    const english = details(row);
    expect(english).toContain("Task totals and limits (record snapshot)");
    expect(english).toContain("The total attempt limit for this task is exhausted.");
    expect(english).not.toContain("记录快照");
  });
});
