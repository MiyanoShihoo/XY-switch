import { invoke } from "@tauri-apps/api/core";

/**
 * 应用级偏好。以前这三项只写 localStorage，后端一行都不读：
 * 超时恒定 180 秒、日志恒定 info、关闭窗口永远隐藏到托盘。
 * 现在它们存在 SQLite 的 settings 表里，由后端真正读取。
 */
export interface AppPreferences {
  /** 关闭主窗口时隐藏到托盘；关掉后点关闭按钮就是退出进程。 */
  minimizeToTray: boolean;
  /** 转发到上游的整体请求超时（秒），15–600。连接超时固定 10 秒。 */
  requestTimeoutSecs: number;
  /** tracing 日志等级，需重启应用生效。 */
  logLevel: LogLevel;
}

export type LogLevel = "debug" | "info" | "warn" | "error";

export const logLevels: LogLevel[] = ["debug", "info", "warn", "error"];

/** 和 Rust 侧 `AppPreferences::default()` 保持一致，浏览器预览模式也用这套值。 */
export const defaultPreferences: AppPreferences = {
  minimizeToTray: true,
  requestTimeoutSecs: 180,
  logLevel: "info",
};

export const minRequestTimeoutSecs = 15;
export const maxRequestTimeoutSecs = 600;

function isDesktopRuntime(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

/** 保存前先在前端拦一遍，错误提示比后端的更贴近控件。 */
export function validatePreferences(preferences: AppPreferences): string {
  const timeout = preferences.requestTimeoutSecs;
  if (!Number.isInteger(timeout)) return "请求超时必须是整数秒";
  if (timeout < minRequestTimeoutSecs || timeout > maxRequestTimeoutSecs) {
    return `请求超时必须在 ${minRequestTimeoutSecs} 到 ${maxRequestTimeoutSecs} 秒之间`;
  }
  if (!logLevels.includes(preferences.logLevel)) {
    return `未知的日志等级：${preferences.logLevel}`;
  }
  return "";
}

export async function loadPreferences(): Promise<AppPreferences> {
  if (!isDesktopRuntime()) return { ...defaultPreferences };
  return invoke<AppPreferences>("get_app_preferences");
}

export async function savePreferences(
  preferences: AppPreferences,
): Promise<AppPreferences> {
  if (!isDesktopRuntime()) throw new Error("浏览器预览模式不能修改应用偏好");
  return invoke<AppPreferences>("save_app_preferences", { preferences });
}

/**
 * 开机自启的真实状态以 Windows 注册表 Run 项为准，不走 AppPreferences：
 * 用户可能在任务管理器「启动」页里手动禁用，界面必须跟着变。
 */
export async function loadAutostart(): Promise<boolean> {
  if (!isDesktopRuntime()) return false;
  return invoke<boolean>("get_autostart_enabled");
}

/** 返回写入后回读的真实状态，被安全软件拦掉时会是 false。 */
export async function saveAutostart(enabled: boolean): Promise<boolean> {
  if (!isDesktopRuntime()) throw new Error("浏览器预览模式不能修改开机自启");
  return invoke<boolean>("set_autostart_enabled", { enabled });
}
