import { invoke } from "@tauri-apps/api/core";
import { useQuery } from "@tanstack/react-query";

export interface PlatformCapabilities {
  os: string;
  mobile: boolean;
  systemTray: boolean;
  autostart: boolean;
  clientTakeover: boolean;
  credentialStore: string;
}
export const browserCapabilities: PlatformCapabilities = Object.freeze({
  os: "browser", mobile: false, systemTray: false, autostart: false,
  clientTakeover: false, credentialStore: "Unavailable",
});

export async function loadPlatformCapabilities(): Promise<PlatformCapabilities> {
  if (typeof window === "undefined" || !("__TAURI_INTERNALS__" in window)) return browserCapabilities;
  return invoke<PlatformCapabilities>("app_platform");
}

export function usePlatformCapabilities() {
  return useQuery({ queryKey: ["app-platform"], queryFn: loadPlatformCapabilities, retry: false, staleTime: Infinity });
}
