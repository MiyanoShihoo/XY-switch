import { useState } from "react";
import { localeTag, t, useLanguage } from "../../i18n/language";
import { Button, DataTable, Tabs } from "../../ui";
import type { ModelUsageStat, ProviderUsageStat, UsageFilter } from "../../types/usage";
import { aggregateCostLabel, durationLabel, tokenLabel } from "./usage-presentation";

export function UsageContributions({ providerStats, modelStats, providerNames, onInspect }: { providerStats: ProviderUsageStat[]; modelStats: ModelUsageStat[]; providerNames: Map<string, string>; onInspect: (scope: Partial<UsageFilter>) => void }) {
  useLanguage();
  const [view, setView] = useState("providers");
  return <section className="usage-contributions" aria-label={t("统计明细")}>
    <div className="usage-contributions__switch"><Tabs id="usage-breakdown" label={t("统计明细")} value={view} onChange={setView} items={[{ value: "providers", label: t("供应商统计") }, { value: "models", label: t("模型统计") }]} /></div>
    <div className="usage-contributions__columns">
      <section className="usage-contributions__group" data-current={view === "providers"} aria-labelledby="usage-provider-stats-title">
        <h2 id="usage-provider-stats-title">{t("供应商统计")}</h2>
        <div className="usage-table-region usage-contributions__table" role="region" tabIndex={0} aria-label={t("供应商用量统计")}><DataTable caption={t("供应商用量统计")} rows={providerStats} rowKey={row => row.providerId} columns={[
          { key: "provider", label: t("供应商"), render: row => <><strong className="usage-provider-name">{providerNames.get(row.providerId) ?? row.providerId}</strong><span className="usage-cell-note">{t("成功率")} {row.successRate.toFixed(1)}% · {t("平均耗时")} {durationLabel(row.avgLatencyMs)}</span><span className="usage-cell-note">{t("输入 / 输出")} <span className="usage-contributions__tokens">{tokenLabel(row.inputTokens, row.tokenCoverage?.input, row.totalRequests)} / {tokenLabel(row.outputTokens, row.tokenCoverage?.output, row.totalRequests)}</span></span></> },
          { key: "requests", label: t("请求数"), numeric: true, render: row => row.totalRequests.toLocaleString(localeTag()) },
          { key: "cost", label: t("成本"), numeric: true, render: row => aggregateCostLabel(row.totalCostUsd, row.unknownCostRequests, row.totalRequests) },
          { key: "inspect", label: t("请求记录"), render: row => <Button size="sm" variant="quiet" aria-label={t("查看 {name} 的请求", { name: providerNames.get(row.providerId) ?? row.providerId })} onClick={() => onInspect({ providerId: row.providerId })}>{t("查看请求")}</Button> },
        ]} /></div>
      </section>
      <section className="usage-contributions__group" data-current={view === "models"} aria-labelledby="usage-model-stats-title">
        <h2 id="usage-model-stats-title">{t("模型统计")}</h2>
        <div className="usage-table-region usage-contributions__table" role="region" tabIndex={0} aria-label={t("模型用量统计")}><DataTable caption={t("模型用量统计")} rows={modelStats} rowKey={row => JSON.stringify([row.providerId, row.model])} columns={[
          { key: "model", label: t("模型 / 供应商"), render: row => <><code tabIndex={0}>{row.model}</code><span className="usage-cell-note usage-provider-name">{providerNames.get(row.providerId) ?? row.providerId}</span><span className="usage-cell-note">{t("输入 / 输出")} <span className="usage-contributions__tokens">{tokenLabel(row.inputTokens, row.tokenCoverage?.input, row.totalRequests)} / {tokenLabel(row.outputTokens, row.tokenCoverage?.output, row.totalRequests)}</span></span></> },
          { key: "requests", label: t("请求数"), numeric: true, render: row => row.totalRequests.toLocaleString(localeTag()) },
          { key: "cost", label: t("成本"), numeric: true, render: row => aggregateCostLabel(row.totalCostUsd, row.unknownCostRequests, row.totalRequests) },
          { key: "inspect", label: t("请求记录"), render: row => <Button size="sm" variant="quiet" aria-label={t("查看 {name} 的请求", { name: row.model })} onClick={() => onInspect({ providerId: row.providerId, model: row.model })}>{t("查看请求")}</Button> },
        ]} /></div>
      </section>
    </div>
  </section>;
}
