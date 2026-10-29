import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { describe, expect, it } from "vitest";
import { ModelPricingModal } from "./ModelPricingModal";
import type { ModelPricing } from "../../types/usage";
const row = (id = "qa/model"): ModelPricing => ({ id, inputPerMillion: 2.5, outputPerMillion: 10, cacheReadPerMillion: 0, cacheCreationPerMillion: null, source: "custom", updatedAt: 123 });
function render(data?: ModelPricing[], error?: Error) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false, retryOnMount: false, gcTime: Infinity, staleTime: Infinity } } });
  if (data) client.setQueryData(["model-pricing"], data);
  if (error) client.getQueryCache().build(client, { queryKey: ["model-pricing"] }).setState({ status: "error", error, fetchStatus: "idle" });
  try { return renderToStaticMarkup(createElement(QueryClientProvider, { client }, createElement(ModelPricingModal, { onClose() {} }))); }
  finally { client.clear(); }
}
describe("model pricing dialog states", () => {
  it("explains precise IDs, local estimates, cache uncertainty and future-only edits", () => {
    const html = render([]);
    for (const text of ["模型定价", "USD / 百万 token", "不代表供应商账单", "完整模型 ID 精确匹配", "历史费用不会重算", "留空表示未知，0 表示免费", "没有匹配的定价"]) expect(html).toContain(text);
    expect(html).toContain('role="dialog"');
    expect(html).toContain('type="submit"');
    expect(html).toContain('required=""');
  });
  it("distinguishes loading from an empty catalog", () => {
    const html = render();
    expect(html).toContain("读取定价目录");
    expect(html).not.toContain("没有匹配的定价");
  });
  it("does not turn a catalog read error into an empty success", () => {
    const html = render(undefined, new Error("database is locked"));
    expect(html).toContain('role="alert"');
    expect(html).toContain("database is locked");
    expect(html).not.toContain("没有匹配的定价");
  });
  it("shows free and unknown cache rates separately and escapes model names", () => {
    const html = render([row("qa/<script>")]);
    expect(html).toContain("0 / 未知");
    expect(html).toContain("qa/&lt;script&gt;");
    expect(html).not.toContain("qa/<script>");
  });
  it("initially lists only custom prices and pages the catalog", () => {
    const html = render([...Array.from({ length: 26 }, (_, i) => row("qa/" + String(i).padStart(2, "0"))), { ...row("reference/hidden"), source: "models.dev" }]);
    expect(html).toContain("qa/24");
    expect(html).not.toContain("qa/25");
    expect(html).not.toContain("reference/hidden");
    expect(html).toContain("共 26 条定价");
  });
});
