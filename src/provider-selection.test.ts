import { beforeEach, describe, expect, it, vi } from "vitest";

const { invokeMock } = vi.hoisted(() => ({ invokeMock: vi.fn() }));

vi.mock("@tauri-apps/api/core", () => ({
  invoke: invokeMock,
}));

class MemoryStorage {
  private values = new Map<string, string>();

  getItem(key: string): string | null {
    return this.values.get(key) ?? null;
  }

  setItem(key: string, value: string): void {
    this.values.set(key, value);
  }

  removeItem(key: string): void {
    this.values.delete(key);
  }

  clear(): void {
    this.values.clear();
  }
}

const storage = new MemoryStorage();
Object.defineProperty(globalThis, "window", {
  configurable: true,
  value: {
    localStorage: storage,
    __TAURI_INTERNALS__: {},
  },
});
Object.defineProperty(globalThis, "localStorage", {
  configurable: true,
  value: storage,
});

import { loadSelectedProvider, saveSelectedProvider } from "./lib/provider-service";

const selectedProviderKey = "xy-switch:selected-provider";

describe("provider selection persistence", () => {
  beforeEach(() => {
    storage.clear();
    invokeMock.mockReset();
  });

  it("persists to the backend before updating local storage", async () => {
    let cachedDuringInvoke: string | null = "not-called";
    invokeMock.mockImplementation(async () => {
      cachedDuringInvoke = storage.getItem(selectedProviderKey);
    });

    await saveSelectedProvider("grok");

    expect(invokeMock).toHaveBeenCalledWith("set_current_provider", { id: "grok" });
    expect(cachedDuringInvoke).toBeNull();
    expect(loadSelectedProvider()).toBe("grok");
  });

  it("keeps the previous local selection when backend persistence fails", async () => {
    storage.setItem(selectedProviderKey, "openai");
    invokeMock.mockRejectedValue(new Error("database unavailable"));

    await expect(saveSelectedProvider("grok")).rejects.toThrow("database unavailable");

    expect(loadSelectedProvider()).toBe("openai");
  });

  it("removes the browser fallback selection when passed an empty id", async () => {
    storage.setItem(selectedProviderKey, "grok");

    await saveSelectedProvider("");

    expect(invokeMock).not.toHaveBeenCalled();
    expect(loadSelectedProvider()).toBe("anthropic");
  });
});
