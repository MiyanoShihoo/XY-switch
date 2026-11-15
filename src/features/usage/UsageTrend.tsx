import { useState } from "react";
import { localeTag, t, useLanguage } from "../../i18n/language";
import { DataTable, EmptyState, Select } from "../../ui";
import type { DateRange, UsageTrendPoint } from "../../types/usage";
import { aggregateCostLabel, costLabel, inputOutputTokens, partialLabel, tokenLabel } from "./usage-presentation";

export function UsageTrend({ points, dateRange }: { points: UsageTrendPoint[]; dateRange?: DateRange }) {
  useLanguage();
  const [metric, setMetric] = useState("requests");
  const hourly = dateRange === "today" || dateRange === "yesterday" || dateRange === "1d" || dateRange === "7d";
  const dateFormat = new Intl.DateTimeFormat(localeTag(), { month: "short", day: "numeric" });
  const hourFormat = new Intl.DateTimeFormat(localeTag(), { hour: "2-digit", minute: "2-digit", hour12: false });
  const timeFormat = new Intl.DateTimeFormat(localeTag(), { year: hourly ? undefined : "numeric", month: "short", day: "numeric", ...(hourly ? { hour: "2-digit", minute: "2-digit", hour12: false } as const : {}) });
  const metricLabel = metric === "cost" ? t("成本（USD）") : metric === "tokens" ? t("输入 + 输出 token") : t("请求数");
  const readings = points.map((point, index) => {
    const tokens = inputOutputTokens(point);
    const cost = point.requests > 0 && point.unknownCostRequests != null && point.unknownCostRequests >= point.requests ? null : point.cost;
    const raw = metric === "cost" ? cost : metric === "tokens" ? tokens.value : point.requests;
    const value = raw !== null && Number.isFinite(raw) ? raw : null;
    return { point, index, value, label: metric === "cost" ? aggregateCostLabel(value, point.unknownCostRequests, point.requests) : value === null ? t("未知") : partialLabel(value.toLocaleString(localeTag()), metric === "tokens" && tokens.knownCount < point.requests * 2) };
  });
  const maximum = Math.max(0, ...readings.map(reading => reading.value ?? 0));
  const hasKnownReadings = readings.some(reading => reading.value !== null);
  const dense = readings.length > 12;
  const tickCount = Math.min(6, readings.length);
  const ticks = Array.from({ length: tickCount }, (_, tick) => readings[Math.round(tick * (readings.length - 1) / Math.max(1, tickCount - 1))]);
  const gridStyle = { gridTemplateColumns: "repeat(" + readings.length + ", minmax(0, 1fr))", columnGap: dense ? 0 : 4 };
  const scaleLabel = (value: number) => metric === "cost" ? costLabel(value) : value.toLocaleString(localeTag(), { notation: value >= 10000 ? "compact" : "standard", maximumFractionDigits: 1 });
  return <section className="usage-trend" aria-labelledby="usage-trend-title">
    <div className="section-heading"><h2 id="usage-trend-title">{t("请求趋势")}</h2><Select aria-label={t("趋势指标")} value={metric} onChange={event => setMetric(event.target.value)}><option value="requests">{t("请求数")}</option><option value="tokens">{t("输入 + 输出 token")}</option><option value="cost">{t("成本（USD）")}</option></Select></div>
    {readings.length ? <>
      {readings.length === 1 ? <dl className="usage-trend__single"><div><dt><time dateTime={new Date(readings[0].point.timestamp).toISOString()}>{timeFormat.format(readings[0].point.timestamp)}</time><span>{metricLabel}</span></dt><dd className={readings[0].value === null ? "usage-trend__unknown" : undefined}>{readings[0].label}</dd></div></dl>
      : !hasKnownReadings ? <p className="usage-trend__unavailable">{t("当前指标暂无已知数值")}</p> : <div className="usage-trend__chart" data-dense={dense}>
        <div className="usage-trend__scale" data-flat={maximum === 0} aria-hidden="true">{maximum > 0 && <><span>{scaleLabel(maximum)}</span><span>{scaleLabel(maximum / 2)}</span></>}<span>0</span></div>
        <ol className="usage-trend__plot" tabIndex={0} aria-label={t("趋势柱状图") + " · " + metricLabel} style={gridStyle}>
          {readings.map(({ point, index, value, label }) => <li key={point.timestamp} style={{ gridColumn: index + 1 }} title={timeFormat.format(point.timestamp) + " · " + label} aria-label={timeFormat.format(point.timestamp) + " · " + label}>
            {!dense && <strong className={"usage-trend__value" + (value === null ? " usage-trend__unknown" : "")}>{label}</strong>}
            <span className="usage-trend__bar-slot" aria-hidden="true">{value === null ? <span className="usage-trend__unknown">—</span> : <span className="usage-trend__bar" style={{ height: (value > 0 ? Math.max(2, value / maximum * 100) : 0) + "%" }} />}</span>
          </li>)}
        </ol>
        <div className="usage-trend__time-axis" aria-hidden="true" style={gridStyle}>{ticks.map(reading => <time key={reading.point.timestamp} dateTime={new Date(reading.point.timestamp).toISOString()} style={{ gridColumn: reading.index + 1 }}><span>{dateFormat.format(reading.point.timestamp)}</span>{hourly && <span>{hourFormat.format(reading.point.timestamp)}</span>}</time>)}</div>
      </div>}
      {metric === "tokens" && <><p className="usage-footnote">{t("未知 token 未计入；已知部分仅包含已记录数值。")}</p><p className="usage-footnote">{t("输入及缓存按各来源的记录口径展示，不应直接相加。")}</p></>}
      {metric === "cost" && <p className="usage-footnote">{t("趋势仅汇总已知费用；未知费用未计入，空值不是零。")}</p>}
      <details className="usage-trend__data"><summary>{t("查看趋势数据")}</summary><DataTable caption={t("趋势原始数据")} rows={points} rowKey={row => String(row.timestamp)} columns={[
        { key: "date", label: t("时间"), render: row => <time dateTime={new Date(row.timestamp).toISOString()}>{timeFormat.format(row.timestamp)}</time> },
        { key: "requests", label: t("请求数"), numeric: true, render: row => row.requests.toLocaleString(localeTag()) },
        { key: "tokens", label: t("输入 / 输出"), numeric: true, render: row => tokenLabel(row.inputTokens, row.tokenCoverage?.input, row.requests) + " / " + tokenLabel(row.outputTokens, row.tokenCoverage?.output, row.requests) },
        { key: "cost", label: t("成本"), numeric: true, render: row => aggregateCostLabel(row.cost, row.unknownCostRequests, row.requests) },
      ]} /></details>
    </> : <EmptyState title={t("暂无趋势数据")} description={t("本地路由产生请求后，在这里查看变化。")} />}
  </section>;
}
