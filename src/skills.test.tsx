import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { afterEach, describe, expect, it, vi } from "vitest";
import { SkillSources, SkillsPage } from "./features/skills/SkillsPage";
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
function render(data: SkillCatalog, error?: Error) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false, retryOnMount: false, gcTime: Infinity, staleTime: Infinity } } });
  client.setQueryData(["skills", "settings"], { codex: true, deepseekHarness: false, claude: false, gemini: false, agents: false });
  if (error) client.getQueryCache().build(client, { queryKey: ["skills", "catalog"] }).setState({ status: "error", error, fetchStatus: "idle" });
  else client.setQueryData(["skills", "catalog"], data);
  const html = renderToStaticMarkup(createElement(QueryClientProvider, { client }, createElement(SkillsPage)));
  client.clear();
  return html;
}

function renderSources(data: SkillCatalog) {
  return renderToStaticMarkup(createElement(SkillSources, {
    settings: { codex: true, deepseekHarness: false, claude: false, gemini: false, agents: false },
    catalog: data, busy: false, scanning: false, onChange: () => {},
  }));
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
    expect(skillImportMessage([imported("root", 0, 50)])).toBe("已新增 0 个技能引用；已跳过 50 个已导入项；源文件未修改");
  });
});

describe("Skills 路径与数量显示", () => {
  it("默认引用库保留路径和来源，授权与扫描属于独立管理任务", () => {
    const list = render(catalog());
    expect(list).toContain("nested-system-skill");
    expect(list).toContain('class="skill-record__select"');
    expect(list).toContain("C:/Users/operator/.codex/skills");
    expect(list).toContain('data-task="library"');
    expect(list).toContain("管理来源");
    expect(list).not.toContain('role="switch"');
    expect(list).toContain("XY Switch 引用库");
    expect(list).toContain("只读来源");
    expect(list).not.toContain("导入只添加引用，源文件保留在所选目录。");
    expect(list).not.toContain("刷新技能来源");
    expect(list).not.toContain("选择文件夹");
    expect(list).not.toContain('role="dialog"');
    const html = renderSources(catalog());
    expect(html).toContain("C:/Users/operator/.codex/skills");
    expect(html).toContain("已识别 50 个技能");
    expect(html).not.toContain("只读");
    expect(html).not.toContain("隔离测试窗口");
  });
  it("来源授权默认全部关闭，未读取设置时禁用开关", () => {
    const html = renderToStaticMarkup(createElement(SkillSources, { catalog: catalog(), busy: false, scanning: false, onChange: () => {} }));
    const switches = html.match(/<button[^>]*role="switch"[^>]*>/g) ?? [];
    expect(switches).toHaveLength(5);
    expect(switches.every(button => button.includes('aria-checked="false"') && button.includes('disabled=""'))).toBe(true);
    expect(html).toContain("默认全部关闭");
  });
  it("来源问题只在默认目录留下管理入口，详细错误不占据查找空间", () => {
    const html = render(catalog({ warnings: ["Synthetic source warning"] }));
    expect(html).toContain("查看来源问题");
    expect(html).not.toContain("Synthetic source warning");
    expect(html).not.toContain('role="switch"');
  });
  it("隔离路径明确标注测试窗口，不会伪装成真实 Codex 目录", () => {
    const data = catalog({ isolated: true, sources: [{ source: "codex", directory: "E:/qa/home/.codex/skills", enabled: true, discovered: 1, status: "ready" }] });
    const html = render(data);
    expect(html).toContain("隔离测试窗口");
    const sourcePanel = renderSources(data);
    expect(sourcePanel).toContain("E:/qa/home/.codex/skills");
    expect(sourcePanel).toContain("已识别 1 个技能");
  });
  it.each(["missing", "error", "partial"] as const)("显示来源 %s 状态", state => {
    const data = catalog();
    data.sources[0].status = state;
    const html = renderSources(data);
    expect(html).toContain({ missing: "目录不存在", error: "读取失败", partial: "部分项目未读取" }[state]);
  });
  it("读取失败不显示成功空状态，浏览器空状态说明本地能力边界", () => {
    const failed = render(catalog({ skills: [] }), new Error("QA directory denied"));
    expect(failed).toContain("查看来源问题");
    expect(failed).not.toContain("QA directory denied");
    expect(failed).not.toContain("暂无可显示的技能");
    expect(failed).not.toContain("桌面端可读取本地技能");
    const preview = render(catalog({ skills: [] }));
    expect(preview).toContain("浏览器预览不读取本地文件，也不能导入技能。");
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
