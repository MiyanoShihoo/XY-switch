import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import serviceSource from "./lib/governance-service.ts?raw";
import commandRegistry from "../src-tauri/src/lib.rs?raw";
import accessSource from "../src-tauri/src/governance/access.rs?raw";
import budgetSource from "../src-tauri/src/governance/budget.rs?raw";
import monitorSource from "../src-tauri/src/governance/monitor.rs?raw";
import versionsSource from "../src-tauri/src/governance/versions.rs?raw";
import mountsSource from "../src-tauri/src/governance/mounts.rs?raw";
import { governanceDiff, validInteger } from "./lib/governance-diff";
import * as service from "./lib/governance-service";
import type { VersionPreview } from "./types/governance";
const invoke = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke }));

describe("governance diff preserves meaningful changes", () => {
  it("has no changes for identical text", () => expect(governanceDiff("a\r\nb\n", "a\r\nb\n")).toEqual([]));
  it("shows add/remove with surrounding context", () => {
    expect(governanceDiff("a\nb\nc", "a\nB\nc")).toEqual([
      { kind:"same", text:"a" }, { kind:"remove", text:"b" }, { kind:"add", text:"B" }, { kind:"same", text:"c" },
    ]);
  });
  it("does not hide line-ending or final newline changes", () => {
    expect(governanceDiff("line\r\n", "line\n").some(l => l.kind === "remove" && l.text === "line\r")).toBe(true);
    expect(governanceDiff("line", "line\n").some(l => l.kind === "add")).toBe(true);
  });
  it("bounds huge diffs with explicit fold counts instead of silent truncation", () => {
    const old = Array.from({length:10000}, (_,i) => "old"+i).join("\n");
    const next = Array.from({length:20000}, (_,i) => "new"+i).join("\n");
    const diff = governanceDiff(old,next);
    expect(diff).toHaveLength(402);
    expect(diff.filter(l => l.kind === "fold").map(l => l.count)).toEqual([9800,19800]);
  });
  it("does not duplicate common prefixes and suffixes", () => {
    const diff=governanceDiff("1\n2\n3\n4\n5\nA\n6\n7\n8\n9", "1\n2\n3\n4\n5\nB\n6\n7\n8\n9",1);
    expect(diff.filter(l=>l.kind==="same").map(l=>l.text)).toEqual(["5","6"]);
    expect(diff.filter(l=>l.kind==="fold").map(l=>l.count)).toEqual([4,3]);
  });
  it.each([NaN,Infinity,-1,0,1.5,Number.MAX_SAFE_INTEGER+1])("rejects unsafe numeric controls: %s", n => expect(validInteger(n,1,100)).toBe(false));
  it("accepts the inclusive bounds", () => { expect(validInteger(1,1,100)).toBe(true); expect(validInteger(100,1,100)).toBe(true); });
});

describe("governance IPC never simulates a privileged success", () => {
  beforeEach(() => { vi.resetAllMocks(); vi.stubGlobal("window", {__TAURI_INTERNALS__:{}}); });
  afterEach(() => vi.unstubAllGlobals());
  const preview: VersionPreview = {versionId:"v1",expectedCurrentHash:"current",targetHash:"target",before:"old",after:"new"};
  it("round-trips native reads and surfaces errors", async () => {
    invoke.mockResolvedValueOnce({requireToken:true,requestsPerMinute:10,maxInFlight:2});
    expect((await service.loadAccessPolicy()).requireToken).toBe(true);
    expect(invoke).toHaveBeenCalledWith("get_access_policy");
    invoke.mockRejectedValueOnce(new Error("unavailable"));
    await expect(service.loadHardBudget()).rejects.toThrow("unavailable");
  });
  it("passes the exact preview hashes and explicit confirmation to rollback", async () => {
    await service.applyConfigRollback(preview,"ROLLBACK");
    expect(invoke).toHaveBeenLastCalledWith("apply_config_rollback",{id:"v1",expectedCurrentHash:"current",targetHash:"target",confirmation:"ROLLBACK"});
  });
  it("passes scoped root handles instead of arbitrary filesystem paths", async () => {
    await service.previewExplicitMount("native-handle","codex",["prompt"],["skill"]);
    expect(invoke).toHaveBeenLastCalledWith("preview_explicit_mount",{rootId:"native-handle",client:"codex",promptIds:["prompt"],skillIds:["skill"]});
    await service.applyExplicitMount("preview-handle","WRITE FILE");
    expect(invoke).toHaveBeenLastCalledWith("apply_explicit_mount",{id:"preview-handle",confirmation:"WRITE FILE"});
    await service.previewMountRollback("native-handle","backup");
    expect(invoke).toHaveBeenLastCalledWith("preview_mount_rollback",{rootId:"native-handle",backupId:"backup"});
  });
  it("does not generate or persist a token in the renderer", async () => {
    const issued={token:"one-time-unit-test",info:{id:"one"}};
    invoke.mockResolvedValueOnce(issued);
    expect(await service.issueAccessToken("Monitor","monitor",30)).toBe(issued);
    expect(invoke).toHaveBeenCalledExactlyOnceWith("create_access_token",{label:"Monitor",scope:"monitor",days:30});
  });
  it("separates preview reads from writes without fake data", async () => {
    vi.stubGlobal("window",{});
    expect(await service.loadGovernanceOverview()).toBeNull();
    expect(await service.listSyntheticRuns()).toEqual([]);
    const p=await service.loadAccessPolicy(); p.requireToken=true;
    expect((await service.loadAccessPolicy()).requireToken).toBe(false);
    const writes = [
      () => service.saveAccessPolicy(service.defaultAccessPolicy,true),
      () => service.issueAccessToken("Test","proxy",1),
      () => service.revokeAccessToken("id",true),
      () => service.saveHardBudget(service.defaultBudgetPolicy,true),
      () => service.saveMonitorPolicy(service.defaultMonitorPolicy,"ALLOW PAID CHECKS"),
      () => service.runSyntheticCheck(service.defaultMonitorPolicy,"ALLOW PAID CHECKS"),
      () => service.createConfigVersion("Test"), () => service.previewConfigRollback("id"),
      () => service.applyConfigRollback(preview,"ROLLBACK"), () => service.deleteConfigVersion("id","DELETE VERSION"),
      () => service.chooseMountRoot(), () => service.previewExplicitMount("r","codex",[],[]),
      () => service.previewMountRollback("r","b"), () => service.applyExplicitMount("id","WRITE FILE"),
      () => service.deleteMountBackup("id","DELETE BACKUP"),
    ];
    for (const write of writes) await expect(write()).rejects.toThrow();
    expect(invoke).not.toHaveBeenCalled();
  });
  it("registers all governance commands and blocks mobile mutation entrypoints", () => {
    const source=serviceSource;
    const registry=commandRegistry;
    const sources: Record<string, string> = {access:accessSource,budget:budgetSource,monitor:monitorSource,versions:versionsSource,mounts:mountsSource};
    for (const match of source.matchAll(/(?:read|write)<[^\n]*?>\("([a-z_]+)"/g)) expect(registry,match[1]).toContain("::"+match[1]+",");
    for (const [file,names] of Object.entries({access:["save_access_policy","create_access_token","revoke_access_token"],budget:["save_hard_budget"],monitor:["save_synthetic_monitor","run_synthetic_check"],versions:["create_config_version","apply_config_rollback","delete_config_version"],mounts:["choose_mount_root","apply_explicit_mount","delete_mount_backup"]})) {
      const rust=sources[file].replaceAll("\r\n","\n");
      for (const name of names) expect(new RegExp("pub (?:async )?fn "+name+"\\([\\s\\S]*?\\)\\s*->[^\\n]+\\{\\n\\s*crate::platform::require_desktop").test(rust),name).toBe(true);
    }
  });
});
