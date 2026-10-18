import { invoke } from "@tauri-apps/api/core";
import type { SessionMeta, SessionMessage } from "../types/session";

const IS_PREVIEW = typeof window === "undefined" || !("__TAURI_INTERNALS__" in window);

export async function listSessions(): Promise<SessionMeta[]> {
  if (IS_PREVIEW) return [];
  return invoke<SessionMeta[]>("list_sessions");
}

export async function getSessionMessages(providerId: string, sessionId: string): Promise<SessionMessage[]> {
  if (IS_PREVIEW) return [];
  return invoke<SessionMessage[]>("get_session_messages", { providerId, sessionId });
}

export async function deleteSessions(ids: Array<{providerId: string; sessionId: string}>): Promise<number> {
  if (IS_PREVIEW) return 0;
  return invoke<number>("delete_sessions", { ids });
}
