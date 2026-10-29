import { t } from "../i18n/language";
import { invoke } from "@tauri-apps/api/core";

export interface DiscoveredModel {
  id: string;
  name: string;
}

export interface ModelDiscoveryRequest {
  providerId: string;
  providerType: string;
  upstreamFormat: string;
  endpoint: string;
  apiKey?: string;
}

export async function discoverProviderModels(request: ModelDiscoveryRequest): Promise<DiscoveredModel[]> {
  if (!("__TAURI_INTERNALS__" in window)) {
    throw new Error(t("模型探索仅在桌面应用中可用"));
  }
  return invoke<DiscoveredModel[]>("discover_provider_models", { request });
}
