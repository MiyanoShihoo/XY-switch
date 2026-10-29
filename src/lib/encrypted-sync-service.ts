import { invoke } from "@tauri-apps/api/core";
import { prepareNativeFavorites } from "./native-favorites-service";
import type { SyncApplied, SyncBackup, SyncChange, SyncChoice, SyncPayload, SyncPreview, SyncRollback } from "../types/encrypted-sync";
function native() { if (typeof window === "undefined" || !("__TAURI_INTERNALS__" in window)) throw new Error("Encrypted file exchange requires the desktop application"); }
export async function previewSyncExport() { native(); await prepareNativeFavorites(); return invoke<SyncPreview<SyncPayload>>("preview_sync_export"); }
export async function saveSyncExport(ticketId: string, passphrase: string, confirmation: string) { native(); return invoke<{ saved: boolean; auditRecorded: boolean }>("save_sync_export", { ticketId, passphrase, confirmation }); }
export async function previewSyncImport(passphrase: string) { native(); await prepareNativeFavorites(); return invoke<SyncPreview<SyncChange[]> | null>("preview_sync_import", { passphrase }); }
export async function applySyncImport(ticketId: string, choices: SyncChoice[], confirmation: string) { native(); return invoke<SyncApplied>("apply_sync_import", { ticketId, choices, confirmation }); }
export async function cancelSyncPreview(ticketId: string) { native(); await invoke("cancel_sync_preview", { ticketId }); }
export async function listSyncBackups() { native(); return invoke<SyncBackup[]>("list_sync_backups"); }
export async function previewSyncRollback(id: string) { native(); await prepareNativeFavorites(); return invoke<SyncPreview<SyncRollback>>("preview_sync_rollback", { id }); }
export async function applySyncRollback(ticketId: string, confirmation: string) { native(); await invoke("apply_sync_rollback", { ticketId, confirmation }); }
export function validSyncPassphrase(value: string) { return [...value].length >= 12 && new TextEncoder().encode(value).length <= 1024; }
