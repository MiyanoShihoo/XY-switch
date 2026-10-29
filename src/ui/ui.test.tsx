import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { Badge, Button, Card, Field, Input, Modal, Tabs } from "./index";

describe("aligned-workspace UI primitives", () => {
  it("renders semantic button variants without leaking implementation props", () => {
    const html = renderToStaticMarkup(<Button variant="secondary" size="sm" icon={<span>+</span>}>新增</Button>);
    expect(html).toContain('class="xy-ui-button"');
    expect(html).toContain('data-variant="secondary"');
    expect(html).toContain('data-size="sm"');
    expect(html).toContain(">新增</button>");
    expect(html).not.toMatch(/\\svariant=/);
  });

  it("keeps status tone and dot explicit", () => {
    const html = renderToStaticMarkup(<Badge tone="success" dot>已连接</Badge>);
    expect(html).toContain('data-tone="success"');
    expect(html).toContain('class="xy-ui-status-dot"');
    expect(html).toContain("已连接");
  });

  it("renders field structure around the new input primitive", () => {
    const html = renderToStaticMarkup(
      <Field label="Endpoint" description="Provider 的连接地址" required>
        <Input name="endpoint" />
      </Field>,
    );
    expect(html).toContain('class="xy-ui-field"');
    expect(html).toContain('class="xy-ui-field__description"');
    expect(html).toContain('class="xy-ui-field__required"');
    expect(html).toContain('class="xy-ui-input"');
  });

  it("exposes tab semantics and active state", () => {
    const html = renderToStaticMarkup(
      <Tabs value="providers" onChange={() => {}} items={[{ value: "providers", label: "供应商", count: 3 }, { value: "models", label: "模型" }]} />,
    );
    expect(html).toContain('role="tablist"');
    expect(html).toContain('role="tab"');
    expect(html).toContain('aria-selected="true"');
    expect(html).toContain("供应商");
  });

  it("keeps modal semantics available for the future shell", () => {
    const html = renderToStaticMarkup(<Modal open title="编辑供应商" onClose={() => {}}>内容</Modal>);
    expect(html).toContain('role="dialog"');
    expect(html).toContain('aria-modal="true"');
    expect(html).toContain("编辑供应商");
  });

  it("supports flat, low-noise surfaces instead of forcing card content", () => {
    const html = renderToStaticMarkup(<Card padding="lg">路由内容</Card>);
    expect(html).toContain('class="xy-ui-card"');
    expect(html).toContain('data-padding="lg"');
    expect(html).toContain("路由内容");
  });
});
