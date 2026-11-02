import { renderToStaticMarkup } from "react-dom/server";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { TaskDetailView, TaskSummaryView } from "./features/governance/TaskPanel";
import { budgetReasonLabel, taskIdentityLabel } from "./features/governance/task-format";
import { setLanguagePreference } from "./i18n/language";
import type { ProxyTask, ProxyTaskDetail } from "./types/governance";

const task: ProxyTask = {
  id: "task-one", identitySource: "codex_turn", maxAttempts: 3, maxMicrousd: 3168,
  attempts: 3, reservedMicrousd: 2112, committedMicrousd: 2112, knownMicrousd: 6,
  unknownCostAttempts: 2, unreservedAttempts: 1, activeAttempts: 0, cancelled: true,
  createdAt: 1_800_000_000, updatedAt: 1_800_000_001,
};
const detail: ProxyTaskDetail = {
  task,
  attempts: [
    { id: "send-one", requestId: "client-one", ordinal: 1, providerId: "primary", model: "unit-model", route: "openai_chat", reservedTokens: 1040, reservedMicrousd: 1056, status: "http_error", statusCode: 503, errorKind: "upstream_status", eventId: 1, inputTokens: null, outputTokens: null, actualMicrousd: null, usageState: "missing", costState: "unknown", startedAt: 1_800_000_000_000, finishedAt: 1_800_000_000_100 },
    { id: "send-two", requestId: "client-one", ordinal: 2, providerId: "fallback", model: "unit-model", route: "openai_chat", reservedTokens: 1040, reservedMicrousd: 1056, status: "completed", statusCode: 200, errorKind: null, eventId: 2, inputTokens: 4, outputTokens: 1, actualMicrousd: 6, usageState: "complete", costState: "known", startedAt: 1_800_000_000_200, finishedAt: 1_800_000_000_300 },
    { id: "send-three", requestId: "client-retry", ordinal: 3, providerId: "fallback", model: "<script>private()</script>", route: "openai_chat", reservedTokens: null, reservedMicrousd: null, status: "cancelled", statusCode: null, errorKind: "request_cancelled", eventId: 3, inputTokens: null, outputTokens: null, actualMicrousd: null, usageState: "partial", costState: "unknown", startedAt: 1_800_000_000_400, finishedAt: 1_800_000_000_500 },
  ],
};

beforeEach(async () => { await setLanguagePreference("zh-CN"); });
afterEach(async () => { await setLanguagePreference("system"); });

describe("persistent task budget presentation", () => {
  it("keeps microdollar overages, absent limits and zero limits distinct", () => {
    const html = renderToStaticMarkup(<TaskSummaryView task={{ ...task, attempts: 0, maxAttempts: null, maxMicrousd: 0, reservedMicrousd: 1, committedMicrousd: 2, knownMicrousd: 0 }} />);
    expect(html).toContain("任务累计尝试 / 上限</dt><dd>0 / 未设置");
    expect(html).toContain("0.000002");
    expect(html).toContain("0.000001");
    expect(html).toContain("已知实际费用</dt><dd>US$0.00");
    expect(html).toContain("费用未结清的尝试</dt><dd>2");
    expect(html).toContain("历史无预留尝试</dt><dd>1");
    expect(html).not.toContain("NaN");
  });

  it("shows each physical send, client retry, failover and unknown cost without inventing usage", () => {
    const html = renderToStaticMarkup(<TaskDetailView detail={detail} />);
    expect(html).toContain("尝试 1 · 上游 HTTP 失败");
    expect(html).toContain("尝试 2 · 已完成");
    expect(html).toContain("尝试 3 · 已取消");
    for (const identifier of ["send-one", "send-two", "send-three", "client-one", "client-retry", "primary", "fallback"]) expect(html).toContain(identifier);
    expect(html).toContain("0.000006");
    expect(html).toContain("实际费用</dt><dd>未知");
    expect(html).toContain("预留费用</dt><dd>未预留");
    expect(html).toContain("用量状态</dt><dd>部分 · — / —");
    expect(html).toContain("任务已取消，后续重试已阻断。");
    expect(html).toContain("&lt;script&gt;private()&lt;/script&gt;");
    expect(html).not.toContain("<script>");
    expect(html).not.toContain("request_cancelled");
  });

  it("explains production rejection codes and leaves future codes available for diagnosis", () => {
    expect(budgetReasonLabel("budget_exact_custom_price_required")).toBe("实际派发模型缺少精确自定义价格。");
    expect(budgetReasonLabel("task_attempt_budget_exhausted")).toBe("单任务总尝试上限已耗尽。");
    expect(budgetReasonLabel("task_cost_budget_exhausted")).toBe("本次预留会超过单任务费用占用上限。");
    expect(budgetReasonLabel("future_task_rule")).toBe("future_task_rule");
    expect(taskIdentityLabel("future_identity")).toBe("未知");
  });

  it("renders task history and rejection reasons completely in English", async () => {
    await setLanguagePreference("en");
    const html = renderToStaticMarkup(<TaskDetailView detail={detail} />);
    expect(html).toContain("Task details");
    expect(html).toContain("Task committed cost / limit");
    expect(html).toContain("Codex conversation turn");
    expect(html).toContain("Actual cost</dt><dd>Unknown");
    expect(html).not.toMatch(/\p{Script=Han}/u);
    expect(budgetReasonLabel("budget_exact_custom_price_required")).toBe("The dispatched model has no exact custom price.");
    expect(budgetReasonLabel("task_identity_required")).toBe("A stable task ID is required to count client retries together.");
  });

  it("shows a zero-send task with no fabricated upstream attempts", () => {
    const html = renderToStaticMarkup(<TaskDetailView detail={{ task: { ...task, attempts: 0 }, attempts: [] }} />);
    expect(html).toContain("此任务尚未发送上游请求。");
    expect(html).not.toContain("HTTP 200");
  });
});
