import { afterEach, describe, expect, it, vi } from "vitest";
import { validSyncPassphrase } from "./lib/encrypted-sync-service";
const {invoke}=vi.hoisted(()=>({invoke:vi.fn()}));
vi.mock("@tauri-apps/api/core",()=>({invoke}));
afterEach(()=>{vi.unstubAllGlobals();vi.resetModules();invoke.mockReset();});
describe("encrypted sync services",()=>{
  it("uses character minimum and UTF-8 byte maximum",()=>{
    expect(validSyncPassphrase("short")).toBe(false); expect(validSyncPassphrase("a".repeat(12))).toBe(true); expect(validSyncPassphrase("测试口令至少十二个汉字字符")).toBe(true); expect(validSyncPassphrase("字".repeat(400))).toBe(false);
  });
  it("rejects browser mutation attempts before invoking native APIs",async()=>{
    vi.stubGlobal("window",{}); const service=await import("./lib/encrypted-sync-service"); await expect(service.previewSyncExport()).rejects.toThrow("desktop"); expect(invoke).not.toHaveBeenCalled();
  });
  it("migrates legacy favorites before export even if the session page was never opened",async()=>{
    const getItem=vi.fn(()=>'{"version":1,"keys":[]}'), setItem=vi.fn(); vi.stubGlobal("window",{__TAURI_INTERNALS__:{},localStorage:{getItem,setItem}});
    invoke.mockImplementation(async(command)=>command==="load_session_favorites"?[]:{ticketId:"ticket",expiresAt:123,content:{}});
    const service=await import("./lib/encrypted-sync-service"); await service.previewSyncExport(); expect(invoke.mock.calls.map(c=>c[0])).toEqual(["load_session_favorites","preview_sync_export"]);
    expect(invoke.mock.calls[0][1]).toEqual({legacy:'{"version":1,"keys":[]}',legacyAvailable:true}); expect(setItem).not.toHaveBeenCalled();
  });
  it("never substitutes absent legacy data when storage is unavailable",async()=>{
    vi.stubGlobal("window",{__TAURI_INTERNALS__:{},get localStorage(){throw new Error("denied");}}); invoke.mockRejectedValueOnce("Load and migrate local favorites before using encrypted sync");
    const service=await import("./lib/encrypted-sync-service"); await expect(service.previewSyncExport()).rejects.toThrow("favorites"); expect(invoke).toHaveBeenCalledWith("load_session_favorites",{legacy:null,legacyAvailable:false}); expect(invoke).toHaveBeenCalledTimes(1);
  });
  it("reports a failed post-import favorites refresh instead of silently keeping stale UI", async () => {
    vi.stubGlobal("window", { __TAURI_INTERNALS__: {}, localStorage: { getItem: () => null } });
    invoke.mockRejectedValueOnce("database unavailable");
    const service = await import("./lib/native-favorites-service");
    await expect(service.refreshNativeFavorites()).rejects.toThrow("favorites");
    expect(invoke).toHaveBeenCalledTimes(1);
  });
  it("passes ephemeral passphrases only to the explicit crypto command",async()=>{
    const setItem=vi.fn(); vi.stubGlobal("window",{__TAURI_INTERNALS__:{},localStorage:{getItem:()=>null,setItem}}); invoke.mockImplementation(async(command)=>command==="load_session_favorites"?[]:null);
    const service=await import("./lib/encrypted-sync-service"); await service.previewSyncImport("dummy test passphrase");
    expect(invoke).toHaveBeenLastCalledWith("preview_sync_import",{passphrase:"dummy test passphrase"}); expect(setItem).not.toHaveBeenCalled();
  });
});
