import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { describe, expect, it } from "vitest";
import { PromptEditor, PromptsPage } from "./features/prompts/PromptsPage";
import { PromptReader } from "./features/prompts/PromptReader";
import type { Prompt } from "./types/prompt";

const prompt: Prompt = { id: "qa-review", name: "Review instructions", description: "Check boundary cases", content: "Full instructions\n".repeat(20), enabled: false };
function render(error?: Error) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false, retryOnMount: false, gcTime: Infinity, staleTime: Infinity } } });
  if (error) client.getQueryCache().build(client, { queryKey: ["prompts"] }).setState({ status: "error", error, fetchStatus: "idle" });
  else client.setQueryData(["prompts"], [prompt]);
  const html = renderToStaticMarkup(createElement(QueryClientProvider, { client }, createElement(PromptsPage)));
  client.clear();
  return html;
}

describe("focused prompt workspace", () => {
  it("starts with the full directory and waits for an explicit reading selection", () => {
    const html = render();
    const row = html.match(/<article class="prompt-record"[\s\S]*?<\/article>/)?.[0] ?? "";
    expect(row).toContain("Review instructions");
    expect(row).toContain("Check boundary cases");
    expect(row).toContain("已停用");
    expect(row.match(/<button /g)).toHaveLength(1);
    expect(html).not.toContain('role="dialog"');
    expect(row).toContain('aria-pressed="false"');
    expect(row).not.toContain(prompt.content);
    expect(html).toContain('data-view="library"');
    expect(html).not.toContain('class="prompt-document"');
    expect(html).not.toContain(prompt.content);
    expect(html).not.toContain("删除提示词");
  });
  it("keeps full plain text and explicit actions in the focused reader", () => {
    const html = renderToStaticMarkup(createElement(PromptReader, {
      prompt: { ...prompt, content: prompt.content + '<script>alert("qa")</script>' }, busy: false, editable: true,
      onCopy: () => {}, onEdit: () => {}, onToggle: () => {}, onDuplicate: () => {}, onDelete: () => {},
    }));
    expect(html).toContain(prompt.content);
    expect(html).toContain("&lt;script&gt;");
    expect(html).not.toContain("<script>");
    const beforeContent = html.slice(0, html.indexOf('class="prompt-preview"'));
    expect(beforeContent).toContain("复制内容");
    expect(beforeContent).toContain(">编辑<");
    expect(html).toContain("复制为新提示词");
    expect(html).toContain("删除提示词");
    expect(html).toContain("启用状态仅在提示词库中保存，不表示已注入客户端。");
    expect(html).toContain('aria-checked="false"');
  });
  it("keeps new prompts enabled by default and labels the editing state", () => {
    const html = renderToStaticMarkup(createElement(PromptEditor, { prompt: null, onClose: () => {}, onSaved: async () => {} }));
    expect(html).toContain('aria-checked="true"');
    expect(html).toContain("编辑中");
    expect(html).toContain("保存后返回阅读；取消可返回原位置。");
  });
  it("explains browser preview and disables native-only creation", () => {
    const html = render();
    expect(html).toContain("浏览器仅预览示例提示词；编辑与保存请使用桌面端。");
    expect(html).toMatch(/<button[^>]*disabled=""[^>]*>[\s\S]*?添加提示词<\/button>/);
  });
  it("does not turn read failures into an empty-library success state", () => {
    const html = render(new Error("QA prompts unavailable"));
    expect(html).toContain("QA prompts unavailable");
    expect(html).not.toContain("尚未添加提示词");
  });
  it("preserves full text and disabled status in the focused editor, escaping user content", () => {
    const content = prompt.content + '<script>alert("qa")</script>';
    const html = renderToStaticMarkup(createElement(PromptEditor, { prompt: { ...prompt, content }, onClose: () => {}, onSaved: async () => {} }));
    expect(html).toContain('<form class="prompt-editor"');
    expect(html).not.toContain('role="dialog"');
    expect(html).toContain('maxLength="100"');
    expect(html).toContain('maxLength="500"');
    expect(html).toContain('aria-checked="false"');
    expect(html).toContain(prompt.content);
    expect(html).toContain("&lt;script&gt;");
    expect(html).not.toContain("<script>");
    expect(html).toContain("保存提示词");
  });
});
