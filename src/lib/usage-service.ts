import { invoke } from "@tauri-apps/api/core";
import { t } from "../i18n/language";
import type {
  UsageSummary,
  LocalUsageSync,
  UsageLogEntry,
  ModelPricing,
  ModelPricingInput,
  UsageFilter,
  UsageTrendPoint,
  ProviderUsageStat,
  ModelUsageStat,
} from "../types/usage";

const IS_PREVIEW = typeof window === "undefined" || !("__TAURI_INTERNALS__" in window);

export async function getUsageSummary(filter: UsageFilter): Promise<UsageSummary> {
  if (IS_PREVIEW) return { totalRequests: 0, successCount: 0, unknownCostRequests: 0, totalInputTokens: 0, totalOutputTokens: 0, totalCacheReadTokens: 0, totalCacheCreationTokens: 0, totalCostUsd: "0.00", avgLatencyMs: 0 };
  return invoke<UsageSummary>("get_usage_summary", { filter });
}

export async function getUsageLogs(filter: UsageFilter, limit: number, offset: number): Promise<UsageLogEntry[]> {
  if (IS_PREVIEW) return [];
  return invoke<UsageLogEntry[]>("get_usage_logs", { filter, limit, offset });
}

export async function getUsageTrends(filter: UsageFilter): Promise<UsageTrendPoint[]> {
  if (IS_PREVIEW) return [];
  return invoke<UsageTrendPoint[]>("get_usage_trends", { filter });
}

export async function getProviderStats(filter: UsageFilter): Promise<ProviderUsageStat[]> {
  if (IS_PREVIEW) return [];
  return invoke<ProviderUsageStat[]>("get_provider_stats", { filter });
}

export async function getModelStats(filter: UsageFilter): Promise<ModelUsageStat[]> {
  if (IS_PREVIEW) return [];
  return invoke<ModelUsageStat[]>("get_model_stats", { filter });
}

function requireNativePricing(): void {
  if (typeof window === "undefined" || !("__TAURI_INTERNALS__" in window)) {
    throw new Error(t("模型定价管理需要原生程序；浏览器预览不会保存或同步价格。"));
  }
}

export async function listModelPricing(): Promise<ModelPricing[]> {
  requireNativePricing();
  return invoke<ModelPricing[]>("list_model_pricing");
}

export async function saveModelPricing(pricing: ModelPricingInput): Promise<void> {
  requireNativePricing();
  const { id, inputPerMillion, outputPerMillion, cacheReadPerMillion, cacheCreationPerMillion } = pricing;
  return invoke("save_model_pricing", { pricing: { id, inputPerMillion, outputPerMillion, cacheReadPerMillion, cacheCreationPerMillion } });
}

export async function deleteModelPricing(id: string): Promise<void> {
  requireNativePricing();
  return invoke("delete_model_pricing", { id });
}

export async function syncModelsDevPricing(): Promise<number> {
  requireNativePricing();
  return invoke<number>("sync_models_dev_pricing");
}

export async function syncLocalUsage(): Promise<LocalUsageSync> {
  if (IS_PREVIEW) return { codexImported: 0, harnessImported: 0, warnings: [] };
  return invoke<LocalUsageSync>("sync_local_usage");
}
