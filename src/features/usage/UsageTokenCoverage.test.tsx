import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { ModelUsageStat, ProviderUsageStat, TokenCoverage, UsageLogEntry, UsageSummary } from "../../types/usage";
import { UsageLedger } from "./UsageLedger";
import { UsageContributions } from "./UsageContributions";
import { UsageRequestWorkspace } from "./UsageRequestWorkspace";
import { UsageRequestDetails } from "./UsageRequestDetails";
import { UsagePage } from "./UsagePage";
import { aggregateCostLabel } from "./usage-presentation";

const missing: TokenCoverage = { input: 0, output: 0, cacheRead: 0, cacheCreation: 0 };
const complete: TokenCoverage = { input: 1, output: 1, cacheRead: 1, cacheCreation: 1 };
const providerNames = new Map([["candidate", "Candidate"]]);

function summary(tokenCoverage: TokenCoverage = missing, overrides: Partial<UsageSummary> = {}): UsageSummary {
  return { totalRequests: 1, successCount: 0, unknownCostRequests: 1, totalInputTokens: 0, totalOutputTokens: 0, totalCacheReadTokens: 0, totalCacheCreationTokens: 0, totalCostUsd: "", avgLatencyMs: 12, ...overrides, tokenCoverage };
}

function request(tokenCoverage: TokenCoverage = missing, overrides: Partial<UsageLogEntry> = {}): UsageLogEntry {
  return { requestId: "2", providerId: "candidate", appType: "codex", source: "proxy", model: "paid-model", statusCode: 525, success: false, createdAt: 1_800_000_000_000, latencyMs: 12, isStreaming: false, costMultiplier: "", totalTokens: null, reasoningTokens: null, inputTokens: 0, outputTokens: 0, cacheReadTokens: 0, cacheCreationTokens: 0, totalCostUsd: "", inputCostUsd: "", outputCostUsd: "", cacheReadCostUsd: "0.0000", cacheCreationCostUsd: "0.0000", ...overrides, tokenCoverage };
}

function contributions(data: ReturnType<typeof summary>) {
  const common = { providerId: "candidate", totalRequests: data.totalRequests, inputTokens: data.totalInputTokens, outputTokens: data.totalOutputTokens, totalCostUsd: data.totalCostUsd, tokenCoverage: data.tokenCoverage, unknownCostRequests: data.unknownCostRequests };
  const providerStats: ProviderUsageStat[] = [{ ...common, successRate: 0, avgLatencyMs: 12 }];
  const modelStats: ModelUsageStat[] = [{ ...common, model: "paid-model" }];
  return { providerStats, modelStats };
}

function details(row: UsageLogEntry) {
  return renderToStaticMarkup(createElement(UsageRequestDetails, { row, providerName: "Candidate" }));
}

function tokenPairs(html: string) {
  return [...html.matchAll(/<span class="usage-contributions__tokens">(.*?)<\/span>/g)].map(match => match[1]);
}

function ledgerValues(html: string) {
  const ledger = html.match(/<div class="usage-ledger__tokens"[\s\S]*?<\/dl>/)?.[0] ?? "";
  return [...ledger.matchAll(/<dd>(.*?)<\/dd>/g)].map(match => match[1]);
}

afterEach(() => vi.unstubAllGlobals());

describe("usage token coverage presentation", () => {
  it("keeps the HTTP 525 missing-usage shape unknown in overview, both groups and the request list", () => {
    const data = summary();
    const ledger = renderToStaticMarkup(createElement(UsageLedger, { summary: data }));
    expect(ledgerValues(ledger)).toEqual(["未知", "未知", "未知", "未知"]);
    const groups = renderToStaticMarkup(createElement(UsageContributions, { ...contributions(data), providerNames, onInspect: () => {} }));
    expect(tokenPairs(groups)).toEqual(["未知 / 未知", "未知 / 未知"]);
    const logs = renderToStaticMarkup(createElement(UsageRequestWorkspace, { rows: [request()], providerNames, selected: null, onSelect: () => {} }));
    expect(tokenPairs(logs)).toEqual(["未知 / 未知"]);
    expect(logs).toContain("HTTP 525");
  });

  it("preserves explicitly known zero tokens and zero costs independently", () => {
    const data = summary(complete, { successCount: 1, unknownCostRequests: 0, totalCostUsd: "0.0000" });
    const ledger = renderToStaticMarkup(createElement(UsageLedger, { summary: data }));
    expect(ledgerValues(ledger)).toEqual(["0", "0", "0", "0"]);
    expect(ledger).toContain('class="usage-ledger__amount">$0.0000 <small>');
    const groups = renderToStaticMarkup(createElement(UsageContributions, { ...contributions(data), providerNames, onInspect: () => {} }));
    expect(tokenPairs(groups)).toEqual(["0 / 0", "0 / 0"]);
    expect(groups.match(/>\$0\.0000<\/td>/g)).toHaveLength(2);
    const row = request(complete, { totalTokens: 0, reasoningTokens: 0, inputCostUsd: "0.0000", outputCostUsd: "0.0000", totalCostUsd: "0.0000" });
    const html = details(row);
    expect(html).toContain("总 token / 推理 token</dt><dd>0 / 0</dd>");
    for (const label of ["输入", "输出", "缓存读取", "缓存写入"]) {
      expect(html).toContain('<th scope="row">' + label + '</th><td>0</td><td><span class="usage-amount">$0.0000</span>');
    }
    expect(details(request(complete))).toContain('<th scope="row">输入</th><td>0</td><td><span class="usage-amount">未知</span>');
  });

  it("labels each partially covered field, including a known partial zero", () => {
    const data = summary({ input: 1, output: 2, cacheRead: 0, cacheCreation: 3 }, { totalRequests: 3, totalInputTokens: 11, totalOutputTokens: 0, totalCacheCreationTokens: 0, totalCostUsd: "0.1000", unknownCostRequests: 2 });
    expect(ledgerValues(renderToStaticMarkup(createElement(UsageLedger, { summary: data })))).toEqual(["11（已知部分）", "0（已知部分）", "未知", "0"]);
    const groups = renderToStaticMarkup(createElement(UsageContributions, { ...contributions(data), providerNames, onInspect: () => {} }));
    expect(tokenPairs(groups)).toEqual(["11（已知部分） / 0（已知部分）", "11（已知部分） / 0（已知部分）"]);
    expect(groups.match(/\$0\.1000（已知部分）/g)).toHaveLength(2);
  });

  it("only derives totals from known components and respects an explicit total zero", () => {
    const partial = request({ ...missing, input: 1 }, { inputTokens: 0 });
    expect(details(partial)).toContain("总 token / 推理 token</dt><dd>0（已知部分） / 未知</dd>");
    expect(details(request(complete, { inputTokens: 11, outputTokens: 2 }))).toContain("总 token / 推理 token</dt><dd>13 / 未知</dd>");
    expect(details(request(missing, { totalTokens: 0 }))).toContain("总 token / 推理 token</dt><dd>0 / 未知</dd>");
    expect(details({ ...partial, tokenCoverage: undefined })).toContain("总 token / 推理 token</dt><dd>未知 / 未知</dd>");
  });

  it.each(["known", "missing"])("keeps whole-range coverage when the current page is entirely %s", currentPage => {
    const data = summary({ input: 20, output: 20, cacheRead: 20, cacheCreation: 20 }, { totalRequests: 21, totalInputTokens: 200, totalOutputTokens: 40 });
    const logs = Array.from({ length: currentPage === "known" ? 20 : 1 }, (_, index) => request(currentPage === "known" ? complete : missing, { requestId: String(index + 1), inputTokens: currentPage === "known" ? 10 : 0, outputTokens: currentPage === "known" ? 2 : 0 }));
    const client = new QueryClient({ defaultOptions: { queries: { retry: false, gcTime: Infinity, staleTime: Infinity } } });
    client.setQueryData(["usage", { dateRange: "today" }, 0], { summary: data, logs, trends: [], ...contributions(data), sync: { codexImported: 0, harnessImported: 0, warnings: [] } });
    try {
      const html = renderToStaticMarkup(createElement(QueryClientProvider, { client }, createElement(UsagePage, { providers: [] })));
      expect(ledgerValues(html)).toEqual(["200（已知部分）", "40（已知部分）", "0（已知部分）", "0（已知部分）"]);
      expect(tokenPairs(html)).toEqual(["200（已知部分） / 40（已知部分）", "200（已知部分） / 40（已知部分）"]);
    } finally { client.clear(); }
  });

  it("translates unknown and partial labels in the production overview", () => {
    vi.stubGlobal("navigator", { language: "en-US", languages: ["en-US"] });
    const data = summary({ ...missing, input: 1 }, { totalRequests: 2, totalInputTokens: 11 });
    expect(ledgerValues(renderToStaticMarkup(createElement(UsageLedger, { summary: data })))).toEqual(["11 (known portion)", "Unknown", "Unknown", "Unknown"]);
  });

  it("keeps missing request totals and all four token/cost breakdowns unknown, including zero cache snapshots", () => {
    const html = details(request());
    expect(html).toContain("总 token / 推理 token</dt><dd>未知 / 未知</dd>");
    for (const label of ["输入", "输出", "缓存读取", "缓存写入"]) {
      expect(html).toContain(`<th scope="row">${label}</th><td>未知</td><td><span class="usage-amount">未知</span>`);
    }
    expect(html).not.toContain("$0.0000");
  });

  it("keeps each detail cost tied to its own token coverage", () => {
    const html = details(request({ input: 1, output: 0, cacheRead: 0, cacheCreation: 1 }, { inputTokens: 11, inputCostUsd: "0.0110", outputCostUsd: "0.0000" }));
    expect(html).toContain("总 token / 推理 token</dt><dd>11（已知部分） / 未知</dd>");
    expect(html).toContain('<th scope="row">输入</th><td>11</td><td><span class="usage-amount">$0.0110</span>');
    expect(html).toContain('<th scope="row">缓存写入</th><td>0</td><td><span class="usage-amount">$0.0000</span>');
    for (const label of ["输出", "缓存读取"]) {
      expect(html).toContain(`<th scope="row">${label}</th><td>未知</td><td><span class="usage-amount">未知</span>`);
    }
  });
});

describe("usage cost coverage compatibility", () => {
  it.each(["0.0000", "0.1250"])("marks legacy provider and model cost %s as a known portion", totalCostUsd => {
    const stats = contributions(summary(missing, { totalRequests: 2, totalCostUsd }));
    for (const stat of [...stats.providerStats, ...stats.modelStats]) delete stat.unknownCostRequests;
    const html = renderToStaticMarkup(createElement(UsageContributions, { ...stats, providerNames, onInspect: () => {} }));
    const expected = "$" + totalCostUsd + "（已知部分）";
    expect(html.match(/\$[\d.]+（已知部分）/g)).toEqual([expected, expected]);
    expect(html).not.toContain(">$" + totalCostUsd + "</td>");
  });

  it.each([null, undefined, ""])("keeps missing legacy cost %s unknown even when records exist", cost => {
    expect(aggregateCostLabel(cost, undefined, 2)).toBe("未知");
  });

  it.each([undefined, 0])("does not invent partial cost coverage for an empty range with coverage %s", unknownCount => {
    expect(aggregateCostLabel("0.0000", unknownCount, 0)).toBe("$0.0000");
    expect(aggregateCostLabel("", unknownCount, 0)).toBe("未知");
  });
});
