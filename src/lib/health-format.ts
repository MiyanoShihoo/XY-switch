import { t } from "../i18n/language";
import type { Provider, ProviderHealth } from "../types/provider";

export type HealthTone = "healthy" | "warning" | "danger" | "muted";

export interface HealthDisplay {
  label: string;
  tone: HealthTone;
  short?: string;
}

export function healthDisplay(provider: Provider, health?: ProviderHealth, now = Date.now()): HealthDisplay {
  if (!provider.enabled) return { label: t("已禁用"), tone: "muted" };
  if (!health) return { label: t("未测速"), tone: "muted" };
  if (health.lastError === "api_key_missing") return { label: t("未配置密钥"), tone: "warning" };
  if (health.circuitOpenUntil !== null && health.circuitOpenUntil > now) {
    return { label: t("熔断中"), tone: "danger" };
  }
  if (health.status === "healthy") {
    return { label: t("健康"), tone: "healthy", short: health.latencyMs != null ? `${health.latencyMs}ms` : undefined };
  }
  if (health.status === "circuit_open") return { label: t("等待恢复"), tone: "warning" };
  if (health.status === "unhealthy") return { label: t("异常"), tone: "danger" };
  return { label: t("状态未知"), tone: "muted" };
}

export function formatSuccessRate(successful: number, total: number): string {
  if (total <= 0) return t("暂无数据");
  return `${((successful / total) * 100).toFixed(1)}%`;
}
