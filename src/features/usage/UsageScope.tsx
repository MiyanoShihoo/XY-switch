import { localeTag } from "../../i18n/language";
import type { UsageFilter } from "../../types/usage";
import { t } from "../../i18n/language";

export function UsageScope({ filter, providerNames }: { filter: UsageFilter; providerNames: Map<string, string> }) {
  const dates: Record<string, string> = { today: t("今天"), yesterday: t("昨天"), "1d": t("近 24 小时"), "7d": t("近 7 天"), "14d": t("近 14 天"), "30d": t("近 30 天") };
  return <div className="usage-scope" aria-label={t("已应用范围")}>
    <strong>{t("已应用范围")}</strong>
    <span>{filter.startTimeMs != null ? new Date(filter.startTimeMs).toLocaleString(localeTag()) : dates[filter.dateRange ?? ""] ?? t("全部时间")}</span>
    <span>{filter.appType ? t(filter.appType === "model_probe" ? "模型探测" : filter.appType) : t("全部客户端")}</span>
    <span className="usage-scope__technical" tabIndex={0}>{filter.providerId ? providerNames.get(filter.providerId) ?? filter.providerId : t("全部供应商")}</span>
    <code tabIndex={0}>{filter.model || t("全部模型")}</code>
    <span>{filter.status === "success" ? t("成功") : filter.status === "error" ? t("失败") : t("全部状态")}</span>
  </div>;
}
