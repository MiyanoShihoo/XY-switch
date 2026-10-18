import { invoke } from "@tauri-apps/api/core";
import type { ProxySettings, ProxyStatus } from "../types/proxy";

const previewStatus: ProxyStatus = {
  settings: {
    enabled: false,
    showInHome: false,
    host: "127.0.0.1",
    port: 15721,
    routeTargets: ["claude", "codex", "gemini", "grok", "opencode"],
  },
  running: false,
  address: "http://127.0.0.1:15721",
  lastError: null,
};

function isDesktopRuntime(): boolean {
  return "__TAURI_INTERNALS__" in window;
}

export async function loadProxyStatus(): Promise<ProxyStatus> {
  if (!isDesktopRuntime()) return previewStatus;
  return invoke<ProxyStatus>("get_proxy_status");
}

/** Preserve the backend's disabled startup state; never turn routing on just by opening the UI. */
export async function loadInitialProxyStatus(): Promise<ProxyStatus> {
  const status = await loadProxyStatus();
  if (!status.settings.enabled || status.running) return status;
  try {
    return await setProxyEnabled(true);
  } catch (error) {
    console.warn("Auto-start proxy on launch failed:", error);
    return status;
  }
}

export async function saveProxySettings(settings: ProxySettings): Promise<ProxyStatus> {
  if (!isDesktopRuntime()) throw new Error("浏览器预览模式不能修改本地路由");
  return invoke<ProxyStatus>("save_proxy_settings", { settings });
}

export async function setProxyEnabled(enabled: boolean): Promise<ProxyStatus> {
  if (!isDesktopRuntime()) throw new Error("浏览器预览模式不能启动本地路由");
  return invoke<ProxyStatus>("set_proxy_enabled", { enabled });
}
