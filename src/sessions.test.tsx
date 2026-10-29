import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { describe, expect, it } from "vitest";
import { SessionsPage } from "./features/sessions/SessionsPage";
import type { SessionCatalog, SessionDetail, SessionMeta } from "./types/session";

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
});
