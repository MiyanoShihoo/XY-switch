import { beforeEach, describe, expect, it, vi } from "vitest";
const invoke = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
import { defaultPreferences, loadAutostart, loadPreferences, saveAutostart, savePreferences, validatePreferences } from "./lib/preferences-service";

describe("real application preferences", () => {
  beforeEach(() => { vi.resetAllMocks(); vi.stubGlobal("window", { __TAURI_INTERNALS__: {} }); });
  it.each([15, 180, 600])("accepts integer timeout %s", requestTimeoutSecs => {
    expect(validatePreferences({ ...defaultPreferences, requestTimeoutSecs })).toBe("");
  });
  it.each([0, 14, 601, NaN, Infinity, 20.5])("rejects invalid timeout %s", requestTimeoutSecs => {
    expect(validatePreferences({ ...defaultPreferences, requestTimeoutSecs })).not.toBe("");
  });
  it("uses the backend preferences and returns its saved result", async () => {
    const persisted = { ...defaultPreferences, requestTimeoutSecs: 60, minimizeToTray: false };
    invoke.mockResolvedValue(persisted);
    expect(await loadPreferences()).toEqual(persisted);
    expect(await savePreferences(persisted)).toEqual(persisted);
    expect(invoke).toHaveBeenLastCalledWith("save_app_preferences", { preferences: persisted });
  });
  it("returns actual autostart state instead of echoing requested state", async () => {
    invoke.mockResolvedValue(false);
    expect(await saveAutostart(true)).toBe(false);
    expect(invoke).toHaveBeenCalledWith("set_autostart_enabled", { enabled: true });
  });
  it("does not turn an autostart read error into a false/off state", async () => {
    invoke.mockRejectedValue(new Error("registry unavailable"));
    await expect(loadAutostart()).rejects.toThrow("registry unavailable");
  });
  it("preview is read-only and does not invoke IPC", async () => {
    vi.stubGlobal("window", {});
    expect(await loadPreferences()).toEqual(defaultPreferences);
    await expect(savePreferences(defaultPreferences)).rejects.toThrow("浏览器预览");
    await expect(saveAutostart(true)).rejects.toThrow("浏览器预览");
    expect(invoke).not.toHaveBeenCalled();
  });
});
