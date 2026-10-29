import { invoke } from "@tauri-apps/api/core";
import { t } from "../i18n/language";
import type { SessionCatalog, SessionDetail, SessionIdPair, SessionMeta, SessionMessage } from "../types/session";

function requireNative(): void {
  if (typeof window === "undefined" || !("__TAURI_INTERNALS__" in window)) {
    throw new Error(t("本地会话需要原生桌面程序，浏览器预览不能读取客户端文件。"));
  }
}

export async function getSessionCatalog(): Promise<SessionCatalog> {
  requireNative();
  return invoke<SessionCatalog>("get_session_catalog");
}

export async function listSessions(): Promise<SessionMeta[]> {
  requireNative();
  return invoke<SessionMeta[]>("list_sessions");
}

export async function getSessionDetail(providerId: string, sessionId: string): Promise<SessionDetail> {
  requireNative();
  return invoke<SessionDetail>("get_session_detail", { providerId, sessionId });
}

export async function getSessionMessages(providerId: string, sessionId: string): Promise<SessionMessage[]> {
  requireNative();
  return invoke<SessionMessage[]>("get_session_messages", { providerId, sessionId });
}

export async function deleteSessions(ids: SessionIdPair[]): Promise<number> {
  requireNative();
  return invoke<number>("delete_sessions", { ids });
}
