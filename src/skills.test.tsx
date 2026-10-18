import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { afterEach, describe, expect, it, vi } from "vitest";
import { SkillsPage } from "./features/skills/SkillsPage";
import { importSkillFolders, skillImportMessage } from "./lib/skill-import";
import type { InstalledSkill, SkillCatalog, SkillImportResult } from "./types/skill";

const { invokeMock } = vi.hoisted(() => ({ invokeMock: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: invokeMock }));
afterEach(() => { vi.unstubAllGlobals(); invokeMock.mockReset(); vi.resetModules(); });

const imported = (path: string, added = 2, alreadyImported = 0): SkillImportResult => ({ path, matched: added + alreadyImported, added, alreadyImported, warnings: [] });
function skill(name: string): InstalledSkill {
  return { id: name, name, description: "A separated skill description", directory: "C:/Users/operator/.codex/skills/" + name,
    source: "codex", readOnly: true, apps: { claude: false, codex: true, gemini: false, grokbuild: false, opencode: false, hermes: false, pi: false }, installedAt: 0, updatedAt: 0 };
}
function catalog(change: Partial<SkillCatalog> = {}): SkillCatalog {
  return { skills: [skill("nested-system-skill")], warnings: [], isolated: false,
    sources: [{ source: "codex", directory: "C:/Users/operator/.codex/skills", enabled: true, discovered: 50, status: "ready" }], ...change };
}
function render(data: SkillCatalog) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false, gcTime: Infinity, staleTime: Infinity } } });
  client.setQueryData(["skills", "settings"], { codex: true, deepseekHarness: false, claude: false, gemini: false, agents: false });
  client.setQueryData(["skills", "catalog"], data);
  const html = renderToStaticMarkup(createElement(QueryClientProvider, { client }, createElement(SkillsPage)));
  client.clear();
  return html;
}

describe("Skills 批量导入", () => {
  it("处理全部拖入目录并合并重复路径，不只取第一个", async () => {
    const read = vi.fn(async (path: string) => imported(path));
    const result = await importSkillFolders(["A", "B", "A", "C"], read);
    expect(read.mock.calls.map(([path]) => path)).toEqual(["A", "B", "C"]);
    expect(result.results).toHaveLength(3);
    expect(result.warnings).toEqual([]);
    expect(skillImportMessage(result.results)).toContain("新增 6 个技能引用");
  });
  it("一个目录失败不会丢掉其余成功目录", async () => {
    const result = await importSkillFolders(["bad", "good"], async path => {
      if (path === "bad") throw new Error("not found");
      return { ...imported(path), warnings: ["nested file skipped"] };
    });
    expect(result.results).toHaveLength(1);
    expect(result.warnings).toEqual(["bad：not found", "nested file skipped"]);
  });
  it("全部失败会报错而不是宣称导入成功", async () => {
    await expect(importSkillFolders(["bad"], async () => { throw "permission denied"; })).rejects.toThrow("permission denied");
    await expect(importSkillFolders([], async path => imported(path))).rejects.toThrow("没有可导入");
  });
  it("重复导入的反馈保留新增和已存在计数", () => {
    expect(skillImportMessage([imported("root", 0, 50)])).toBe("新增 0 个技能引用，50 个已导入项已跳过；源文件未修改");
  });
});

describe("Skills 路径与数量显示", () => {
  it("显示后端实际绝对路径和识别数量，不再渲染只读标签", () => {
    const html = render(catalog());
    expect(html).toContain("C:/Users/operator/.codex/skills");
    expect(html).toContain("已识别 50 个技能");
    expect(html).toContain("nested-system-skill");
    expect(html).toContain('class="skill-record__copy"');
    expect(html).not.toContain("只读");
    expect(html).not.toContain("隔离测试窗口");
  });
  it("隔离路径明确标注测试窗口，不会伪装成真实 Codex 目录", () => {
    const data = catalog({ isolated: true, sources: [{ source: "codex", directory: "E:/qa/home/.codex/skills", enabled: true, discovered: 1, status: "ready" }] });
    const html = render(data);
    expect(html).toContain("隔离测试窗口");
    expect(html).toContain("E:/qa/home/.codex/skills");
    expect(html).toContain("已识别 1 个技能");
  });
  it.each(["missing", "error", "partial"] as const)("显示来源 %s 状态", state => {
    const data = catalog();
    data.sources[0].status = state;
    const html = render(data);
    expect(html).toContain({ missing: "目录不存在", error: "读取失败", partial: "部分项目未读取" }[state]);
  });
  it("所有识别项都进入列表，含系统和嵌套技能，不做首项截断", () => {
    const html = render(catalog({ skills: Array.from({ length: 50 }, (_, i) => skill("skill-" + i)) }));
    expect(html.match(/class="skill-record"/g)).toHaveLength(50);
  });
  it("文件元数据按文本转义，不执行 Skill 中的 HTML", () => {
    const html = render(catalog({ skills: [skill('<script>alert("x")</script>')] }));
    expect(html).not.toContain("<script>");
    expect(html).toContain("&lt;script&gt;");
  });
});

describe("Skills 本地 IPC", () => {
  it("网页预览为空且没有 IPC 请求", async () => {
    vi.stubGlobal("window", {});
    const service = await import("./lib/skill-service");
    expect(await service.getSkillCatalog()).toEqual({ skills: [], warnings: [], sources: [], isolated: false });
    await expect(service.importSkillFolder("C:/skills")).rejects.toThrow("桌面端");
    expect(invokeMock).not.toHaveBeenCalled();
  });
  it("批量导入结果完整返回，取消目录选择不伪造结果", async () => {
    vi.stubGlobal("window", { __TAURI_INTERNALS__: {} });
    const service = await import("./lib/skill-service");
    invokeMock.mockResolvedValueOnce(imported("C:/skills", 50));
    expect(await service.importSkillFolder("C:/skills")).toEqual(imported("C:/skills", 50));
    expect(invokeMock).toHaveBeenCalledWith("import_skill_folder", { path: "C:/skills" });
    invokeMock.mockResolvedValueOnce(null);
    expect(await service.pickAndImportSkillFolder()).toBeNull();
  });
});
