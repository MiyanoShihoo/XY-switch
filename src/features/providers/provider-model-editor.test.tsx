import { renderToStaticMarkup as render } from "react-dom/server";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createProviderAdvanced } from "../../lib/providers";
import type { Provider } from "../../types/provider";
import { ProviderModelEditor, synchronizeProviderModel, updateProviderDefaultModel } from "./ProviderModelEditor";

const { invokeMock } = vi.hoisted(() => ({ invokeMock: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: invokeMock }));

function provider(change: Partial<Provider> = {}): Provider {
  return {
    id: "relay", name: "Relay", group: "Claude", providerType: "custom", sortOrder: 17,
    endpoint: "https://relay.invalid/custom/messages/", model: "original-model", enabled: true, icon: "/app-icon.png",
    advanced: {
      ...createProviderAdvanced("anthropic"), isFullUrl: true, customUserAgent: "Relay/1.0",
      modelMappings: [{ model: "mapped-model", displayName: "Mapped alias", contextWindow: 131072 }],
      headerOverrides: { "x-provider": "relay" }, bodyOverrides: { temperature: 0.2, metadata: { route: "research" } },
      codex: { remoteCompression: true, applyCommonConfig: true, context1m: true, compactionThreshold: 850000, configToml: 'model_reasoning_effort = "high"' },
      claude: { disableArtifactTool: true },
    }, ...change,
  };
}

function buttonAttributes(html: string, label: string): string {
  const button = [...html.matchAll(/<button\b([^>]*)>([\s\S]*?)<\/button>/g)]
    .find(match => match[2].replace(/<[^>]*>/g, "").trim() === label);
  expect(button, label).toBeDefined();
  return button![1];
}

beforeEach(() => {
  invokeMock.mockReset().mockResolvedValue(undefined);
  vi.stubGlobal("window", { __TAURI_INTERNALS__: {} });
});
afterEach(() => vi.unstubAllGlobals());

describe("direct default-model persistence", () => {
  it("changes only the trimmed model and sends canonical provider fields without credentials", async () => {
    const source = { ...provider(), apiKey: "DO_NOT_SUBMIT_API_KEY", api_key: "DO_NOT_SUBMIT_API_KEY" };
    const original = structuredClone(source);
    const events: string[] = [];
    invokeMock.mockImplementation(async (command: string) => { events.push(command); });
    const persisted = vi.fn(() => { events.push("persisted"); });
    const runtimeChanged = vi.fn(async () => { events.push("runtime"); });

    const next = await updateProviderDefaultModel(source, "  replacement-model  ", persisted, runtimeChanged);

    expect(invokeMock.mock.calls[0]).toEqual(["save_provider", { provider: {
      id: source.id, name: source.name, endpoint: source.endpoint, model: "replacement-model", enabled: source.enabled,
      advanced: source.advanced, appGroup: source.group, providerType: source.providerType, sortOrder: source.sortOrder,
    } }]);
    expect(JSON.stringify(invokeMock.mock.calls)).not.toContain("DO_NOT_SUBMIT_API_KEY");
    expect(events).toEqual(["save_provider", "persisted", "refresh_codex_provider", "runtime"]);
    expect(invokeMock.mock.calls[1]).toEqual(["refresh_codex_provider", { id: source.id }]);
    expect(next).toEqual({ ...original, model: "replacement-model" });
    expect(source).toEqual(original);
    expect(persisted).toHaveBeenCalledWith(next);
  });

  it("does not publish or synchronize a failed save", async () => {
    invokeMock.mockRejectedValueOnce(new Error("storage unavailable"));
    const persisted = vi.fn(), runtimeChanged = vi.fn(async () => {});
    await expect(updateProviderDefaultModel(provider(), "new-model", persisted, runtimeChanged)).rejects.toThrow("storage unavailable");
    expect(invokeMock.mock.calls.map(call => call[0])).toEqual(["save_provider"]);
    expect(persisted).not.toHaveBeenCalled();
    expect(runtimeChanged).not.toHaveBeenCalled();
  });

  it("retains a completed save when synchronization fails and retries without resaving", async () => {
    invokeMock.mockImplementation(async (command: string) => {
      if (command === "refresh_codex_provider") throw new Error("client config is locked");
    });
    const persisted = vi.fn(), runtimeChanged = vi.fn(async () => {});
    await expect(updateProviderDefaultModel(provider(), "new-model", persisted, runtimeChanged)).rejects.toThrow("client config is locked");
    expect(persisted).toHaveBeenCalledExactlyOnceWith(expect.objectContaining({ model: "new-model" }));
    expect(runtimeChanged).not.toHaveBeenCalled();
    const saved = persisted.mock.calls[0][0] as Provider;

    invokeMock.mockResolvedValue(undefined);
    await synchronizeProviderModel(saved, runtimeChanged);
    expect(invokeMock.mock.calls.map(call => call[0])).toEqual(["save_provider", "refresh_codex_provider", "refresh_codex_provider"]);
    expect(persisted).toHaveBeenCalledTimes(1);
    expect(runtimeChanged).toHaveBeenCalledTimes(1);
  });

  it("reports a runtime-refresh failure after publishing the saved model", async () => {
    const persisted = vi.fn();
    const runtimeChanged = vi.fn(async () => { throw new Error("runtime read failed"); });
    await expect(updateProviderDefaultModel(provider(), "new-model", persisted, runtimeChanged)).rejects.toThrow("runtime read failed");
    expect(persisted).toHaveBeenCalledExactlyOnceWith(expect.objectContaining({ model: "new-model" }));
    expect(invokeMock.mock.calls.map(call => call[0])).toEqual(["save_provider", "refresh_codex_provider"]);
  });

  it("saves disabled providers without synchronizing managed clients", async () => {
    const persisted = vi.fn(), runtimeChanged = vi.fn(async () => {});
    await updateProviderDefaultModel(provider({ enabled: false }), "new-model", persisted, runtimeChanged);
    expect(invokeMock.mock.calls.map(call => call[0])).toEqual(["save_provider"]);
    expect(persisted).toHaveBeenCalledExactlyOnceWith(expect.objectContaining({ enabled: false, model: "new-model" }));
    expect(runtimeChanged).toHaveBeenCalledTimes(1);
  });

  it.each(["", " \t\n "])("rejects a blank model before any IPC call: %j", async model => {
    const persisted = vi.fn(), runtimeChanged = vi.fn(async () => {});
    await expect(updateProviderDefaultModel(provider(), model, persisted, runtimeChanged)).rejects.toThrow("请填写默认模型 ID");
    expect(invokeMock).not.toHaveBeenCalled();
    expect(persisted).not.toHaveBeenCalled();
    expect(runtimeChanged).not.toHaveBeenCalled();
  });

  it("keeps browser preview saves explicitly unsupported", async () => {
    vi.stubGlobal("window", {});
    const persisted = vi.fn(), runtimeChanged = vi.fn(async () => {});
    await expect(updateProviderDefaultModel(provider(), "new-model", persisted, runtimeChanged)).rejects.toThrow("浏览器预览模式不能修改供应商");
    expect(invokeMock).not.toHaveBeenCalled();
    expect(persisted).not.toHaveBeenCalled();
    expect(runtimeChanged).not.toHaveBeenCalled();
  });
});

describe("default-model dialog", () => {
  const callbacks = () => ({ onSaved: vi.fn(), onRuntimeChanged: vi.fn(async () => {}), onClose: vi.fn() });

  it("offers current and mapped models without rendering or reading credentials", () => {
    const html = render(<ProviderModelEditor provider={provider()} current={false} {...callbacks()} />);
    expect(html).toContain('value="original-model"');
    expect(html).toContain("Mapped alias · mapped-model");
    expect(html).toContain("仅保存此配置");
    expect(html).not.toContain('type="password"');
    expect(html).not.toContain("model_reasoning_effort");
    expect(buttonAttributes(html, "保存模型")).toContain('disabled=""');
    expect(buttonAttributes(html, "探索可用模型")).not.toContain('disabled=""');
    expect(invokeMock).not.toHaveBeenCalled();
  });

  it("waits for the actual shared-gateway scope before allowing a current-model save", () => {
    const html = render(<ProviderModelEditor provider={provider()} current {...callbacks()} />);
    expect(html).toContain("正在读取影响范围");
    expect(html).toContain("不会开启新的接管");
    expect(buttonAttributes(html, "保存模型")).toContain('disabled=""');
    expect(buttonAttributes(html, "取消")).not.toContain('disabled=""');
  });

  it("blocks model changes and discovery on a read failure while allowing cancellation", () => {
    const html = render(<ProviderModelEditor provider={provider()} current={false} readError="Configuration read failed" {...callbacks()} />);
    expect(html).toContain("Configuration read failed");
    expect(buttonAttributes(html, "保存模型")).toContain('disabled=""');
    expect(buttonAttributes(html, "探索可用模型")).toContain('disabled=""');
    expect(buttonAttributes(html, "取消")).not.toContain('disabled=""');
  });

  it("locks save, discovery and close controls during an operation", () => {
    const html = render(<ProviderModelEditor provider={provider()} current={false} disabled {...callbacks()} />);
    for (const label of ["保存模型", "探索可用模型", "取消"]) expect(buttonAttributes(html, label)).toContain('disabled=""');
    expect(html).toMatch(/aria-label="关闭弹窗"[^>]*disabled=""/);
  });
});
