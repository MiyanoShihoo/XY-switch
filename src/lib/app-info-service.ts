import { invoke } from "@tauri-apps/api/core";

/** 后端 `app_paths` 命令的返回，字段名与 Rust 侧 camelCase 序列化一致。 */
export interface AppPaths {
  dataDir: string;
  databasePath: string;
  homeDir: string;
  codexHome: string;
}

function isDesktopRuntime(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

/**
 * 读取应用版本号。以前「关于」页写死了 v1.0.0，
 * 后端其实一直有 `app_version` 命令，但前端从来没调过。
 */
export async function loadAppVersion(): Promise<string> {
  if (!isDesktopRuntime()) return "";
  return invoke<string>("app_version");
}

/** 读取数据目录、数据库文件与 Codex 配置目录的真实路径。 */
export async function loadAppPaths(): Promise<AppPaths | null> {
  if (!isDesktopRuntime()) return null;
  return invoke<AppPaths>("app_paths");
}
