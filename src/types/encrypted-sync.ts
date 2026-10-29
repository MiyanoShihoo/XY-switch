export interface SyncPreview<T> { ticketId: string; expiresAt: number; content: T }
export interface SyncPayload {
  schema: number;
  providers: { id: string; name: string; providerType: string; appGroup: "Claude" | "Codex" | "Gemini"; endpoint: string; model: string; upstreamFormat: string; isFullUrl: boolean; modelMappings: { model: string; displayName: string; contextWindow: number | null }[] }[];
  favorites: { providerId: string; sessionId: string }[];
  prices: { id: string; inputPerMillion: number; outputPerMillion: number; cacheReadPerMillion: number | null; cacheCreationPerMillion: number | null }[];
}
export interface SyncChange { key: string; kind: "provider" | "favorite" | "price"; status: "add" | "conflict" | "unchanged"; effect: string; local: unknown; incoming: unknown }
export interface SyncChoice { key: string; useIncoming: boolean }
export interface SyncApplied { backupId: string; applied: number; disabledCopies: number }
export interface SyncBackup { id: string; createdAt: number; used: boolean }
export interface SyncRollback { before: unknown; after: unknown }
