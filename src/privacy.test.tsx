import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const invoke = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
import { defaultPrivacySettings, loadPrivacySettings, maxPrivacyPreviewBytes, previewPrivacy, savePrivacySettings } from "./lib/privacy-service";
import { PrivacySettings } from "./features/settings/PrivacySettings";

describe("native privacy policies and local inspection", () => {
  beforeEach(() => { vi.resetAllMocks(); vi.stubGlobal("window", { __TAURI_INTERNALS__: {} }); });
  afterEach(() => { vi.unstubAllGlobals(); });

  it("returns persisted policies and does not simulate a successful save", async () => {
    const actual = { ...defaultPrivacySettings, redactPersonalInformation: false };
    invoke.mockResolvedValue(actual);
    expect(await loadPrivacySettings()).toEqual(actual);
    expect(await savePrivacySettings(defaultPrivacySettings)).toEqual(actual);
    expect(invoke).toHaveBeenLastCalledWith("save_privacy_settings", { settings: defaultPrivacySettings });
    invoke.mockRejectedValue(new Error("settings unavailable"));
    await expect(loadPrivacySettings()).rejects.toThrow("settings unavailable");
    await expect(savePrivacySettings(defaultPrivacySettings)).rejects.toThrow("settings unavailable");
  });

  it("cannot scan or save in browser preview and explains this in the production panel", async () => {
    vi.stubGlobal("window", {});
    await expect(loadPrivacySettings()).rejects.toThrow("桌面应用");
    await expect(savePrivacySettings(defaultPrivacySettings)).rejects.toThrow("桌面应用");
    await expect(previewPrivacy("sample", defaultPrivacySettings)).rejects.toThrow("桌面应用");
    const client = new QueryClient();
    const html = renderToStaticMarkup(createElement(QueryClientProvider, { client }, createElement(PrivacySettings, { desktop: false })));
    expect(html).toContain("保存和本地检查需要桌面应用");
    expect(html).toContain("role=\"switch\"");
    expect(html).toContain("disabled=\"\"");
    expect(html).not.toContain("检查完成");
    expect(invoke).not.toHaveBeenCalled();
    client.clear();
  });

  it("runs preview through the native command with draft settings and no durable content storage", async () => {
    const storage = { getItem: vi.fn(), setItem: vi.fn(), removeItem: vi.fn() };
    vi.stubGlobal("localStorage", storage);
    const inspected = { redacted: "[REDACTED:EMAIL]", blockedCode: null, report: { secretRedactions: 0, personalRedactions: 1, protectedFields: 0, uninspectedAttachments: 0 } };
    invoke.mockResolvedValue(inspected);
    const policy = { ...defaultPrivacySettings, blockUninspectedAttachments: false };
    expect(await previewPrivacy("person@example.test", policy)).toEqual(inspected);
    expect(invoke).toHaveBeenCalledExactlyOnceWith("preview_privacy", { text: "person@example.test", settings: policy });
    expect(storage.setItem).not.toHaveBeenCalled();
    expect(storage.getItem).not.toHaveBeenCalled();
    expect(storage.removeItem).not.toHaveBeenCalled();
  });

  it("uses UTF-8 byte limits and rejects empty or oversized input before IPC", async () => {
    await expect(previewPrivacy(" \n ", defaultPrivacySettings)).rejects.toThrow("文本或 JSON");
    await expect(previewPrivacy("x".repeat(maxPrivacyPreviewBytes + 1), defaultPrivacySettings)).rejects.toThrow("256 KiB");
    await expect(previewPrivacy("🦆".repeat(maxPrivacyPreviewBytes / 4 + 1), defaultPrivacySettings)).rejects.toThrow("256 KiB");
    expect(invoke).not.toHaveBeenCalled();
    invoke.mockResolvedValue({ redacted: "safe", blockedCode: null });
    await previewPrivacy("x".repeat(maxPrivacyPreviewBytes), defaultPrivacySettings);
    expect(invoke).toHaveBeenCalledOnce();
  });
});
