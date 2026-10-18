import { renderToStaticMarkup as render } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { Button, DataTable, Field, InlineError, Input, Modal, Tabs, Toggle } from "./index";

describe("aligned-workspace interaction states", () => {
  it("associates field labels, hints and errors with the control", () => {
    const html = render(<Field label="API 地址" description="使用 HTTPS 地址" error="地址不能为空" required><Input name="endpoint" /></Field>);
    const id = html.match(/<label[^>]+for="([^"]+)"/)![1];
    expect(html).toContain('id="' + id + '"');
    expect(html).toContain('aria-describedby="' + id + '-description ' + id + '-error"');
    expect(html).toContain('aria-invalid="true"');
    expect(html).toContain('required=""');
  });
  it("marks pending actions busy and prevents duplicate submission", () => {
    const html = render(<Button loading>保存</Button>);
    expect(html).toContain('disabled=""');
    expect(html).toContain('aria-busy="true"');
    expect(html).toContain("保存");
  });
  it("does not render closed dialogs", () => {
    expect(render(<Modal open={false} title="隐藏" onClose={() => {}}>私有内容</Modal>)).toBe("");
  });
  it("exposes pending dialog and disabled close state", () => {
    const html = render(<Modal open busy title="保存中" onClose={() => {}}>内容</Modal>);
    expect(html).toContain('aria-busy="true"');
    expect(html).toContain('disabled=""');
  });
  it("keeps switch state readable to assistive technology", () => {
    const html = render(<Toggle checked disabled aria-label="启用服务器" />);
    expect(html).toContain('role="switch"');
    expect(html).toContain('aria-checked="true"');
    expect(html).toContain('disabled=""');
  });
  it("does not collide when two tab lists use identical values", () => {
    const html = render(<><Tabs items={[{ value: "all", label: "全部" }]} value="all" onChange={() => {}} /><Tabs items={[{ value: "all", label: "全部" }]} value="all" onChange={() => {}} /></>);
    const ids = [...html.matchAll(/id="([^"]+)"/g)].map(match => match[1]);
    expect(new Set(ids).size).toBe(ids.length);
    expect(html).not.toContain("aria-controls");
  });
  it("renders no error announcement until a real error exists", () => {
    expect(render(<InlineError />)).toBe("");
    expect(render(<InlineError message="读取失败" onRetry={() => {}} />)).toContain('role="alert"');
  });
  it("renders data with a caption and headers, not empty success metrics", () => {
    const columns = [{ key: "name", label: "名称", render: (row: { name: string }) => row.name }];
    expect(render(<DataTable columns={columns} rows={[]} rowKey={row => row.name} caption="请求记录" />)).toContain("暂无记录");
    const html = render(<DataTable columns={columns} rows={[{ name: "test" }]} rowKey={row => row.name} caption="请求记录" />);
    expect(html).toContain("请求记录");
    expect(html).toContain('scope="col"');
  });
});
