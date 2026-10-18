import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { browserCapabilities, loadPlatformCapabilities } from "./lib/platform-service";
const invoke = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
beforeEach(() => { invoke.mockReset(); });
afterEach(() => vi.unstubAllGlobals());

describe("platform capabilities", () => {
  it("does not pretend a renderer preview has native capabilities", async () => {
    vi.stubGlobal("window", {});
    expect(await loadPlatformCapabilities()).toEqual(browserCapabilities);
    expect(browserCapabilities.clientTakeover).toBe(false);
    expect(browserCapabilities.systemTray).toBe(false);
    expect(browserCapabilities.autostart).toBe(false);
    expect(invoke).not.toHaveBeenCalled();
  });
  it("is safe during server rendering", async () => {
    vi.stubGlobal("window", undefined);
    expect(await loadPlatformCapabilities()).toEqual(browserCapabilities);
    expect(invoke).not.toHaveBeenCalled();
  });
  it.each([
    { os: "windows", mobile: false, systemTray: true, autostart: true, clientTakeover: true, credentialStore: "Windows Credential Manager" },
    { os: "linux", mobile: false, systemTray: false, autostart: false, clientTakeover: true, credentialStore: "Secret Service" },
    { os: "ios", mobile: true, systemTray: false, autostart: false, clientTakeover: false, credentialStore: "Apple Keychain" },
  ])("uses the native capabilities for $os, not a user-agent guess", async value => {
    vi.stubGlobal("window", { __TAURI_INTERNALS__: {} });
    invoke.mockResolvedValue(value);
    expect(await loadPlatformCapabilities()).toEqual(value);
    expect(invoke).toHaveBeenCalledExactlyOnceWith("app_platform");
  });
  it("propagates a failed IPC lookup instead of enabling unsupported controls", async () => {
    vi.stubGlobal("window", { __TAURI_INTERNALS__: {} });
    invoke.mockRejectedValue(new Error("native unavailable"));
    await expect(loadPlatformCapabilities()).rejects.toThrow("native unavailable");
  });
});
