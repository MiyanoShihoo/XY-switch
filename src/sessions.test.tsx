import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { afterEach, describe, expect, it } from "vitest";
import { SessionDetailPanel, SessionsPage } from "./features/sessions/SessionsPage";
import { SessionReader } from "./features/sessions/SessionReader";
import { groupSessionsByProject, sessionProjectLabels } from "./features/sessions/SessionDirectory";
import { sessionPreviewMessages } from "./features/sessions/SessionPreview";
import { sessionDiscussion } from "./features/sessions/SessionDiscussion";
import type { SessionCatalog, SessionDetail, SessionMessage, SessionMeta } from "./types/session";
import { setLanguagePreference } from "./i18n/language";

afterEach(async () => { await setLanguagePreference("system"); });

const session: SessionMeta = { providerId: "codex", sessionId: "session-1", title: "A local conversation", projectDir: "QA:/workspace", resumeCommand: "codex resume session-1" };
type RenderOptions = { catalog?: SessionCatalog; detail?: SessionDetail; catalogError?: Error; detailError?: Error; favoriteError?: string; busy?: boolean };
function render(options: RenderOptions = {}, panel: boolean | "inline" = false) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false, retryOnMount: false, gcTime: Infinity, staleTime: Infinity } } });
  const selected = options.catalog?.sessions[0] ?? session;
  const listKey = ["sessions", "list"];
  const messagesKey = ["sessions", "messages", selected.providerId, selected.sessionId];
  if (options.catalogError) client.getQueryCache().build(client, { queryKey: listKey }).setState({ status: "error", error: options.catalogError, fetchStatus: "idle" });
  else client.setQueryData(listKey, options.catalog ?? { sessions: [session], warnings: [] });
  if (options.detailError) {
    if (options.detail) client.setQueryData(messagesKey, options.detail);
    client.getQueryCache().build(client, { queryKey: messagesKey }).setState({ status: "error", error: options.detailError, fetchStatus: "idle" });
  }
  else client.setQueryData(messagesKey, options.detail ?? { messages: [{ role: "user", content: "Inspect the local logs", ts: null }], warnings: [] });
  const content = panel ? createElement(panel === "inline" ? SessionReader : SessionDetailPanel, {
    session: selected, favorite: false, busy: options.busy ?? false, favoriteError: options.favoriteError,
    onClose: () => {}, onFavorite: () => {}, onCopyResume: () => {}, onDelete: () => {}, onDismissMessage: () => {}, onRetryFavorite: () => {},
    onExpand: () => {}, onCopyDirectory: () => {},
  }) : createElement(SessionsPage);
  const html = renderToStaticMarkup(createElement(QueryClientProvider, { client }, content));
  client.clear();
  return html;
}
const renderDetail = (options: RenderOptions = {}) => render(options, true);

describe("session catalog and detail presentation", () => {
  it("keeps one accessible row action and previews the latest exchange before full reading", () => {
    const html = render({ detail: { warnings: [], messages: [
      { role: "user", content: "Earlier context" },
      { role: "assistant", content: "Earlier answer" },
      { role: "user", content: "Current question" },
      { role: "assistant", content: "Current answer" },
    ] } });
    expect(html).toContain('aria-label="仅看收藏" aria-pressed="false"');
    expect(html).toContain('title="按 Esc 清除搜索"');
    const rows = html.match(/<div class="session-index__row"[\s\S]*?<\/button><\/div>/g) ?? [];
    expect(rows).toHaveLength(1);
    expect(rows[0]?.match(/<button /g)).toHaveLength(1);
    expect(rows[0]).toContain('aria-label="预览会话 A local conversation"');
    expect(rows[0]).toContain('aria-current="true"');
    expect(html).not.toContain('role="dialog"');
    expect(html).toContain('aria-label="收藏会话 A local conversation"');
    expect(html).toContain("Current question");
    expect(html).toContain("Current answer");
    expect(html).not.toContain("Earlier context");
    expect(html).not.toContain("Earlier answer");
    expect(html).not.toContain('class="session-message"');
    expect(html).toContain("从此处阅读");
    expect(html).toContain("阅读全文");
    expect(html).not.toContain('class="session-reading-pane"');
  });
  it("groups by the complete project path without merging identically named projects", () => {
    const projects = [
      { ...session, sessionId: "active", projectDir: "QA:/active/shared" },
      { ...session, sessionId: "archived", projectDir: "QA:/archive/shared" },
      { ...session, sessionId: "active-2", projectDir: "QA:/active/shared" },
    ];
    expect(groupSessionsByProject(projects).map(group => [group.path, group.entries.map(entry => entry.sessionId)]))
      .toEqual([["QA:/active/shared", ["active", "active-2"]], ["QA:/archive/shared", ["archived"]]]);
    const html = render({ catalog: { sessions: projects, warnings: [] } });
    expect(html.match(/class="session-index__row"/g)).toHaveLength(3);
    expect(html).toContain('value="&quot;QA:/active/shared&quot;"');
    expect(html).toContain('value="&quot;QA:/archive/shared&quot;"');
    expect(html).toContain("QA:/active/shared (2)");
    expect(html).toContain("QA:/archive/shared (1)");
    expect(html).toContain("active / shared");
    expect(html).toContain("archive / shared");
    expect(html).toContain("筛选工作目录");
    expect(html).toContain("最近活动");
  });
  it("builds discussion navigation from loaded user messages without removing transcript content", () => {
    const messages = [
      { role: "system", content: "Keep the original evidence" },
      { role: "user", content: "First question\nwith details" },
      { role: "tool", content: "Unabridged tool output" },
      { role: "assistant", content: "Full response" },
      { role: "user", content: "Another question" },
    ];
    expect(sessionDiscussion(messages)).toEqual([{ index: 1, title: "First question with details" }, { index: 4, title: "Another question" }]);
    const html = renderDetail({ detail: { messages, warnings: [] } });
    const discussion = html.match(/<section class="session-discussion"[\s\S]*?<\/section>/)?.[0] ?? "";
    expect(discussion).toContain("First question with details");
    expect(discussion.match(/<button /g)).toHaveLength(2);
    expect(discussion).not.toContain("Unabridged tool output");
    for (const message of messages) expect(html).toContain(message.content);
    expect(html.match(/class="session-message"/g)).toHaveLength(messages.length);
    expect(html).toMatch(/data-message-index="4"[^>]*tabindex="-1"/);
  });
  it("disambiguates repeated parent names and translates missing project labels", async () => {
    const projects = [
      { ...session, sessionId: "one", projectDir: "QA:/one/active/shared" },
      { ...session, sessionId: "two", projectDir: "QA:/two/active/shared" },
      { ...session, sessionId: "windows", projectDir: "C:\\team\\release\\shared" },
      { ...session, sessionId: "missing", projectDir: null },
    ];
    const labels = sessionProjectLabels(projects);
    expect(labels.get("QA:/one/active/shared")).toBe("one / active / shared");
    expect(labels.get("QA:/two/active/shared")).toBe("two / active / shared");
    expect(labels.get("C:\\team\\release\\shared")).toBe("release / shared");
    expect(labels.get("")).toBe("未记录工作目录");
    await setLanguagePreference("en");
    expect(sessionProjectLabels(projects).get("")).toBe("Working directory not recorded");
  });
  it("previews the latest user question and final non-tool reply at their original indices", () => {
    const messages: SessionMessage[] = [
      { role: "system", content: "Original instruction" },
      { role: "user", content: "Earlier question" },
      { role: "assistant", content: "Earlier answer" },
      { role: "user", content: "Latest question" },
      { role: "assistant", content: "Read logs", tool: { callId: "read", name: "read_logs", eventKind: "call", status: "requested" } },
      { role: "tool", content: "Exact log bytes", tool: { callId: "read", name: "read_logs", eventKind: "result", status: "completed" } },
      { role: "assistant", content: "Latest answer" },
      { role: "assistant", content: "Finish tool", tool: { callId: "finish", name: "finish", eventKind: "call", status: "requested" } },
    ];
    const preview = sessionPreviewMessages(messages);
    expect(preview.start).toBe(3);
    expect(preview.excerpts.map(item => [item.index, item.message.content])).toEqual([[3, "Latest question"], [6, "Latest answer"]]);
    expect(preview.otherCount).toBe(3);
    expect(preview.discussionCount).toBe(2);
    expect(preview.excerpts[1].message).toBe(messages[6]);
    const html = render({ detail: { messages, warnings: [] } });
    expect(html).toContain("Latest question");
    expect(html).toContain("Latest answer");
    expect(html).not.toContain("Exact log bytes");
    expect(html).toContain("3 条其他记录可在全文中查看");
    const full = renderDetail({ detail: { messages, warnings: [] } });
    for (const message of messages) expect(full).toContain(message.content);
    expect(full.match(/class="session-message"/g)).toHaveLength(messages.length);
  });
  it("does not borrow an earlier answer for an unanswered latest question", () => {
    const messages = [
      { role: "user", content: "Earlier question" },
      { role: "assistant", content: "Earlier answer" },
      { role: "user", content: "Still awaiting a reply" },
    ];
    expect(sessionPreviewMessages(messages)).toEqual({ start: 2, excerpts: [{ index: 2, message: messages[2] }], otherCount: 0, discussionCount: 2 });
    expect(sessionPreviewMessages([])).toEqual({ start: 0, excerpts: [], otherCount: 0, discussionCount: 0 });
  });
  it("retains original indices when falling back to records without user messages", () => {
    const messages = [
      { role: "system", content: "Initial instruction" },
      { role: "assistant", content: "First recorded reply" },
      { role: "assistant", content: "Latest recorded reply" },
    ];
    expect(sessionPreviewMessages(messages)).toEqual({ start: 1, excerpts: [{ index: 1, message: messages[1] }, { index: 2, message: messages[2] }], otherCount: 0, discussionCount: 0 });
  });
  it("removes cached excerpts and transcript after a detail refresh error", () => {
    const options = { detail: { messages: [{ role: "user", content: "Cached question" }, { role: "assistant", content: "Cached answer" }], warnings: [] }, detailError: new Error("Source is unavailable") };
    for (const html of [render(options), render(options, "inline")]) {
      expect(html).toContain("Source is unavailable");
      expect(html).toContain("重试");
      expect(html).not.toContain("Cached question");
      expect(html).not.toContain("Cached answer");
      expect(html).not.toContain("没有可读取的消息");
      expect(html).not.toContain("2 条消息");
    }
  });
  it("translates preview, filters and resume steps without translating stored values", async () => {
    await setLanguagePreference("en");
    const html = render();
    for (const label of ["Preview session A local conversation", "Filter working directory", "Latest exchange · Excerpts", "Read from here", "Read from start", "Copy working directory", "Copy resume command", "Then use this command in your terminal"]) expect(html).toContain(label);
    for (const raw of [session.providerId, session.projectDir, session.resumeCommand, "Inspect the local logs"]) expect(html).toContain(raw);
    expect(html).not.toContain("最近一轮");
  });
  it("shows metadata and supported clients, with resume and favorite actions in the reader", () => {
    const list = render({ catalog: { sessions: [session, { ...session, providerId: "claude" }, { ...session, providerId: "gemini" }], warnings: [] } });
    expect(list).toContain("A local conversation");
    expect(list).toContain("QA:/workspace");
    for (const provider of ["codex", "claude", "gemini"]) expect(list).toContain('<option>' + provider + '</option>');
    const detail = renderDetail();
    expect(detail).toContain('role="dialog"');
    expect(detail).toContain("Inspect the local logs");
    expect(detail).toContain("codex resume session-1");
    expect(detail).toContain('aria-label="收藏会话 A local conversation"');
  });
  it("keeps readable messages alongside partial-read warnings", () => {
    const list = render({ catalog: { sessions: [session], warnings: ["Some source folders were skipped"] } });
    const detail = renderDetail({ detail: { messages: [{ role: "tool", content: "Valid tool output" }], warnings: ["The final JSONL entry is incomplete"] } });
    expect(list).toContain("Some source folders were skipped");
    expect(detail).toContain("The final JSONL entry is incomplete");
    expect(detail).toContain("Valid tool output");
    expect(detail).toContain('data-role="tool"');
  });
  it("does not misreport a catalog failure as an empty local history", () => {
    const html = render({ catalogError: new Error("Permission denied for session directory") });
    expect(html).toContain("会话读取失败：");
    expect(html).toContain("Permission denied for session directory");
    expect(html).not.toContain("尚未发现本地会话");
  });
  it("does not show a no-messages success state after a detail read failure", () => {
    const html = renderDetail({ detailError: new Error("Ambiguous session identifier") });
    expect(html).toContain("消息读取失败：");
    expect(html).toContain("Ambiguous session identifier");
    expect(html).not.toContain("没有可读取的消息");
  });
  it("distinguishes empty browser history from a metadata-only session", () => {
    const empty = render({ catalog: { sessions: [], warnings: [] } });
    expect(empty).toContain("尚未发现本地会话");
    expect(empty).toContain("浏览器预览不读取本地会话；请在桌面端查看历史。");
    expect(renderDetail({ detail: { messages: [], warnings: [] } })).toContain("没有可读取的消息");
  });
  it("escapes untrusted titles, message bodies and warnings", () => {
    const options = { catalog: { sessions: [{ ...session, title: "<script>title()</script>" }], warnings: ["<img src=x onerror=bad()>"] }, detail: { messages: [{ role: "assistant", content: "<script>message()</script>" }], warnings: [] } };
    const html = render(options) + renderDetail(options);
    expect(html).toContain("&lt;script&gt;");
    expect(html).toContain("&lt;img");
    expect(html).not.toContain("<script>");
    expect(html).not.toContain("<img ");
  });
  it("does not offer a working-looking resume action without a native command", () => {
    const html = renderDetail({ catalog: { sessions: [{ ...session, resumeCommand: null }], warnings: [] } });
    expect(html).toMatch(/<button[^>]*disabled=""[^>]*title="此客户端未提供恢复命令"/);
  });
  it("keeps favorite storage failures and retry reachable inside the reader", () => {
    const html = renderDetail({ favoriteError: "Local favorites could not be saved" });
    expect(html).toContain("Local favorites could not be saved");
    expect(html).toContain("重试");
  });

  it("keeps reader actions available and complete metadata in the collapsed information section", () => {
    const title = "A complete session title that remains available when the narrow heading is clamped ".repeat(4).trim();
    const html = render({ catalog: { sessions: [{ ...session, title, sourcePath: "QA:/sessions/session-1.jsonl" }], warnings: [] } }, "inline");
    const heading = html.match(/<header class="session-reader__heading">[\s\S]*?<\/header>/)?.[0] ?? "";
    const resume = html.match(/<section class="session-resume-context"[\s\S]*?<\/section>/)?.[0] ?? "";
    const source = html.match(/<details class="workspace-disclosure session-source"[\s\S]*?<\/details>/)?.[0] ?? "";
    expect(heading).toContain("返回会话目录");
    expect(heading).toContain(title);
    expect(heading).toContain("放大阅读");
    expect(heading).toContain(session.providerId);
    for (const value of ["复制恢复命令", session.projectDir, session.resumeCommand, "先切换工作目录", "再在终端使用恢复命令", "复制工作目录"]) expect(resume).toContain(value);
    expect(resume.indexOf("先切换工作目录")).toBeLessThan(resume.indexOf("再在终端使用恢复命令"));
    expect(html).toContain('aria-label="收藏会话 ' + title + '"');
    expect(html).not.toContain("刷新索引");
    expect(source).toContain("会话信息与管理");
    expect(source).not.toContain(' open=""');
    for (const value of [title, "工作目录", session.projectDir, "客户端", session.providerId, "会话 ID", session.sessionId, "QA:/sessions/session-1.jsonl", session.resumeCommand, "删除会话", "恢复命令只复制，不执行"]) {
      expect(source).toContain(value);
    }
    expect(html).toContain("Inspect the local logs");
  });

  it("keeps navigation, favorite, resume, expansion and deletion disabled during an operation", () => {
    const html = render({ busy: true }, "inline");
    const buttons = html.match(/<button\b[^>]*>[\s\S]*?<\/button>/g) ?? [];
    for (const action of ["返回会话目录", "放大阅读", "收藏会话 A local conversation", "复制恢复命令", "复制工作目录", "删除会话"]) {
      expect(buttons.find(button => button.includes(action)), action).toContain('disabled=""');
    }
    expect(html).toContain('aria-label="查找会话正文"');
    expect(html).toContain("Inspect the local logs");
  });

  it("shows search, approvals and exact tool results with links to their associated calls", () => {
    const html = renderDetail({ detail: { warnings: [], messages: [
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
    const html = renderDetail({ detail: { messages, warnings: [] } });
    const calls = [...html.matchAll(/<article[^>]* id="([^"]+)"/g)].map(match => match[1]);
    const links = [...html.matchAll(/href="#([^"]+)"/g)].map(match => match[1]);
    expect(new Set(calls).size).toBe(2);
    expect(links).toEqual(calls);
    expect(html.match(/Repeated output/g)).toHaveLength(2);
  });

  it("leaves legacy execution status unknown and escapes tool names and IDs", () => {
    const html = renderDetail({ detail: { warnings: [], messages: [
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
    const html = renderDetail({ detail: { warnings: [], messages: [
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
    const html = renderDetail({ detail: { warnings: [], messages: [
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
