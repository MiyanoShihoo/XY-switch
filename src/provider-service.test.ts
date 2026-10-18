import { beforeEach, describe, expect, it, vi } from "vitest";
import { normalizeProvider, providers } from "./lib/providers";
import { loadProviders, refreshCodexProvider, saveProvider, saveProviderOrder } from "./lib/provider-service";
const { invokeMock } = vi.hoisted(() => ({ invokeMock: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: invokeMock }));
vi.stubGlobal("window", { __TAURI_INTERNALS__: {} });

describe("provider IPC contract", () => {
  beforeEach(() => invokeMock.mockReset());
  it.each(["grok", "kimi", "openai"])("saves %s with only canonical backend fields", async (id) => {
    const source = providers.find((p) => p.id === id)!;
    // Reproduce a backend record normalized then edited/duplicated in the UI.
    const provider = normalizeProvider({ ...source, appGroup: "Claude", type: "stale" });
    provider.group = "Codex";
    await saveProvider(provider);
    const payload = invokeMock.mock.calls[0][1].provider;
    expect(payload.appGroup).toBe("Codex");
    expect(Object.keys(payload).sort()).toEqual([
      "id", "name", "providerType", "endpoint", "model", "appGroup", "enabled", "sortOrder", "advanced",
    ].sort());
  });
  it("uses the Tauri camelCase argument for reorder_providers", async () => {
    await saveProviderOrder(["kimi", "grok"]);
    expect(invokeMock).toHaveBeenCalledWith("reorder_providers", { providerIds: ["kimi", "grok"] });
  });
  it("normalizes backend records before returning them", async () => {
    invokeMock.mockResolvedValue([{ id: "kimi", name: "Kimi", appGroup: "Codex", endpoint: "https://api.moonshot.cn/v1", model: "test", enabled: true }]);
    const result = await loadProviders();
    expect(result[0].group).toBe("Codex");
    expect(result[0].advanced.upstreamFormat).toBe("openai_responses");
  });
  it("refreshes already managed clients without changing the global selection", async () => {
    await refreshCodexProvider("kimi");
    expect(invokeMock).toHaveBeenCalledWith("refresh_codex_provider", { id: "kimi" });
  });
});
