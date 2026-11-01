import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { afterEach, describe, expect, it } from "vitest";
import { SessionsPage } from "./features/sessions/SessionsPage";
import type { SessionCatalog, SessionDetail, SessionMeta } from "./types/session";
import { setLanguagePreference } from "./i18n/language";

afterEach(async () => { await setLanguagePreference("system"); });

const session: SessionMeta = { providerId: "codex", sessionId: "session-1", title: "A local conversation", projectDir: "QA:/workspace", resumeCommand: "codex resume session-1" };
function render(options: { catalog?: SessionCatalog; detail?: SessionDetail; catalogError?: Error; detailError?: Error } = {}) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false, retryOnMount: false, gcTime: Infinity, staleTime: Infinity } } });
  const listKey = ["sessions", "list"];
  const messagesKey = ["sessions", "messages", "codex", "session-1"];
  if (options.catalogError) client.getQueryCache().build(client, { queryKey: listKey }).setState({ status: "error", error: options.catalogError, fetchStatus: "idle" });
  else client.setQueryData(listKey, options.catalog ?? { sessions: [session], warnings: [] });
  if (options.detailError) client.getQueryCache().build(client, { queryKey: messagesKey }).setState({ status: "error", error: options.detailError, fetchStatus: "idle" });
  else client.setQueryData(messagesKey, options.detail ?? { messages: [{ role: "user", content: "Inspect the local logs", ts: null }], warnings: [] });
  const html = renderToStaticMarkup(createElement(QueryClientProvider, { client }, createElement(SessionsPage)));
  client.clear();
  return html;
}

describe("session catalog and detail presentation", () => {
  it("exposes separate accessible favorite controls without nesting row buttons", () => {
    const html = render();
    expect(html).toContain('aria-label="仅看收藏" aria-pressed="false"');
    expect(html).toContain('aria-label="收藏会话 A local conversation"');
    expect(html).toContain('title="按 Esc 清除搜索"');
    expect(html).toContain('aria-current="true"');
    expect(html).toMatch(/<div class="session-index__row"[^>]*><button[^>]*>[\s\S]*?<\/button><button[^>]*class="xy-ui-icon-button session-favorite"/);
  });
  it("shows real metadata, messages and supported clients", () => {
    const html = render();
    expect(html).toContain("A local conversation");
    expect(html).toContain("QA:/workspace");
    expect(html).toContain("Inspect the local logs");
    expect(html).toContain("Claude Code");
    expect(html).toContain("Gemini CLI");
    expect(html).toContain("codex resume session-1");
  });
  it("keeps readable messages alongside partial-read warnings", () => {
    const html = render({ catalog: { sessions: [session], warnings: ["Some source folders were skipped"] }, detail: { messages: [{ role: "tool", content: "Valid tool output" }], warnings: ["The final JSONL entry is incomplete"] } });
    expect(html).toContain("Some source folders were skipped");
    expect(html).toContain("The final JSONL entry is incomplete");
    expect(html).toContain("Valid tool output");
    expect(html).toContain('data-role="tool"');
  });
  it("does not misreport a catalog failure as an empty local history", () => {
    const html = render({ catalogError: new Error("Permission denied for session directory") });
    expect(html).toContain("会话读取失败：");
    expect(html).toContain("Permission denied for session directory");
    expect(html).not.toContain("尚未发现本地会话");
  });
  it("does not show a no-messages success state after a detail read failure", () => {
    const html = render({ detailError: new Error("Ambiguous session identifier") });
    expect(html).toContain("消息读取失败：");
    expect(html).toContain("Ambiguous session identifier");
    expect(html).not.toContain("没有可读取的消息");
  });
  it("distinguishes genuinely empty history from a metadata-only session", () => {
    expect(render({ catalog: { sessions: [], warnings: [] } })).toContain("尚未发现本地会话");
    expect(render({ detail: { messages: [], warnings: [] } })).toContain("没有可读取的消息");
  });
  it("escapes untrusted titles, message bodies and warnings", () => {
    const html = render({ catalog: { sessions: [{ ...session, title: "<script>title()</script>" }], warnings: ["<img src=x onerror=bad()>"] }, detail: { messages: [{ role: "assistant", content: "<script>message()</script>" }], warnings: [] } });
    expect(html).toContain("&lt;script&gt;");
    expect(html).toContain("&lt;img");
    expect(html).not.toContain("<script>");
    expect(html).not.toContain("<img ");
  });
  it("does not offer a working-looking resume action without a native command", () => {
    const html = render({ catalog: { sessions: [{ ...session, resumeCommand: null }], warnings: [] } });
    expect(html).toMatch(/<button[^>]*disabled=""[^>]*title="此客户端未提供恢复命令"/);
  });

  it("shows search, approvals and exact tool results with links to their associated calls", () => {
    const html = render({ detail: { warnings: [], messages: [
      { role: "assistant", content: "Search tools", tool: { callId: "search-a", name: "tool_search", eventKind: "search", status: "completed" } },
      { role: "tool", content: '{"tools":["receipt"]}', tool: { callId: "search-a", name: "tool_search", eventKind: "result", status: "completed" } },
      { role: "assistant", content: "Run tool", tool: { callId: "call-a", name: "mcp__local__receipt", eventKind: "call", status: "result_received" } },
      { role: "system", content: "Approval needed", tool: { callId: "call-a", name: "mcp__local__receipt", eventKind: "approval", status: "pending_approval" } },
      { role: "system", content: "Approved", tool: { callId: "call-a", name: "mcp__local__receipt", eventKind: "approval", status: "approved" } },
      { role: "tool", content: '{"ok":true}', tool: { callId: "call-a", name: "mcp__local__receipt", eventKind: "result", status: "result_received" } },
    ] } });
    expect(html).toContain("搜索工具");
    expect(html).toContain("审批记录");
    expect(html).toContain("等待审批");
    expect(html).toContain("已批准");
    expect(html).toContain("结果已收到");
    expect(html).toContain('{&quot;ok&quot;:true}');
    expect(html).not.toContain("执行成功");
    const calls = [...html.matchAll(/<article[^>]* id="([^"]+)"/g)].map(match => match[1]);
    const links = [...html.matchAll(/href="#([^"]+)"/g)].map(match => match[1]);
    expect(calls).toHaveLength(2);
    expect(links).toEqual([calls[0], calls[1], calls[1], calls[1]]);
  });

  it("associates reused IDs with the nearest preceding call and keeps repeated results", () => {
    const messages = [0, 1].flatMap(() => [
      { role: "assistant", content: "Repeated call", tool: { callId: "repeat", name: "check", eventKind: "call" as const, status: "succeeded" as const } },
      { role: "tool", content: "Repeated output", tool: { callId: "repeat", name: "check", eventKind: "result" as const, status: "succeeded" as const } },
    ]);
    const html = render({ detail: { messages, warnings: [] } });
    const calls = [...html.matchAll(/<article[^>]* id="([^"]+)"/g)].map(match => match[1]);
    const links = [...html.matchAll(/href="#([^"]+)"/g)].map(match => match[1]);
    expect(new Set(calls).size).toBe(2);
    expect(links).toEqual(calls);
    expect(html.match(/Repeated output/g)).toHaveLength(2);
  });

  it("leaves legacy execution status unknown and escapes tool names and IDs", () => {
    const html = render({ detail: { warnings: [], messages: [
      { role: "tool", content: "Legacy output" },
      { role: "assistant", content: "Call", tool: { callId: '"><script>metadata()</script>', name: '<img src=x onerror="bad()">', eventKind: "call", status: "requested" } },
      { role: "tool", content: "Result", tool: { callId: '"><script>metadata()</script>', name: null, eventKind: "result", status: "failed" } },
    ] } });
    expect(html).toContain("执行状态未知");
    expect(html).toContain("执行失败");
    expect(html).toContain("&lt;script&gt;");
    expect(html).toContain("&lt;img");
    expect(html).not.toContain("<script>");
    expect(html).not.toContain("<img ");
  });

  it("links hosted approvals to the explicitly referenced call even before that reused ID appears again", () => {
    const html = render({ detail: { warnings: [], messages: [
      { role: "assistant", content: "Earlier call", tool: { callId: "reused", name: "lookup", eventKind: "call", status: "failed" } },
      { role: "system", content: "Approval needed", tool: { callId: "reused", approvalRequestId: "approval-next", name: "lookup", eventKind: "approval", status: "pending_approval" } },
      { role: "system", content: "Approved", tool: { callId: "reused", approvalRequestId: "approval-next", name: "lookup", eventKind: "approval", status: "approved" } },
      { role: "assistant", content: "Next call", tool: { callId: "reused", approvalRequestId: "approval-next", name: "lookup", eventKind: "call", status: "completed" } },
      { role: "tool", content: "Exact result", tool: { callId: "reused", approvalRequestId: "approval-next", name: "lookup", eventKind: "result", status: "completed" } },
    ] } });
    const calls = [...html.matchAll(/<article[^>]* id="([^"]+)"/g)].map(match => match[1]);
    const links = [...html.matchAll(/href="#([^"]+)"/g)].map(match => match[1]);
    expect(calls).toHaveLength(2);
    expect(links).toEqual([calls[1], calls[1], calls[1]]);
    expect(html).toContain("审批 ID");
    expect(html).toContain("approval-next");
  });

  it("translates tool event labels and keeps protocol identifiers intact", async () => {
    await setLanguagePreference("en");
    const html = render({ detail: { warnings: [], messages: [
      { role: "assistant", content: "Search", tool: { callId: "search", name: "tool_search", eventKind: "search", status: "completed" } },
      { role: "tool", content: "Result", tool: { callId: "search", name: "tool_search", eventKind: "result", status: "result_received" } },
    ] } });
    expect(html).toContain("Tool search");
    expect(html).toContain("Result received");
    expect(html).toContain("View associated call search");
    expect(html).toContain("tool_search");
    expect(html).not.toContain("搜索工具");
  });
});
