import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { en } from "./en";

const { invokeMock } = vi.hoisted(() => ({ invokeMock: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: invokeMock }));
let storage: Map<string, string>;
let events: EventTarget;
let system: { language: string; languages: string[] };

beforeEach(() => {
  vi.resetModules();
  invokeMock.mockReset();
  storage = new Map();
  events = new EventTarget();
  system = { language: "zh-CN", languages: ["zh-CN"] };
  vi.stubGlobal("localStorage", { getItem: (key: string) => storage.get(key) ?? null, setItem: (key: string, value: string) => storage.set(key, value) });
  vi.stubGlobal("navigator", system);
  vi.stubGlobal("window", events);
  vi.stubGlobal("document", { documentElement: { lang: "" } });
});
afterEach(() => { vi.unstubAllGlobals(); vi.restoreAllMocks(); });
function native() { Object.assign(events, { __TAURI_INTERNALS__: {} }); }
function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (error: Error) => void;
  const promise = new Promise<T>((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}

// Tests use stable application fixtures rather than the runner's language.
describe("interface language", () => {
  it("validates preferences and resolves the first system language", async () => {
    const { normalizeLanguagePreference, resolveLanguage } = await import("./language");
    expect(normalizeLanguagePreference("en")).toBe("en");
    expect(normalizeLanguagePreference("zh-CN")).toBe("zh-CN");
    for (const value of [null, "fr", "en-US", "", 0, {}]) expect(normalizeLanguagePreference(value)).toBe("system");
    expect(resolveLanguage("system", ["zh-TW", "en-US"])).toBe("zh-CN");
    expect(resolveLanguage("system", ["en-US", "zh-CN"])).toBe("en");
    expect(resolveLanguage("system", ["fr-FR"])).toBe("en");
    expect(resolveLanguage("zh-CN", ["en-US"])).toBe("zh-CN");
    expect(resolveLanguage("en", ["zh-CN"])).toBe("en");
  });
  it("updates labels, locale and document language without native IPC in preview", async () => {
    const language = await import("./language");
    const labels = language.localizedLabels({ healthy: "健康", custom: "用户名称" });
    expect(labels.healthy).toBe("健康");
    await language.setLanguagePreference("en");
    expect(language.t("设置")).toBe("Settings");
    expect(language.localeTag()).toBe("en-US");
    expect(document.documentElement.lang).toBe("en");
    expect(storage.get(language.LANGUAGE_STORAGE_KEY)).toBe("en");
    expect(labels.healthy).toBe(en["健康"]);
    expect(labels.custom).toBe("用户名称");
    expect(invokeMock).not.toHaveBeenCalled();
    await language.setLanguagePreference("zh-CN");
    expect(labels.healthy).toBe("健康");
    expect(language.localeTag()).toBe("zh-CN");
  });
  it("restores the local preference before native hydration", async () => {
    storage.set("xy-switch:language", "en");
    const language = await import("./language");
    await language.initializeLanguage();
    expect(language.currentLanguage()).toBe("en");
    expect(document.documentElement.lang).toBe("en");
  });
  it("does not switch when native persistence fails", async () => {
    native();
    invokeMock.mockRejectedValue(new Error("synthetic save failure"));
    const language = await import("./language");
    await expect(language.setLanguagePreference("en")).rejects.toThrow("synthetic save failure");
    expect(language.currentLanguage()).toBe("zh-CN");
    expect(storage.has(language.LANGUAGE_STORAGE_KEY)).toBe(false);
  });
  it("serializes selections and retains the last successfully saved choice", async () => {
    native();
    const first = deferred<void>();
    invokeMock.mockImplementationOnce(() => first.promise).mockResolvedValue(undefined);
    const language = await import("./language");
    const one = language.setLanguagePreference("en");
    const two = language.setLanguagePreference("zh-CN");
    await vi.waitFor(() => expect(invokeMock).toHaveBeenCalledTimes(1));
    first.resolve(undefined);
    await Promise.all([one, two]);
    expect(invokeMock.mock.calls.map(([, args]) => args.preference)).toEqual(["en", "zh-CN"]);
    expect(language.languagePreference()).toBe("zh-CN");
  });
  it("continues its write queue after a failed choice", async () => {
    native();
    invokeMock.mockRejectedValueOnce(new Error("save failed")).mockResolvedValue(undefined);
    const language = await import("./language");
    const one = language.setLanguagePreference("en").catch(() => undefined);
    const two = language.setLanguagePreference("zh-CN");
    await Promise.all([one, two]);
    expect(language.languagePreference()).toBe("zh-CN");
    expect(invokeMock).toHaveBeenCalledTimes(2);
  });
  it("hydrates the database preference and synchronizes native menus once", async () => {
    native();
    invokeMock.mockResolvedValueOnce("en").mockResolvedValue(undefined);
    const language = await import("./language");
    await Promise.all([language.initializeLanguage(), language.initializeLanguage()]);
    expect(language.currentLanguage()).toBe("en");
    expect(invokeMock.mock.calls).toEqual([["get_interface_language"], ["set_interface_language", { preference: "en", resolvedLanguage: "en" }]]);
    await language.initializeLanguage();
    expect(invokeMock).toHaveBeenCalledTimes(2);
  });
  it("does not let late native hydration overwrite an in-flight selection", async () => {
    native();
    const saved = deferred<string>();
    invokeMock.mockImplementation((command: string) => command === "get_interface_language" ? saved.promise : Promise.resolve(undefined));
    const language = await import("./language");
    const hydration = language.initializeLanguage();
    await vi.waitFor(() => expect(invokeMock).toHaveBeenCalledWith("get_interface_language"));
    await language.setLanguagePreference("en");
    saved.resolve("zh-CN");
    await hydration;
    expect(language.currentLanguage()).toBe("en");
    expect(invokeMock.mock.calls.filter(([command]) => command === "set_interface_language")).toEqual([["set_interface_language", { preference: "en", resolvedLanguage: "en" }]]);
  });
  it("allows retry after startup preference loading fails", async () => {
    native();
    vi.spyOn(console, "error").mockImplementation(() => undefined);
    invokeMock.mockRejectedValueOnce(new Error("read failed")).mockResolvedValueOnce("en").mockResolvedValue(undefined);
    const language = await import("./language");
    await language.initializeLanguage();
    expect(language.currentLanguage()).toBe("zh-CN");
    await language.retryLanguageSynchronization();
    expect(language.currentLanguage()).toBe("en");
    expect(invokeMock).toHaveBeenCalledTimes(3);
  });
  it("follows system changes and synchronizes the resolved native menu language", async () => {
    native();
    invokeMock.mockResolvedValue(undefined);
    const language = await import("./language");
    await language.setLanguagePreference("system");
    system.languages = ["en-GB"];
    events.dispatchEvent(new Event("languagechange"));
    expect(language.currentLanguage()).toBe("en");
    expect(document.documentElement.lang).toBe("en");
    await vi.waitFor(() => expect(invokeMock).toHaveBeenLastCalledWith("set_interface_language", { preference: "system", resolvedLanguage: "en" }));
    await language.setLanguagePreference("zh-CN");
    const count = invokeMock.mock.calls.length;
    events.dispatchEvent(new Event("languagechange"));
    expect(language.currentLanguage()).toBe("zh-CN");
    expect(invokeMock).toHaveBeenCalledTimes(count);
  });
  it("synchronizes a preference changed by another WebView", async () => {
    native();
    invokeMock.mockResolvedValue(undefined);
    const language = await import("./language");
    storage.set(language.LANGUAGE_STORAGE_KEY, "en");
    const event = Object.assign(new Event("storage"), { key: language.LANGUAGE_STORAGE_KEY });
    events.dispatchEvent(event);
    expect(language.currentLanguage()).toBe("en");
    await vi.waitFor(() => expect(invokeMock).toHaveBeenLastCalledWith("set_interface_language", { preference: "en", resolvedLanguage: "en" }));
  });
  it("survives restricted WebView storage without losing a native preference", async () => {
    native();
    vi.stubGlobal("localStorage", { getItem: () => { throw new Error("restricted"); }, setItem: () => { throw new Error("restricted"); } });
    invokeMock.mockResolvedValue(undefined);
    const language = await import("./language");
    await language.setLanguagePreference("en");
    expect(language.currentLanguage()).toBe("en");
    expect(invokeMock).toHaveBeenCalledWith("set_interface_language", { preference: "en", resolvedLanguage: "en" });
  });
  it("keeps all navigation entries and their stable view keys in both languages", async () => {
    const language = await import("./language");
    const { SideNav, navigation } = await import("../shell/SideNav");
    for (const preference of ["zh-CN", "en"] as const) {
      await language.setLanguagePreference(preference);
      const html = renderToStaticMarkup(createElement(SideNav, { activeView: "providers", providerCount: 3, onNavigate: () => undefined }));
      expect(html.match(/class="xy-shell-nav-item"/g)).toHaveLength(8);
      for (const item of navigation) expect(html).toContain(`title="${language.t(item.label).replaceAll("&", "&amp;")}"`);
    }
    expect(navigation.map(item => item.view)).toEqual(["providers", "mcp", "prompts", "skills", "sessions", "usage", "governance", "settings"]);
  });
  it("localizes notifications in either direction while preserving opaque user values", async () => {
    const language = await import("./language");
    const user = "我的供应商 <script> 中文原文";
    const source = "模型列表不是有效 JSON：{error}";
    const zh = language.translate(source, { error: user }, "zh-CN");
    const english = language.translate(source, { error: user }, "en");
    await language.setLanguagePreference("en");
    expect(language.translateMessage(zh)).toBe(english);
    expect(language.translateMessage(english)).toBe(english);
    expect(language.translateMessage(user)).toBe(user);
    expect(language.t("未知用户提示词 {model}", { model: "gpt-custom" })).toBe("未知用户提示词 gpt-custom");
    await language.setLanguagePreference("zh-CN");
    expect(language.translateMessage(english)).toBe(zh);
  });
  it("preserves zero-valued substitutions and raw diagnostic tails", async () => {
    const language = await import("./language");
    await language.setLanguagePreference("en");
    const message = "已新增 0 个技能引用；已跳过 50 个已导入项；源文件未修改";
    expect(language.translateMessage(message)).toBe("Added 0 skill references; skipped 50 previously imported items; source files unchanged");
    const prefix = "语言设置保存失败：";
    const diagnostic = '{"model":"中文自定义","status":500}';
    expect(language.translateMessage(prefix + diagnostic)).toBe(en[prefix] + diagnostic);
    expect(language.translate("{count}/{missing}", { count: 0, missing: null }, "en")).toBe("0/—");
  });
  it("translates transport prefixes without changing opaque diagnostic tails", async () => {
    const language = await import("./language");
    const prefixes = ["连接超时，请检查网络、系统代理及上游服务：", "连接失败，请检查 DNS、TLS 证书及 Windows 系统代理：", "网络请求失败："];
    const tail = 'HTTP 502 {"model":"我的模型","url":"https://example.invalid/v1"}';
    await language.setLanguagePreference("en");
    for (const prefix of prefixes) expect(language.translateMessage(prefix + tail)).toBe(en[prefix] + tail);
    await language.setLanguagePreference("zh-CN");
    for (const prefix of prefixes) expect(language.translateMessage(en[prefix] + tail)).toBe(prefix + tail);
  });
  it("localizes application backend notes and desktop capability errors", async () => {
    const language = await import("./language");
    await language.setLanguagePreference("en");
    expect(language.translateMessage("客户端接管仅适用于桌面端；Android / iOS 不能访问其他应用的 CLI 配置或会话目录")).toBe("Client takeover is desktop-only; Android / iOS cannot access another application's CLI configuration or session directories");
    expect(language.translateMessage("未知的日志等级：custom")).toBe("Unknown log level: custom");
    expect(language.t("本地文件 SHA256 只证明文件内容，不证明下载来源或模型身份。")).toBe(en["本地文件 SHA256 只证明文件内容，不证明下载来源或模型身份。"]);
  });
  it("does not translate provider names, model identifiers or technical recognition", async () => {
    const language = await import("./language");
    const { inferProviderIcon, normalizeProvider } = await import("../lib/providers");
    await language.setLanguagePreference("en");
    expect(inferProviderIcon("custom", "智谱自定义")).toBe("/providers/zhipu.svg");
    const provider = normalizeProvider({ id: "custom", name: "我的供应商", group: "Codex", endpoint: "https://example.invalid/v1", model: "custom/中文-model", enabled: true });
    expect(provider.name).toBe("我的供应商");
    expect(provider.model).toBe("custom/中文-model");
  });
});


