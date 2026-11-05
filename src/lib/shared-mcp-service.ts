import { invoke } from "@tauri-apps/api/core";
import { t } from "../i18n/language";
import { sharedMcpOwners, type SharedMcpConnection, type SharedMcpOwner, type SharedMcpStatus, type SharedToolApproval } from "../types/shared-mcp";

export const SHARED_MCP_QUERY_KEYS = { status: ["shared-mcp", "status"] as const, pending: ["shared-mcp", "pending"] as const };

export function isSharedMcpDesktopRuntime(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

export function sharedMcpErrorText(cause: unknown): string {
  const code = typeof cause === "string" ? cause : cause instanceof Error ? cause.message : "";
  const messages: Record<string, string> = {
    shared_mcp_confirmation: t("请先确认本次共享 MCP 操作。"),
    shared_mcp_scope: t("客户端授权范围无效。"),
    shared_mcp_disabled: t("请先启动共享 MCP 服务。"),
    shared_mcp_bind: t("共享 MCP 端口被占用，启动失败。"),
    shared_mcp_limit: t("连接已存在或已达到共享 MCP 数量限制。"),
    shared_mcp_cleanup_required: t("请先清理此配置的旧连接，再重新注册。"),
    shared_mcp_expired: t("调用已取消或审核已过期，请让客户端重新发起。"),
    shared_mcp_credentials: t("共享连接凭据无法安全保存或读取。"),
    shared_mcp_credentials_cleanup: t("连接已停用，凭据清理未完成；请重试撤销。"),
    shared_mcp_config: t("客户端配置写入失败，共享连接未注册。"),
    shared_mcp_config_cleanup: t("连接已停用，客户端配置清理未完成；请重试撤销。"),
    shared_mcp_plugins: t("插件版本或授权已变化，请重新审阅调用。"),
    shared_mcp_store: t("共享 MCP 设置无法保存或读取。"),
    shared_mcp_server: t("共享 MCP 服务已停止，请重新启动。"),
  };
  return messages[code] ?? t("共享 MCP 操作失败，请刷新状态后重试。");
}

async function nativeCall<T>(command: string, argumentsValue?: Record<string, unknown>): Promise<T> {
  if (!isSharedMcpDesktopRuntime()) throw new Error(t("共享 MCP 操作需要桌面应用"));
  try { return await invoke<T>(command, argumentsValue); }
  catch (cause) { throw new Error(sharedMcpErrorText(cause)); }
}

function requireConfirmation(confirmed: boolean): void {
  if (!confirmed) throw new Error(sharedMcpErrorText("shared_mcp_confirmation"));
}

export async function loadSharedMcpStatus(): Promise<SharedMcpStatus> {
  return nativeCall("shared_mcp_status");
}

export async function loadSharedMcpPending(): Promise<SharedToolApproval[]> {
  return nativeCall("shared_mcp_pending");
}

export async function setSharedMcpEnabled(enabled: boolean, confirmed: boolean): Promise<SharedMcpStatus> {
  requireConfirmation(confirmed);
  return nativeCall("shared_mcp_set_enabled", { enabled, confirmed });
}

export async function registerSharedMcp(owner: SharedMcpOwner, confirmed: boolean, sharedScopeConfirmed: boolean): Promise<SharedMcpConnection> {
  requireConfirmation(confirmed);
  const scope = sharedMcpOwners.find(candidate => candidate.id === owner);
  if (!scope) throw new Error(sharedMcpErrorText("shared_mcp_scope"));
  if (scope.clients.length > 1 && !sharedScopeConfirmed) throw new Error(t("请确认共享配置的全部客户端授权范围。"));
  return nativeCall("shared_mcp_register", { owner, confirmed, sharedScopeConfirmed });
}

export async function revokeSharedMcp(id: string, confirmed: boolean): Promise<void> {
  requireConfirmation(confirmed);
  return nativeCall("shared_mcp_revoke", { id, confirmed });
}

export async function decideSharedMcp(id: string, approved: boolean, confirmed: boolean): Promise<void> {
  if (approved) requireConfirmation(confirmed);
  return nativeCall("shared_mcp_decide", { id, approved, confirmed });
}
