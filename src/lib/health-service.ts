import { invoke } from "@tauri-apps/api/core";
import type { ProviderHealth, RequestSummary } from "../types/provider";

const EMPTY_REQUEST_SUMMARY: RequestSummary = {
  totalRequests: 0,
  successfulRequests: 0,
  averageLatencyMs: 0,
};

export function isDesktopRuntime(): boolean {
  return "__TAURI_INTERNALS__" in window;
}

export async function loadProviderHealth(): Promise<ProviderHealth[]> {
  if (!isDesktopRuntime()) return [];
  return invoke<ProviderHealth[]>("list_provider_health");
}

export async function loadRequestSummary(): Promise<RequestSummary> {
  if (!isDesktopRuntime()) return EMPTY_REQUEST_SUMMARY;
  return invoke<RequestSummary>("request_summary");
}

export async function testProviderHealth(providerId: string): Promise<ProviderHealth> {
  if (!isDesktopRuntime()) throw new Error("测速功能仅在桌面应用中可用");
  return invoke<ProviderHealth>("test_provider_health", { providerId });
}

export async function testAllProviderHealth(): Promise<ProviderHealth[]> {
  if (!isDesktopRuntime()) throw new Error("测速功能仅在桌面应用中可用");
  return invoke<ProviderHealth[]>("test_all_provider_health");
}
