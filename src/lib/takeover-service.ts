import { invoke } from "@tauri-apps/api/core";
import type { AppTakeoverStatus, ManagedApp } from "../types/takeover";

const previewStatuses: AppTakeoverStatus[] = [
  { app: "claude", label: "Claude Code", configPaths: ["~/.claude/settings.json"], installed: false, active: false, lastError: null },
  { app: "codex", label: "Codex", configPaths: ["~/.codex/config.toml"], installed: false, active: false, lastError: null },
  { app: "gemini", label: "Gemini CLI", configPaths: ["~/.gemini/.env", "~/.gemini/settings.json"], installed: false, active: false, lastError: null },
];

function isDesktopRuntime(): boolean {
  return "__TAURI_INTERNALS__" in window;
}

export async function loadAppTakeovers(): Promise<AppTakeoverStatus[]> {
  if (!isDesktopRuntime()) return previewStatuses;
  return invoke<AppTakeoverStatus[]>("list_app_takeovers");
}

export async function setAppTakeoverEnabled(
  app: ManagedApp,
  enabled: boolean,
): Promise<AppTakeoverStatus> {
  if (!isDesktopRuntime()) throw new Error("浏览器预览模式不能修改 CLI 配置");
  return invoke<AppTakeoverStatus>("set_app_takeover_enabled", { app, enabled });
}
