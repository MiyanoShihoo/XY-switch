import { renderToStaticMarkup as render } from "react-dom/server";
import { afterEach, describe, expect, it, vi } from "vitest";
import { createProviderAdvanced } from "../../lib/providers";
import type { Provider } from "../../types/provider";
import { ProviderChangeScope, ProviderSwitchConfirmation, type useProviderChangeScope } from "./ProviderChangeScope";
import { ProviderConfigurationPage } from "./ProviderConfigurationPage";
import { ProviderEditor } from "./ProviderEditor";
import { ProviderModelEditor } from "./ProviderModelEditor";
import { ProviderRow } from "./ProviderRow";
import { ProviderSectionNavigation, providerSections, type ProviderSection } from "./ProviderSectionNavigation";
import type { ProvidersController } from "./useProviders";

function provider(change: Partial<Provider> = {}): Provider {
  return {
    id: "relay", name: "Relay", group: "Codex", providerType: "custom", sortOrder: 17,
    endpoint: "https://relay.invalid/custom/messages/", model: "default-model", enabled: true, icon: "/app-icon.png",
    advanced: {
      ...createProviderAdvanced("anthropic"), isFullUrl: true, customUserAgent: "Relay/1.0",
      modelMappings: [{ model: "mapped-model", displayName: "Mapped alias", contextWindow: 131072 }],
      headerOverrides: { "x-provider": "HEADER_DRAFT_VALUE" }, bodyOverrides: { metadata: { label: "BODY_DRAFT_VALUE" } },
      codex: { remoteCompression: true, applyCommonConfig: true, context1m: true, compactionThreshold: 850000, configToml: 'model_reasoning_effort = "high"' },
      claude: { disableArtifactTool: true },
    }, ...change,
  };
}
function controller(change: Partial<ProvidersController> = {}): ProvidersController {
  return {
    providers: [provider()], currentId: "relay", loading: false, busy: false, testing: null, error: "", readError: "", message: "", health: {},
    refresh: vi.fn(async () => true), activate: vi.fn(async () => {}), copy: vi.fn(async () => {}), remove: vi.fn(async () => true),
    reorder: vi.fn(async () => {}), test: vi.fn(async () => {}), saved: vi.fn(), dismissMessage: vi.fn(), ...change,
  };
}
function scope(change: Partial<ReturnType<typeof useProviderChangeScope>> = {}): ReturnType<typeof useProviderChangeScope> {
  return { affected: ["Claude Code", "Codex", "Gemini CLI"], loading: false, error: "", ready: true, refresh: vi.fn(async () => {}), ...change };
}
function buttonAttributes(html: string, label: string): string {
  const button = [...html.matchAll(/<button\b([^>]*)>([\s\S]*?)<\/button>/g)]
    .find(match => match[2].replace(/<[^>]*>/g, "").trim() === label);
  expect(button, label).toBeDefined();
  return button![1];
}
function taskMarkup(html: string, section: string): string {
  const match = html.match(new RegExp('<section[^>]*data-provider-section="provider-edit-' + section + '">([\\s\\S]*?)</section>'));
  expect(match, section).not.toBeNull();
  return match![1];
}
const callbacks = () => ({ onSaved: vi.fn(), onClose: vi.fn(), onRuntimeChanged: vi.fn(async () => {}) });
const editor = (initialSection: ProviderSection = "connection") => render(<ProviderEditor provider={provider()} providers={[provider()]} initialSection={initialSection} {...callbacks()} />);
afterEach(() => vi.unstubAllGlobals());

describe("progressive provider configuration", () => {
  it("keeps the navigation export loadable and configuration details independently renderable", () => {
    const navigation = render(<ProviderSectionNavigation section="models" onSelect={vi.fn()} />);
    expect(navigation).toContain('aria-current="location"');
    const html = render(<ProviderConfigurationPage controller={controller()} providerId="relay" onBack={vi.fn()} onDiagnostics={vi.fn()} onRuntimeChanged={vi.fn(async () => {})} />);
    expect(html).toContain("Relay");
    expect(html).toContain("编辑模型映射");
    expect(html).not.toContain("provider-section-nav");
    expect(html).not.toContain("可拖动或用箭头");
    expect(html).not.toContain("HEADER_DRAFT_VALUE");
    expect(html).not.toContain("BODY_DRAFT_VALUE");
    expect(html).not.toContain("model_reasoning_effort");
  });

  it("shows connection fields first and retains every advanced draft while collapsed", () => {
    const html = editor();
    const connection = html.slice(html.indexOf('class="provider-connection-editor"'), html.indexOf('class="provider-task-list"'));
    for (const field of ["供应商名称", "供应商 ID", "API 地址", "完整请求 URL", "默认模型 ID", "上游协议", "API Key"]) expect(connection).toContain(field);
    expect(connection).toContain('value="https://relay.invalid/custom/messages/"');
    expect(connection).toContain('type="password"');
    expect(connection).toMatch(/type="password"[^>]*value=""/);
    expect(connection).toContain('value="anthropic" selected=""');
    for (const task of providerSections) {
      const section = taskMarkup(html, task.id);
      expect(section).toContain('aria-expanded="false"');
      expect(section).toContain('class="provider-task-section__body" hidden=""');
    }
    for (const value of ["mapped-model", "Mapped alias", "131072", "HEADER_DRAFT_VALUE", "BODY_DRAFT_VALUE", "Relay/1.0", "model_reasoning_effort", "850000", "禁用 Artifact 工具"]) expect(html).toContain(value);
    expect(html).not.toContain("provider-section-nav");
    expect(html).not.toContain("provider-document-layout");
  });

  it.each(["models", "request", "headers", "body", "codex", "claude", "advanced"] as const)("opens the requested %s task without exposing unrelated sections", initialSection => {
    const html = editor(initialSection);
    const expectedOpen = initialSection === "advanced" ? "request" : initialSection;
    for (const task of providerSections) {
      const section = taskMarkup(html, task.id);
      expect(section).toContain('aria-expanded="' + (task.id === expectedOpen) + '"');
      if (task.id === expectedOpen) expect(section).not.toContain('class="provider-task-section__body" hidden=""');
      else expect(section).toContain('class="provider-task-section__body" hidden=""');
    }
  });

  it("keeps a read-failed editor cancellable and disables saves and discovery", () => {
    const html = render(<ProviderEditor provider={null} providers={[]} readError="Unable to read providers" {...callbacks()} />);
    expect(html).toContain("Unable to read providers");
    expect(buttonAttributes(html, "保存配置")).toContain('disabled=""');
    expect(buttonAttributes(html, "探索可用模型")).toContain('disabled=""');
    expect(buttonAttributes(html, "取消")).not.toContain('disabled=""');
  });

  it("provides a direct current-model action independent of switching", () => {
    const html = render(<ProviderRow provider={provider()} current testing={false} busy={false} onInspect={vi.fn()} onActivate={vi.fn()} onEditModel={vi.fn()} />);
    const modelAction = buttonAttributes(html, "default-model");
    expect(modelAction).toContain('data-row-action="model"');
    expect(modelAction).toContain('class="provider-model-action"');
    expect(modelAction).toContain('aria-label="修改 Relay 的默认模型"');
    expect(modelAction).not.toContain('disabled=""');
    expect(buttonAttributes(html, "正在使用")).toContain('disabled=""');
  });

  it.each([{ busy: true, activationDisabled: false }, { busy: false, activationDisabled: true }])("locks model edits and sorting with %j", locks => {
    const html = render(<ProviderRow provider={provider()} current={false} testing={false} {...locks} onInspect={vi.fn()} onActivate={vi.fn()} onEditModel={vi.fn()} />);
    expect(buttonAttributes(html, "default-model")).toContain('disabled=""');
    expect(buttonAttributes(html, "设为当前")).toContain('disabled=""');
    expect(html).toMatch(/aria-label="拖动排序 Relay"[^>]*disabled=""/);
  });

  it("translates the editor, model dialog and shared impact messages in English", () => {
    vi.stubGlobal("navigator", { language: "en-US", languages: ["en-US"] });
    const html = editor("codex")
      + render(<ProviderModelEditor provider={provider()} current {...callbacks()} />)
      + render(<ProviderChangeScope applies scope={scope()} />);
    expect(html).toContain("Shared gateway impact");
    expect(html).toContain("Save model");
    expect(html).not.toMatch(/[\u3400-\u9fff]/u);
  });
});

describe("shared-gateway impact and switch confirmation", () => {
  it("lists every taken-over client rather than treating the provider group as routing scope", () => {
    const html = render(<ProviderChangeScope applies scope={scope()} />);
    for (const client of ["Claude Code", "Codex", "Gemini CLI"]) expect(html).toContain("<li>" + client + "</li>");
    expect(html).toContain("不会开启新的接管");
    expect(html).toContain("重启相关客户端");
  });

  it("still explains direct shared-gateway traffic when no clients are taken over", () => {
    const html = render(<ProviderChangeScope applies scope={scope({ affected: [] })} />);
    expect(html).toContain("没有已接管客户端");
    expect(html).toContain("直接连接共享网关的请求仍会使用当前配置");
  });

  it("keeps failed scope loading explicit and retryable", () => {
    const html = render(<ProviderChangeScope applies scope={scope({ error: "Scope read failed", ready: false })} />);
    expect(html).toContain("Scope read failed");
    expect(html).not.toContain("<li>Codex</li>");
    expect(buttonAttributes(html, "重试读取范围")).not.toContain('disabled=""');
  });

  it("does not claim a non-current or disabled provider changes the gateway", () => {
    const html = render(<ProviderChangeScope applies={false} scope={scope({ affected: [] })} />);
    expect(html).toContain("仅保存此配置");
    expect(html).toContain("保存不会改变当前连接");
    expect(html).not.toContain("<li>");
  });

  it("requires a scope read and explicit confirmation before switching", () => {
    const c = controller({ currentId: "other-provider" });
    const html = render(<ProviderSwitchConfirmation provider={provider()} controller={c} onClose={vi.fn()} />);
    expect(html).toContain("https://relay.invalid/custom/messages/");
    expect(html).toContain("default-model");
    expect(html).toContain("正在读取影响范围");
    expect(buttonAttributes(html, "确认切换")).toContain('disabled=""');
    expect(buttonAttributes(html, "取消")).not.toContain('disabled=""');
    expect(c.activate).not.toHaveBeenCalled();
  });
});
