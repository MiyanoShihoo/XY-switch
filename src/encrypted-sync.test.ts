import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { validSyncPassphrase } from "./lib/encrypted-sync-service";
import { SyncImportReview } from "./features/settings/SyncImportReview";
import { setLanguagePreference } from "./i18n/language";
import type { SyncChange, SyncChoice } from "./types/encrypted-sync";
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
  it("keeps ticket identities, per-item choices and exact confirmation words at the native boundary", async () => {
    const setItem = vi.fn();
    vi.stubGlobal("window", { __TAURI_INTERNALS__: {}, localStorage: { getItem: () => null, setItem } });
    const service = await import("./lib/encrypted-sync-service");
    const choices: SyncChoice[] = [{ key: "provider:keep", useIncoming: false }, { key: "price:accept", useIncoming: true }];
    await service.applySyncImport("import-ticket", choices, "IMPORT");
    await service.saveSyncExport("export-ticket", "dummy test passphrase", "EXPORT");
    await service.applySyncRollback("rollback-ticket", "ROLLBACK SYNC");
    await service.cancelSyncPreview("expired-ticket");
    expect(invoke.mock.calls).toEqual([
      ["apply_sync_import", { ticketId: "import-ticket", choices, confirmation: "IMPORT" }],
      ["save_sync_export", { ticketId: "export-ticket", passphrase: "dummy test passphrase", confirmation: "EXPORT" }],
      ["apply_sync_rollback", { ticketId: "rollback-ticket", confirmation: "ROLLBACK SYNC" }],
      ["cancel_sync_preview", { ticketId: "expired-ticket" }],
    ]);
    expect(setItem).not.toHaveBeenCalled();
  });
});

describe("encrypted sync item review", () => {
  beforeEach(async () => { await setLanguagePreference("zh-CN"); });
  afterEach(async () => { await setLanguagePreference("system"); });
  const change: SyncChange = {
    key: "provider:" + "complete-target-identity-".repeat(8),
    kind: "provider", status: "conflict", effect: "keep_old_target_create_disabled_copy",
    local: { name: "Local", endpoint: "https://local.example/v1" },
    incoming: { name: "<script>incoming()</script>", endpoint: "https://incoming.example/v1" },
  };

  it.each([
    { label: "undecided", choice: undefined, message: "请先选择此对象的处理方式。" },
    { label: "keep local", choice: false, message: "已选择保留本地 / 跳过，此对象不会写入。" },
    { label: "accept incoming", choice: true, message: "已选择接受传入项，最终确认后才会合并。" },
  ])("describes the $label decision without presenting incoming content as an applied result", ({ choice, message }) => {
    const onChange = vi.fn();
    const html = renderToStaticMarkup(createElement(SyncImportReview, {
      changes: [change], choices: { [change.key]: choice }, disabled: false, onChange,
    }));
    expect(html).toContain(message);
    expect(html).toContain("接受传入项后的影响");
    expect(html).toContain("目标地址或协议已变化：保留旧供应商及凭据，另建禁用副本。");
    expect(html).toContain("完整内容 · 本地内容");
    expect(html).toContain("完整内容 · 传入内容");
    expect(html).toContain(change.key);
    expect(html).toContain("&lt;script&gt;incoming()&lt;/script&gt;");
    expect(html).not.toContain("<script>");
    expect(html).not.toContain("修改后");
    expect(onChange).not.toHaveBeenCalled();
  });

  it("keeps unchanged items read-only even if a choice is present", () => {
    const html = renderToStaticMarkup(createElement(SyncImportReview, {
      changes: [{ ...change, status: "unchanged", incoming: change.local }],
      choices: { [change.key]: true }, disabled: false, onChange: vi.fn(),
    }));
    expect(html).toContain("此对象与本地一致，不会写入。");
    expect(html).not.toContain('aria-label="处理方式');
    expect(html).not.toContain("最终确认后才会合并");
    expect(html).not.toContain("接受传入项后的影响");
  });

  it("renders conflict consequences, choices and both content sides in English", async () => {
    await setLanguagePreference("en");
    const html = renderToStaticMarkup(createElement(SyncImportReview, {
      changes: [change], choices: { [change.key]: true }, disabled: true, onChange: vi.fn(),
    }));
    expect(html).toContain("Effect of accepting this item");
    expect(html).toContain("Full content · Local content");
    expect(html).toContain("Full content · Incoming content");
    expect(html).toContain("only after final confirmation");
    expect(html).not.toMatch(/\p{Script=Han}/u);
  });
});
