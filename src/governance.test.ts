import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import serviceSource from "./lib/governance-service.ts?raw";
import commandRegistry from "../src-tauri/src/lib.rs?raw";
import accessSource from "../src-tauri/src/governance/access.rs?raw";
import budgetSource from "../src-tauri/src/governance/budget.rs?raw";
import taskSource from "../src-tauri/src/governance/tasks.rs?raw";
import monitorSource from "../src-tauri/src/governance/monitor.rs?raw";
import versionsSource from "../src-tauri/src/governance/versions.rs?raw";
import mountsSource from "../src-tauri/src/governance/mounts.rs?raw";
import { governanceDiff, validInteger } from "./lib/governance-diff";
import * as service from "./lib/governance-service";
import type { VersionPreview } from "./types/governance";
import { DiffPreview, diffReviewRows } from "./features/governance/shared";
import { setLanguagePreference } from "./i18n/language";
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
    const review = diffReviewRows(diff);
    expect([review.removed, review.added]).toEqual([10000, 20000]);
    expect(review.rows.filter(row => row.kind === "fold")).toMatchObject([
      { side: "remove", count: 9800, beforeLine: 201, afterLine: undefined },
      { side: "add", count: 19800, beforeLine: undefined, afterLine: 201 },
    ]);
  });
  it("does not duplicate common prefixes and suffixes", () => {
    const diff=governanceDiff("1\n2\n3\n4\n5\nA\n6\n7\n8\n9", "1\n2\n3\n4\n5\nB\n6\n7\n8\n9",1);
    expect(diff.filter(l=>l.kind==="same").map(l=>l.text)).toEqual(["5","6"]);
    expect(diff.filter(l=>l.kind==="fold").map(l=>l.count)).toEqual([4,3]);
    const review = diffReviewRows(diff);
    expect([review.removed, review.added]).toEqual([1, 1]);
    expect(review.rows.map(row => [row.side, row.beforeLine, row.afterLine])).toEqual([
      ["same", 1, 1], ["same", 5, 5], ["remove", 6, undefined],
      ["add", undefined, 6], ["same", 7, 7], ["same", 8, 8],
    ]);
  });
  it.each([NaN,Infinity,-1,0,1.5,Number.MAX_SAFE_INTEGER+1])("rejects unsafe numeric controls: %s", n => expect(validInteger(n,1,100)).toBe(false));
  it("accepts the inclusive bounds", () => { expect(validInteger(1,1,100)).toBe(true); expect(validInteger(100,1,100)).toBe(true); });
});

describe("governance diff review presentation", () => {
  beforeEach(async () => { await setLanguagePreference("zh-CN"); });
  afterEach(async () => { await setLanguagePreference("system"); });

  it("keeps full content, file identity and line endings available without executing markup", () => {
    const html = renderToStaticMarkup(createElement(DiffPreview, {
      identity: "C:/profiles/<project>/AGENTS.md",
      before: "<script>private()</script>\r\n",
      after: "line\n",
    }));
    expect(html).toContain("C:/profiles/&lt;project&gt;/AGENTS.md");
    expect(html).toContain('role="region" aria-labelledby=');
    expect(html).toContain('class="gov-visually-hidden">移除</span>');
    expect(html).toContain('class="gov-visually-hidden">新增</span>');
    expect(html).toContain('class="gov-visually-hidden">空行</span>');
    expect(html).toContain('aria-label="回车符">CR</span>');
    expect(html).toContain("修改前: <code>CRLF</code>");
    expect(html).toContain("修改后: <code>LF</code>");
    expect(html.match(/aria-label="完整修改前内容">([\s\S]*?)<\/pre>/)?.[1]).toBe("&lt;script&gt;private()&lt;/script&gt;\r\n");
    expect(html.match(/aria-label="完整修改后内容">([\s\S]*?)<\/pre>/)?.[1]).toBe("line\n");
    expect(html).not.toContain("<script>");
  });

  it("renders folded ranges while retaining every changed line in the full content", () => {
    const before = Array.from({ length: 240 }, (_, index) => `old-${index}`).join("\n");
    const after = Array.from({ length: 260 }, (_, index) => `new-${index}`).join("\n");
    const html = renderToStaticMarkup(createElement(DiffPreview, { before, after }));
    expect(html).toContain("+260");
    expect(html).toContain("−240");
    expect(html).toContain("201–240");
    expect(html).toContain("201–260");
    expect(html).toContain("已折叠 40 行");
    expect(html).toContain("已折叠 60 行");
    expect(html.match(/aria-label="完整修改前内容">([\s\S]*?)<\/pre>/)?.[1]).toBe(before);
    expect(html.match(/aria-label="完整修改后内容">([\s\S]*?)<\/pre>/)?.[1]).toBe(after);
  });

  it("renders unchanged content and absent final newlines in English", async () => {
    await setLanguagePreference("en");
    const html = renderToStaticMarkup(createElement(DiffPreview, { before: "same", after: "same" }));
    expect(html).toContain("No final newline");
    expect(html).toContain("+0");
    expect(html).toContain("−0");
    expect(html).not.toMatch(/\p{Script=Han}/u);
  });
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
  it("keeps task limits and cancellation confirmation intact across native IPC", async () => {
    const task = { id: "task-one", attempts: 3, reservedMicrousd: 1, committedMicrousd: 2 };
    invoke.mockResolvedValueOnce([task]);
    expect(await service.listProxyTasks()).toEqual([task]);
    expect(invoke).toHaveBeenLastCalledWith("list_proxy_tasks");
    invoke.mockResolvedValueOnce({ task, attempts: [] });
    expect(await service.loadProxyTask(task.id)).toEqual({ task, attempts: [] });
    expect(invoke).toHaveBeenLastCalledWith("get_proxy_task", { id: task.id });
    await service.cancelProxyTask(task.id, false);
    expect(invoke).toHaveBeenLastCalledWith("cancel_proxy_task", { id: task.id, confirmed: false });
    await service.cancelProxyTask(task.id, true);
    expect(invoke).toHaveBeenLastCalledWith("cancel_proxy_task", { id: task.id, confirmed: true });
    const policy = { ...service.defaultBudgetPolicy, taskMaxAttempts: 0, taskMaxMicrousd: 1 };
    await service.saveHardBudget(policy, true);
    expect(invoke).toHaveBeenLastCalledWith("save_hard_budget", { policy, confirmed: true });
    invoke.mockRejectedValueOnce(new Error("task_store_unavailable"));
    await expect(service.loadProxyTask(task.id)).rejects.toThrow("task_store_unavailable");
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
    expect(await service.listProxyTasks()).toEqual([]);
    expect(await service.loadProxyTask("task-one")).toBeNull();
    const p=await service.loadAccessPolicy(); p.requireToken=true;
    expect((await service.loadAccessPolicy()).requireToken).toBe(false);
    const writes = [
      () => service.saveAccessPolicy(service.defaultAccessPolicy,true),
      () => service.issueAccessToken("Test","proxy",1),
      () => service.revokeAccessToken("id",true),
      () => service.saveHardBudget(service.defaultBudgetPolicy,true),
      () => service.cancelProxyTask("task-one", true),
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
    const sources: Record<string, string> = {access:accessSource,budget:budgetSource,tasks:taskSource,monitor:monitorSource,versions:versionsSource,mounts:mountsSource};
    for (const match of source.matchAll(/(?:read|write)<[^\n]*?>\("([a-z_]+)"/g)) expect(registry,match[1]).toContain("::"+match[1]+",");
    expect(registry).toContain("tasks::get_proxy_task,");
    for (const [file,names] of Object.entries({access:["save_access_policy","create_access_token","revoke_access_token"],budget:["save_hard_budget"],tasks:["cancel_proxy_task"],monitor:["save_synthetic_monitor","run_synthetic_check"],versions:["create_config_version","apply_config_rollback","delete_config_version"],mounts:["choose_mount_root","apply_explicit_mount","delete_mount_backup"]})) {
      const rust=sources[file].replaceAll("\r\n","\n");
      for (const name of names) expect(new RegExp("pub (?:async )?fn "+name+"\\([\\s\\S]*?\\)\\s*->[^\\n]+\\{\\n\\s*crate::platform::require_desktop").test(rust),name).toBe(true);
    }
  });
});
