/// <reference types="vite/client" />
import promptsSource from "./features/prompts/PromptsPage.tsx?raw";
import mcpSource from "./features/mcp/McpPage.tsx?raw";
import sessionsSource from "./features/sessions/SessionsPage.tsx?raw";
import skillsSource from "./features/skills/SkillsPage.tsx?raw";
import { describe, expect, it } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";
import { ConfirmActionDialog } from "./components/ConfirmActionDialog";

const render = (props: Partial<Parameters<typeof ConfirmActionDialog>[0]> = {}) =>
  renderToStaticMarkup(<ConfirmActionDialog title="删除记录" message="此操作无法撤销。"
    onCancel={() => {}} onConfirm={() => {}} {...props} />);

describe("destructive confirmation presentation", () => {
  it("exposes an accessible, labelled application alertdialog", () => {
    const html = render();
    expect(html).toContain('role="alertdialog"');
    expect(html).toContain('aria-modal="true"');
    expect(html).toMatch(/aria-labelledby="([^"]+)"/);
    expect(html).toMatch(/aria-describedby="([^"]+)"/);
    const titleId = html.match(/aria-labelledby="([^"]+)"/)![1];
    const messageId = html.match(/aria-describedby="([^"]+)"/)![1];
    expect(html).toContain('id="' + titleId + '">删除记录');
    expect(html).toContain('id="' + messageId + '"');
  });
  it("offers separate cancel and explicit delete choices", () => {
    const html = render();
    expect(html).toContain(">取消</button>");
    expect(html).toContain(">确认删除</button>");
    expect(html.match(/<button /g)).toHaveLength(2);
    expect(html.match(/type="button"/g)).toHaveLength(2);
  });
  it("supports a specific uninstall label without removing cancel", () => {
    const html = render({ confirmLabel: "确认卸载" });
    expect(html).toContain(">确认卸载</button>");
    expect(html).toContain(">取消</button>");
    expect(html).not.toContain("确认删除");
  });
  it("disables both choices and announces busy state during mutation", () => {
    const html = render({ busy: true });
    expect(html).toContain('aria-busy="true"');
    expect(html.match(/disabled=""/g)).toHaveLength(2);
    expect(html).toContain("处理中…");
    expect(html).not.toContain("确认删除");
  });
  it("shows the backend failure while retaining the dialog and retry choices", () => {
    const html = render({ error: "卸载技能尚未实现" });
    expect(html).toContain('role="alert"');
    expect(html).toContain("卸载技能尚未实现");
    expect(html).toContain('role="alertdialog"');
    expect(html).toContain(">确认删除</button>");
    expect(html).not.toContain('disabled=""');
  });
  it("does not announce an empty failure", () => {
    expect(render()).not.toContain('role="alert"');
  });
  it("escapes untrusted names and backend errors instead of rendering markup", () => {
    const html = render({ message: '<img src=x onerror="bad()">', error: "<script>bad()</script>" });
    expect(html).toContain("&lt;img");
    expect(html).toContain("&lt;script&gt;");
    expect(html).not.toContain("<script>");
    expect(html).not.toContain("<img ");
  });
});

// Browser cancel/confirm, Escape and focus behavior are exercised by the native acceptance harness.
describe("destructive views use explicit application confirmation", () => {
  for (const [file, source] of Object.entries({ PromptsPage: promptsSource, McpPage: mcpSource, SessionsPage: sessionsSource, SkillsPage: skillsSource })) {
    it(file + " does not treat the native dialog Promise as a boolean", () => {
      expect(source).not.toMatch(/window\.confirm\s*\(/);
      expect(source).toContain('from "../../components/ConfirmActionDialog"');
      expect(source).toContain("<ConfirmActionDialog");
      expect(source).toContain("onConfirm=");
    });
  }
  it("removal awaits the operation and closes only after success", () => {
    const source = skillsSource;
    expect(source).toContain('await uninstallSkill(pendingRemove.id)');
    expect(source).toContain('if (await op.run(async () =>');
    expect(source).toContain('setPendingRemove(null)');
    expect(source).toContain('不删除任何源文件');
    expect(source).not.toContain('mutationFn: (id: string) => runAction(() => uninstallSkill(id))');
  });
});
