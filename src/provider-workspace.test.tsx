import { renderToStaticMarkup as render } from "react-dom/server";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { ProvidersPage } from "./features/providers/ProvidersPage";
import { ProviderConfigurationPage } from "./features/providers/ProviderConfigurationPage";
import { ProviderDiagnosticsPage } from "./features/providers/ProviderDiagnosticsPage";
import { ProviderHealthSummary } from "./features/providers/ProviderHealthSummary";
import { AppShell } from "./shell/AppShell";
import { createProviderAdvanced } from "./lib/providers";
import type { ProvidersController } from "./features/providers/useProviders";
import type { Provider, ProviderHealth } from "./types/provider";

function provider(change: Partial<Provider> = {}): Provider {
  return { id: "relay", name: "Relay", group: "Codex", endpoint: "https://qa.invalid/v1", model: "qa-model", enabled: true,
    icon: "/app-icon.png", advanced: { ...createProviderAdvanced("openai_chat"), modelMappings: [{ model: "logical-model", displayName: "Display model", contextWindow: 131072 }],
      headerOverrides: { Authorization: "DO_NOT_RENDER_HEADER_SECRET" }, bodyOverrides: { embedded: "DO_NOT_RENDER_BODY_SECRET" } }, ...change };
}
function controller(change: Partial<ProvidersController> = {}): ProvidersController {
  return { providers: [provider()], currentId: "relay", loading: false, busy: false, testing: null, error: "", message: "", health: {},
    refresh: vi.fn(async () => true), activate: vi.fn(async () => {}), copy: vi.fn(async () => {}), remove: vi.fn(async () => true),
    reorder: vi.fn(async () => {}), test: vi.fn(async () => {}), saved: vi.fn(), dismissMessage: vi.fn(), ...change };
}
const callbacks = { onBack: vi.fn(), onDiagnostics: vi.fn(), onConfigure: vi.fn(), onSelect: vi.fn(), onProviders: vi.fn(), onRuntimeChanged: vi.fn(async () => {}) };
const detail = (c: ProvidersController, id = "relay") => render(<ProviderConfigurationPage controller={c} providerId={id} {...callbacks} />);
const diagnostics = (c: ProvidersController, id?: string) => render(<ProviderDiagnosticsPage controller={c} providerId={id} {...callbacks} />);
function buttonAttributes(html: string, label: string): string {
  const button = [...html.matchAll(/<button\b([^>]*)>([\s\S]*?)<\/button>/g)].find(match => match[2].replace(/<[^>]*>/g, "").trim() === label);
  expect(button, label).toBeDefined();
  return button![1];
}
const health = (change: Partial<ProviderHealth> = {}): ProviderHealth => ({ providerId: "relay", status: "unknown", latencyMs: null, successRate: 0,
  consecutiveFailures: 0, circuitOpenUntil: null, lastError: null, lastCheckedAt: 0, ...change });
beforeEach(() => { vi.clearAllMocks(); });
afterEach(() => { vi.unstubAllGlobals(); });

describe("full-width provider workspace", () => {
  it("keeps the home list free of an inspector, drawer, splitter or model probe", () => {
    const c = controller();
    const html = render(<ProvidersPage controller={c} {...callbacks} />);
    expect(html).toContain('class="provider-workspace"'); expect(html).toContain('role="list"');
    expect(html).toContain("连接诊断"); expect(html).toContain("查看 Relay 详情"); expect(html).toContain("手柄支持拖动与键盘排序");
    for (const absent of ['role="separator"', 'role="dialog"', "provider-inspector", "provider-response-probe", "provider-configuration-page"]) expect(html).not.toContain(absent);
    expect(c.activate).not.toHaveBeenCalled(); expect(c.test).not.toHaveBeenCalled(); expect(callbacks.onConfigure).not.toHaveBeenCalled();
  });
  it("does not describe an untested current provider as healthy", () => {
    const html = render(<ProvidersPage controller={controller()} {...callbacks} />);
    expect(html).toContain("未测试"); expect(html).not.toContain("测试通过");
  });
});

describe("independent configuration details", () => {
  it("shows mappings and management actions without activation or secret contents", () => {
    const c = controller(); const html = detail(c);
    for (const text of ["logical-model", "Display model", "131072", "Chat Completions", "自动追加路径", "编辑模型映射", "复制（不含密钥）", "删除供应商", "打开连接诊断"]) expect(html).toContain(text);
    expect(html).not.toContain("DO_NOT_RENDER_HEADER_SECRET"); expect(html).not.toContain("DO_NOT_RENDER_BODY_SECRET");
    expect(c.activate).not.toHaveBeenCalled(); expect(c.copy).not.toHaveBeenCalled(); expect(c.remove).not.toHaveBeenCalled(); expect(c.test).not.toHaveBeenCalled();
  });
  it("protects built-in deletion while retaining configuration editing", () => {
    const html = detail(controller({ providers: [provider({ builtIn: true })] }));
    expect(buttonAttributes(html, "删除供应商")).toContain('disabled=""');
    expect(buttonAttributes(html, "编辑配置")).not.toContain("disabled");
  });
  it("allows viewing disabled providers but disables activation", () => {
    const html = detail(controller({ providers: [provider({ enabled: false })], currentId: "another" }));
    expect(html).toContain("已禁用"); expect(buttonAttributes(html, "设为当前")).toContain('disabled=""');
    expect(buttonAttributes(html, "编辑配置")).not.toContain("disabled");
  });
  it("does not fall back to a different provider for an invalid detail URL", () => {
    const html = detail(controller(), "missing"); expect(html).toContain("所选供应商已不存在"); expect(html).toContain("返回供应商");
    expect(html).not.toContain("logical-model");
  });
});

describe("independent connection diagnostics", () => {
  it("has linked, keyboard-ready tabs and no automatic activation or connection test", () => {
    const c = controller(); const html = diagnostics(c);
    expect(html).toContain('aria-label="诊断供应商"'); expect(html).toContain('role="tablist"');
    const tabs = [...html.matchAll(/role="tab"[^>]*aria-controls="([^"]+)"[^>]*tabindex="(-?\d+)"/g)];
    expect(tabs).toHaveLength(2); expect(tabs.map(match => match[2])).toEqual(["0", "-1"]);
    for (const match of tabs) expect(html).toContain('id="' + match[1] + '"');
    expect(html).toContain('hidden=""'); expect(html).not.toContain("低 Token 验证设置");
    expect(c.activate).not.toHaveBeenCalled(); expect(c.test).not.toHaveBeenCalled(); expect(callbacks.onSelect).not.toHaveBeenCalled();
  });
  it("keeps a missing diagnostic target explicit instead of silently selecting another", () => {
    const html = diagnostics(controller(), "missing"); expect(html).toContain("所选供应商已不存在"); expect(html).not.toContain('role="tablist"');
  });
  it("cannot test a disabled provider, including test-all when none are enabled", () => {
    const c = controller({ providers: [provider({ enabled: false })] }); const html = diagnostics(c);
    expect(buttonAttributes(html, "测试连接")).toContain('disabled=""'); expect(buttonAttributes(html, "测试全部")).toContain('disabled=""');
    expect(html).toContain("可查看历史结果"); expect(c.test).not.toHaveBeenCalled();
  });
  it("keeps unknown health, time and success rate distinct from a successful test", () => {
    const html = render(<ProviderHealthSummary provider={provider()} health={health()} busy={false} testing={false} onTest={vi.fn()} onConfigure={vi.fn()} />);
    expect(html).toContain("状态未知"); expect(html).toContain("无记录"); expect(html).toContain("没有连接测试记录");
    expect(html).not.toContain("0.0%"); expect(html).not.toContain("测试通过");
  });
  it("shows actual historical errors rather than masking a failed connection", () => {
    const html = render(<ProviderHealthSummary provider={provider()} health={health({ status: "unhealthy", successRate: 25, lastError: "QA connection refused", lastCheckedAt: 1793340000000 })} busy={false} testing={false} onTest={vi.fn()} onConfigure={vi.fn()} />);
    expect(html).toContain("QA connection refused"); expect(html).toContain("25.0%"); expect(html).not.toContain("测试通过");
  });
  it("translates the grouped shell, details and diagnostic labels in English", () => {
    vi.stubGlobal("navigator", { language: "en-US", languages: ["en-US"] });
    const c = controller();
    const html = render(<AppShell activeView="diagnostics" onNavigate={vi.fn()} providerCount={1} currentProviderName="Relay" proxyStatus={null} onToggleProxy={vi.fn()} theme="light" onToggleTheme={vi.fn()}>{<ProviderDiagnosticsPage controller={c} {...callbacks} />}</AppShell>) + detail(c);
    expect(html).toContain("Observability"); expect(html).toContain("Model responses and verification"); expect(html).toContain("Configuration details");
    expect(html).not.toMatch(/[\u3400-\u9fff]/u);
  });
});
