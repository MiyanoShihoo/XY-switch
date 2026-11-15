import { ArrowRight } from "lucide-react";
import { localeTag, t, useLanguage } from "../../i18n/language";
import type { UsageSummary } from "../../types/usage";
import { Button } from "../../ui";
import { aggregateCostLabel, durationLabel, tokenLabel } from "./usage-presentation";

export function UsageLedger({ summary, onInspect }: { summary: UsageSummary; onInspect?: (status?: "error") => void }) {
  useLanguage();
  const unsuccessful = summary.totalRequests - summary.successCount;
  return <section className="usage-ledger" aria-label={t("筛选范围汇总")}>
    <header className="usage-ledger__heading"><h2>{t("请求完成情况")}</h2>{onInspect && <Button variant="quiet" size="sm" onClick={() => onInspect()}>{t("查看请求记录")}<ArrowRight size={14} aria-hidden="true" /></Button>}</header>
      <dl className="usage-ledger__requests" aria-label={t("请求完成情况")}>
        <div><dt>{t("请求数")}</dt><dd>{summary.totalRequests.toLocaleString(localeTag())}</dd></div>
        <div><dt>{t("成功率")}</dt><dd>{summary.totalRequests ? (summary.successCount / summary.totalRequests * 100).toFixed(1) + "%" : "—"}</dd>{unsuccessful > 0 && onInspect && <Button variant="quiet" size="sm" className="usage-ledger__inspect" onClick={() => onInspect("error")}>{t("{count} 条未成功", { count: unsuccessful.toLocaleString(localeTag()) })}<ArrowRight size={13} aria-hidden="true" /></Button>}</div>
        <div><dt>{t("代理平均耗时")}</dt><dd>{durationLabel(summary.avgLatencyMs)}</dd></div>
      </dl>
    <div className="usage-ledger__accounting">
      <div className="usage-ledger__account"><h3>{t("已知费用合计")}</h3><strong className="usage-ledger__amount">{aggregateCostLabel(summary.totalCostUsd, summary.unknownCostRequests, summary.totalRequests)} <small>USD</small></strong><p className="usage-footnote">{summary.unknownCostRequests > 0 && <span className="usage-ledger__unknown">{t("{count} 条费用未知，未计入合计。", { count: summary.unknownCostRequests })} </span>}{t("不代表上游最终账单")}</p></div>
    <div className="usage-ledger__tokens" aria-label={t("用量对照")}><dl>
      <div><dt>{t("输入")}</dt><dd>{tokenLabel(summary.totalInputTokens, summary.tokenCoverage?.input, summary.totalRequests)}</dd></div><div><dt>{t("输出")}</dt><dd>{tokenLabel(summary.totalOutputTokens, summary.tokenCoverage?.output, summary.totalRequests)}</dd></div><div><dt>{t("缓存读取")}</dt><dd>{tokenLabel(summary.totalCacheReadTokens, summary.tokenCoverage?.cacheRead, summary.totalRequests)}</dd></div><div><dt>{t("缓存写入")}</dt><dd>{tokenLabel(summary.totalCacheCreationTokens, summary.tokenCoverage?.cacheCreation, summary.totalRequests)}</dd></div>
    </dl><p className="usage-footnote">{t("未知 token 未计入；已知部分仅包含已记录数值。")}</p><p className="usage-footnote">{t("输入及缓存按各来源的记录口径展示，不应直接相加。")}</p></div>
    </div>
  </section>;
}
