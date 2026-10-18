import type { SessionMeta } from "../types/session";
export function sessionKey(session: Pick<SessionMeta, "providerId" | "sessionId">): string { return JSON.stringify([session.providerId, session.sessionId]); }
export function timestampMs(value?: number): number { return value && Number.isFinite(value) && value > 0 ? (value < 1e12 ? value * 1000 : value) : 0; }
export function formatSessionTime(value?: number): string {
  const ms = timestampMs(value); if (!ms) return "未记录";
  const date = new Date(ms); return Number.isNaN(date.getTime()) ? "未记录" : date.toLocaleString("zh-CN", { hour12: false });
}
export function sessionTitle(session: SessionMeta): string { return session.title?.trim() || session.summary?.trim() || session.sessionId; }
