import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { SharedApprovalDetails, SharedMcpPanel } from "./features/mcp/SharedMcpPanel";
import { McpPage } from "./features/mcp/McpPage";
import { deleteMcpServer, saveMcpServer, setMcpAppEnabled } from "./lib/mcp-service";
import type { McpServer } from "./types/mcp";
import { SHARED_MCP_QUERY_KEYS, decideSharedMcp, loadSharedMcpPending, loadSharedMcpStatus, registerSharedMcp, revokeSharedMcp, setSharedMcpEnabled, sharedMcpErrorText } from "./lib/shared-mcp-service";
import { emptyPluginPermissions } from "./types/plugins";
import { sharedMcpConfigTarget, sharedMcpOwners, sharedMcpScopeNeedsReview, type SharedMcpConnection, type SharedMcpStatus, type SharedToolApproval } from "./types/shared-mcp";

const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
const clients: QueryClient[] = [];
const id = "01a1a209-a3e2-7750-9ed8-9c65741b2d48";

function approval(): SharedToolApproval {
  return { id, owner: "codex", clients: ["codex-desktop", "codex-cli"], arguments: { note: "[REDACTED_PERSONAL_1]", sample: "</textarea><script>alert(1)</script>" },
    privacy: { secretRedactions: 1, personalRedactions: 2, protectedFields: 0, uninspectedAttachments: 0 },
    preview: { previewId: id, expiresAt: Date.now() + 60000, pluginId: "example.echo", version: "1.0.0", pluginDigest: "fixture-digest", client: "codex-desktop", tool: "echo", argumentsDigest: "arguments-digest", requiredPermissions: emptyPluginPermissions(), effect: "Returns the supplied JSON." } };
}

function renderPanel(running = true, withApproval = true, connections?: SharedMcpConnection[]): string {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false, staleTime: Infinity, gcTime: Infinity } } });
  clients.push(client);
  const status: SharedMcpStatus = { enabled: running, running, address: "http://127.0.0.1:15722", versions: ["2026-07-28"], pendingCount: withApproval ? 1 : 0, lastError: null,
    connections: connections ?? [{ id, owner: "codex", clients: ["codex-desktop", "codex-cli"], serverId: `xy-shared-${id}`, active: false }] };
  client.setQueryData(SHARED_MCP_QUERY_KEYS.status, status);
  client.setQueryData(SHARED_MCP_QUERY_KEYS.pending, withApproval ? [approval()] : []);
  return renderToStaticMarkup(createElement(QueryClientProvider, { client }, createElement(SharedMcpPanel)));
}

beforeEach(() => { vi.resetAllMocks(); vi.stubGlobal("window", { __TAURI_INTERNALS__: {} }); });
afterEach(() => { for (const client of clients.splice(0)) client.clear(); vi.unstubAllGlobals(); });

describe("shared MCP native authorization", () => {
  it("keeps generated connections behind shared-service authorization before ordinary IPC", async () => {
    const shared: McpServer = { id: ` xy-shared-${id} `, name: "Shared test", description: "", transport: "stdio", command: "node", args: [], url: null, env: {}, headers: {}, apps: { claude: false, codex: true, gemini: false, opencode: false, "claude-desktop": false }, builtIn: false, secretsConfigured: false };
    for (const work of [() => saveMcpServer(shared), () => deleteMcpServer(shared.id), () => setMcpAppEnabled(shared.id, "codex", false)]) {
      await expect(work()).rejects.toThrow("共享插件服务");
    }
    expect(invoke).not.toHaveBeenCalled();
    const client = new QueryClient({ defaultOptions: { queries: { retry: false, staleTime: Infinity, gcTime: Infinity } } });
    clients.push(client);
    client.setQueryData(["mcp", "servers"], [shared]);
    client.setQueryData(["mcp", "apps"], [{ app: "codex", installed: true, label: "Codex", configPath: "synthetic-config", lastError: null }]);
    client.setQueryData(SHARED_MCP_QUERY_KEYS.status, { enabled: false, running: false, connections: [], versions: [], pendingCount: 0, lastError: null });
    const html = renderToStaticMarkup(createElement(QueryClientProvider, { client }, createElement(McpPage)));
    const record = html.match(/<article class="mcp-record"[\s\S]*?<\/article>/)?.[0] ?? "";
    expect(record).toContain("共享插件连接请在共享插件服务中管理。");
    const buttons = record.match(/<button[^>]*>/g) ?? [];
    expect(buttons).toHaveLength(7);
    expect(buttons.every(button => button.includes('disabled=""'))).toBe(true);
  });

  it("requires desktop runtime for all reads and writes", async () => {
    vi.stubGlobal("window", {});
    for (const work of [() => loadSharedMcpStatus(), () => loadSharedMcpPending(), () => setSharedMcpEnabled(true, true), () => registerSharedMcp("codex", true, true), () => revokeSharedMcp(id, true), () => decideSharedMcp(id, false, false)]) {
      await expect(work()).rejects.toThrow("桌面应用");
    }
    expect(invoke).not.toHaveBeenCalled();
    const html = renderPanel(false, false);
    expect(html).toContain("共享 MCP 操作需要桌面应用");
    expect(html).toMatch(/<button(?=[^>]*role="switch")(?=[^>]*disabled="")[^>]*>/);
    expect(html).not.toContain("共享 MCP 已启动");
  });

  it("rejects unconfirmed writes before IPC, while rejection remains available", async () => {
    for (const work of [() => setSharedMcpEnabled(true, false), () => setSharedMcpEnabled(false, false), () => registerSharedMcp("claude-desktop", false, false), () => revokeSharedMcp(id, false), () => decideSharedMcp(id, true, false)]) {
      await expect(work()).rejects.toThrow("请先确认");
    }
    expect(invoke).not.toHaveBeenCalled();
    await decideSharedMcp(id, false, false);
    expect(invoke).toHaveBeenCalledWith("shared_mcp_decide", { id, approved: false, confirmed: false });
  });

  it("requires separate authorization for shared desktop, CLI and 3P scopes", async () => {
    await expect(registerSharedMcp("codex", true, false)).rejects.toThrow("全部客户端授权范围");
    await expect(registerSharedMcp("opencode", true, false)).rejects.toThrow("全部客户端授权范围");
    await expect(registerSharedMcp("claude-desktop", true, false)).rejects.toThrow("全部客户端授权范围");
    await expect(registerSharedMcp("claude-desktop-3p", true, false)).rejects.toThrow("全部客户端授权范围");
    expect(invoke).not.toHaveBeenCalled();
    await registerSharedMcp("codex", true, true);
    await registerSharedMcp("claude-desktop", true, true);
    await registerSharedMcp("claude-desktop-3p", true, true);
    await registerSharedMcp("claude", true, false);
    expect(invoke.mock.calls).toEqual([
      ["shared_mcp_register", { owner: "codex", confirmed: true, sharedScopeConfirmed: true }],
      ["shared_mcp_register", { owner: "claude-desktop", confirmed: true, sharedScopeConfirmed: true }],
      ["shared_mcp_register", { owner: "claude-desktop-3p", confirmed: true, sharedScopeConfirmed: true }],
      ["shared_mcp_register", { owner: "claude", confirmed: true, sharedScopeConfirmed: false }],
    ]);
  });

  it("maps both desktop modes to the same config while retaining their explicit scopes", () => {
    for (const owner of ["claude-desktop", "claude-desktop-3p"] as const) {
      expect(sharedMcpConfigTarget(owner)).toBe("claude-desktop");
      expect(sharedMcpOwners.find(candidate => candidate.id === owner)?.clients).toEqual(["claude-desktop", "claude-desktop-3p"]);
    }
    expect(sharedMcpConfigTarget("claude")).toBe("claude");
  });

  it("explains inactive legacy scopes and retains their cleanup action", () => {
    const legacy: SharedMcpConnection = { id, owner: "claude-desktop", clients: ["claude-desktop"], serverId: `xy-shared-${id}`, active: false };
    expect(sharedMcpScopeNeedsReview(legacy)).toBe(true);
    expect(sharedMcpScopeNeedsReview({ ...legacy, clients: ["claude-desktop", "claude-desktop-3p"], active: true })).toBe(false);
    const html = renderPanel(true, false, [legacy]);
    expect(html).toContain("需重新授权");
    expect(html).toContain("旧连接已停用，请清理后重新确认两种模式的授权范围。");
    expect(html).toContain("重试清理");
    expect(html).toContain('<option value="claude-desktop-3p">Claude Desktop 3P</option>');
    expect(sharedMcpErrorText("shared_mcp_cleanup_required")).toContain("请先清理此配置的旧连接");
  });

  it("forwards only command arguments and never synthesizes native success", async () => {
    invoke.mockResolvedValueOnce({ running: true });
    expect(await loadSharedMcpStatus()).toEqual({ running: true });
    await loadSharedMcpPending();
    await setSharedMcpEnabled(false, true);
    await revokeSharedMcp(id, true);
    await decideSharedMcp(id, true, true);
    expect(invoke.mock.calls).toEqual([
      ["shared_mcp_status", undefined], ["shared_mcp_pending", undefined],
      ["shared_mcp_set_enabled", { enabled: false, confirmed: true }],
      ["shared_mcp_revoke", { id, confirmed: true }],
      ["shared_mcp_decide", { id, approved: true, confirmed: true }],
    ]);
  });

  it("removes arbitrary native error content and exposes safe recovery instructions", async () => {
    const privateError = "secret-token user@example.test C:\\private\\conversation.json";
    invoke.mockRejectedValue(privateError);
    await expect(loadSharedMcpStatus()).rejects.toThrow("刷新状态后重试");
    expect(sharedMcpErrorText(privateError)).not.toContain(privateError);
    expect(sharedMcpErrorText("shared_mcp_credentials_cleanup")).toContain("连接已停用");
    expect(sharedMcpErrorText("shared_mcp_config_cleanup")).toContain("重试撤销");
    expect(sharedMcpErrorText("shared_mcp_expired")).toContain("重新发起");
  });
});

describe("shared tool review and privacy", () => {
  it("reviews the full desktop mode scope for a 3P connection", () => {
    const review: SharedToolApproval = { ...approval(), owner: "claude-desktop-3p", clients: ["claude-desktop", "claude-desktop-3p"] };
    const html = renderToStaticMarkup(createElement(SharedApprovalDetails, { approval: review }));
    expect(html).toContain("客户端授权范围");
    expect(html).toContain("Claude Desktop / Claude Desktop 3P");
    expect(html).not.toContain("fixture-digest");
  });

  it("shows the complete client scope, redacted arguments, permissions and counts", () => {
    const html = renderToStaticMarkup(createElement(SharedApprovalDetails, { approval: approval() }));
    for (const expected of ["Codex Desktop / Codex CLI", "example.echo", "脱敏后的调用参数", "[REDACTED_PERSONAL_1]", "本次所需权限", "凭据替换数", "个人信息替换数"]) expect(html).toContain(expected);
    expect(html).not.toContain("<script>");
    expect(html).toContain("&lt;/textarea&gt;&lt;script&gt;");
    expect(html).not.toContain("fixture-digest");
    expect(html).not.toContain("arguments-digest");
  });

  it("offers review and cleanup without exposing arguments or connection tokens in the list", () => {
    const html = renderPanel();
    expect(html).toContain("审阅调用");
    expect(html).toContain("重试清理");
    expect(html).toContain("待清理");
    expect(html).not.toContain("[REDACTED_PERSONAL_1]");
    expect(html).not.toContain("arguments-digest");
    expect(html).not.toContain("http://127.0.0.1:15722");
  });

  it("does not show cached approvals after the shared service has stopped", () => {
    const html = renderPanel(false);
    expect(html).not.toContain("example.echo");
    expect(html).not.toContain("审阅调用</button>");
  });
});
