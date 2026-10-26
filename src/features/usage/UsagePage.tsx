import { localeTag, t, useLanguage } from "../../i18n/language";
import { useEffect, useId, useMemo, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { ChevronLeft, ChevronRight, RefreshCw } from "lucide-react";
import { Badge, Button, DataTable, EmptyState, Field, Input, InlineError, LoadingState, Modal, Select, Tabs } from "../../ui";
import { getModelStats, getProviderStats, getUsageLogs, getUsageSummary, getUsageTrends, syncLocalUsage } from "../../lib/usage-service";
import { errorText } from "../../lib/error";
import type { Provider } from "../../types/provider";
import type { DateRange, UsageFilter, UsageLogEntry, UsageTrendPoint } from "../../types/usage";
import { ModelAuditCell } from "./ModelAuditCell";
import { ResponseModelEvidence } from "./ResponseModelEvidence";
import { formatTokens, formatUsd, lastUsagePage, normalizeUsageFilter } from "./usage-format";
import "./usage.css";

const PAGE_SIZE = 20;
const INITIAL_FILTER: UsageFilter = { dateRange: "today" };
const clients = [["claude", "Claude Code"], ["codex", "Codex"], ["deepseek_harness", "DeepSeek Harness"], ["gemini", "Gemini CLI"], ["grok", "Grok"], ["opencode", "OpenCode"], ["model_probe", "模型探测"]];

const isSessionRow = (row: UsageLogEntry) => row.source === "harness_session" || row.source === "codex_session" || row.providerId.startsWith("local:");
const durationLabel = (value: number) => value > 0 ? Math.round(value).toLocaleString(localeTag()) + " ms" : "—";
const statusLabel = (row: UsageLogEntry) => isSessionRow(row) ? (row.statusCode === 200 ? t("已完成") : t("中断 / 失败")) : (row.statusCode ? String(row.statusCode) + (row.success === false ? t(" · 未完成") : "") : t("未取得 HTTP"));

export function UsagePage({ providers }: { providers: Provider[] }) {
  useLanguage();
  const [draft, setDraft] = useState<UsageFilter>(INITIAL_FILTER);
  const [filter, setFilter] = useState<UsageFilter>(INITIAL_FILTER);
  const [page, setPage] = useState(0);
  const [pageInput, setPageInput] = useState("1");
  const [interval, setIntervalSeconds] = useState(30);
  const [tab, setTab] = useState("logs");
  const [selected, setSelected] = useState<UsageLogEntry | null>(null);
  const modelList = useId();
  // Each filter/page owns its cache entry: a slow old request cannot replace a new selection.
  const query = useQuery({
    queryKey: ["usage", filter, page],
    queryFn: async () => {
      const sync = await syncLocalUsage();
      const [summary, logs, trends, providerStats, modelStats] = await Promise.all([
        getUsageSummary(filter), getUsageLogs(filter, PAGE_SIZE, page * PAGE_SIZE),
        getUsageTrends(filter), getProviderStats(filter), getModelStats(filter),
      ]);
      return { summary, logs, trends, providerStats, modelStats, sync };
    },
    retry: false,
    refetchInterval: interval > 0 ? interval * 1000 : false,
    refetchOnWindowFocus: false,
  });
  const data = query.data;
  const lastPage = lastUsagePage(data?.summary.totalRequests ?? 0, PAGE_SIZE);
  useEffect(() => { if (data && page > lastPage) setPage(lastPage); }, [data, page, lastPage]);
  useEffect(() => { setPageInput(String(page + 1)); }, [page]);
  const names = new Map<string, string>([["local:codex", t("Codex · 本地用量")], ["local:deepseek_harness", t("DeepSeek Harness · 本地用量")], ...providers.map(provider => [provider.id, provider.name] as [string, string])]);
  const providerIds = [...new Set([...providers.map(p => p.id), ...(data?.providerStats.map(p => p.providerId) ?? []), draft.providerId].filter((id): id is string => Boolean(id)))];
  const models = useMemo(() => [...new Set([...(data?.modelStats.map(item => item.model) ?? []), ...(data?.logs.map(item => item.model) ?? [])])].sort(), [data]);
  const patch = (value: Partial<UsageFilter>) => setDraft(previous => ({ ...previous, ...value }));
  const apply = (value: UsageFilter) => { setDraft(value); setFilter(normalizeUsageFilter(value)); setPage(0); setSelected(null); };
  const filterDirty = JSON.stringify(normalizeUsageFilter(draft)) !== JSON.stringify(filter);

  return <section className="workspace-page usage-page" aria-labelledby="usage-title">
    <header className="page-heading"><div className="page-heading__copy"><h1 id="usage-title">{t("用量与审计")}</h1><p className="page-description">{t("查看本地路由与 Codex / DeepSeek Harness 会话的用量及可观测证据。")}</p></div><div className="page-actions"><Badge>{data ? data.summary.totalRequests.toLocaleString(localeTag()) + t(" 次请求") : t("待读取")}</Badge><Button variant="secondary" icon={<RefreshCw size={15} />} loading={query.isFetching} onClick={() => void query.refetch()}>{t("刷新")}</Button></div></header>
    <form className="usage-filter" onSubmit={event => { event.preventDefault(); apply(draft); }}>
      <Field label={t("日期范围")}><Select value={draft.dateRange ?? ""} onChange={event => patch({ dateRange: (event.target.value || undefined) as DateRange | undefined })}><option value="today">{t("今天")}</option><option value="yesterday">{t("昨天")}</option><option value="7d">{t("近 7 天")}</option><option value="14d">{t("近 14 天")}</option><option value="30d">{t("近 30 天")}</option><option value="">{t("全部时间")}</option></Select></Field>
      <Field label={t("客户端")}><Select value={draft.appType ?? ""} onChange={event => patch({ appType: event.target.value })}><option value="">{t("全部客户端")}</option>{clients.map(([id,label]) => <option key={id} value={id}>{t(label)}</option>)}</Select></Field>
      <Field label={t("供应商")}><Select value={draft.providerId ?? ""} onChange={event => patch({ providerId: event.target.value })}><option value="">{t("全部供应商")}</option>{providerIds.map(id => <option key={id} value={id}>{names.get(id) ?? id}</option>)}</Select></Field>
      <Field label={t("模型 ID")}><Input list={modelList} value={draft.model ?? ""} placeholder={t("全部模型")} onChange={event => patch({ model: event.target.value })} /></Field>
      <datalist id={modelList}>{models.map(model => <option key={model} value={model} />)}</datalist>
      <Field label={t("请求状态")}><Select value={draft.status ?? ""} onChange={event => patch({ status: event.target.value })}><option value="">{t("全部状态")}</option><option value="success">{t("成功")}</option><option value="error">{t("失败")}</option></Select></Field>
      <div className="usage-filter__actions"><Button type="submit" variant={filterDirty ? "primary" : "secondary"}>{t("应用筛选")}</Button><Button variant="quiet" onClick={() => apply(INITIAL_FILTER)}>{t("重置")}</Button></div>
    </form>
    {filterDirty && <p className="muted">{t("筛选尚未应用；下方仍显示上一次筛选结果。")}</p>}
    {query.error && <InlineError message={(data ? t("刷新失败，保留下方上次结果：") : t("用量读取失败：")) + errorText(query.error)} onRetry={() => void query.refetch()} />}
    {data?.sync.warnings.length ? <InlineError message={t("本地会话同步提醒：") + data.sync.warnings.join("；")} /> : null}
    {query.isPending && <LoadingState label={t("正在读取用量和请求日志…")} />}
    {data && <>
      <div className="metric-strip" aria-label={t("筛选范围汇总")}>
        <div className="metric-strip__item"><span className="metric-strip__label">{t("请求 / 成功率")}</span><strong className="metric-strip__value">{data.summary.totalRequests.toLocaleString(localeTag())}</strong><small className="muted">{data.summary.totalRequests ? (data.summary.successCount / data.summary.totalRequests * 100).toFixed(1) + t("% 成功") : t("暂无请求")}</small></div>
        <div className="metric-strip__item"><span className="metric-strip__label">{t("输入 / 输出 token")}</span><strong className="metric-strip__value">{formatTokens(data.summary.totalInputTokens)} / {formatTokens(data.summary.totalOutputTokens)}</strong><small className="muted">{t("代理平均耗时")} {durationLabel(data.summary.avgLatencyMs)}</small></div>
        <div className="metric-strip__item"><span className="metric-strip__label">{t("缓存读取 / 写入 token")}</span><strong className="metric-strip__value">{formatTokens(data.summary.totalCacheReadTokens)} / {formatTokens(data.summary.totalCacheCreationTokens)}</strong><small className="muted">{t("按已记录用量统计")}</small></div>
        <div className="metric-strip__item"><span className="metric-strip__label">{t("记录成本（USD）")}</span><strong className="metric-strip__value">{formatUsd(data.summary.totalCostUsd)}</strong><small className="muted">{data.summary.unknownCostRequests ? t("{value0} 条费用未知；", { value0: data.summary.unknownCostRequests }) : ""}{t("不代表上游最终账单")}</small></div>
      </div>
      <UsageTrend points={data.trends} />
    </>}
    <div className="toolbar"><Tabs id="usage-data" label={t("用量视图")} value={tab} onChange={setTab} items={[{ value: "logs", label: t("请求日志") }, { value: "providers", label: t("供应商统计") }, { value: "models", label: t("模型统计") }]} /><label className="usage-refresh">{t("自动刷新")}<Select aria-label={t("自动刷新间隔")} value={interval} onChange={event => setIntervalSeconds(Number(event.target.value))}><option value={0}>{t("关闭")}</option><option value={15}>{t("15 秒")}</option><option value={30}>{t("30 秒")}</option><option value={60}>{t("60 秒")}</option></Select></label></div>
    <div role="tabpanel" id={"usage-data-panel-" + tab} aria-labelledby={"usage-data-tab-" + tab} className="section" aria-busy={query.isFetching}>
      {data && tab === "logs" && <>
        <DataTable caption={t("请求日志")} rows={data.logs} rowKey={row => row.requestId} emptyTitle={t("当前筛选范围没有请求")} columns={[
          { key: "time", label: t("时间 / 客户端"), render: row => <><time dateTime={new Date(row.createdAt).toISOString()}>{new Date(row.createdAt).toLocaleString(localeTag())}</time><small className="usage-cell-note">{clients.find(([id]) => id === row.appType)?.[1] ?? row.appType}</small></> },
          { key: "model", label: t("供应商 / 模型"), render: row => <><strong>{names.get(row.providerId) ?? row.providerId}</strong><code className="usage-cell-note">{row.model || t("未记录")}</code></> },
          { key: "response-model", label: t("响应模型探测"), render: row => <><ResponseModelEvidence audit={row.audit} local={isSessionRow(row)} compact /><Button size="sm" variant="quiet" onClick={() => setSelected(row)} aria-label={t("探测请求 ") + row.requestId + t(" 的响应模型")}>{t("探测详情")}</Button></> },
          { key: "tokens", label: t("输入 / 输出"), numeric: true, render: row => <>{formatTokens(row.inputTokens)} / {formatTokens(row.outputTokens)}<small className="usage-cell-note">{t("缓存")} {formatTokens(row.cacheReadTokens)} / {formatTokens(row.cacheCreationTokens)}</small></> },
          { key: "cost", label: t("成本 / 耗时"), numeric: true, render: row => <>{formatUsd(row.totalCostUsd)}<small className="usage-cell-note">{isSessionRow(row) ? t("本地会话 · 耗时未知") : durationLabel(row.latencyMs)}</small></> },
          { key: "status", label: t("状态"), render: row => <Badge tone={row.success !== false && row.statusCode >= 200 && row.statusCode < 400 ? "success" : "danger"}>{statusLabel(row)}{row.isStreaming ? t(" · 流式") : ""}</Badge> },
          { key: "details", label: t("详情"), render: row => <Button size="sm" variant="quiet" onClick={() => setSelected(row)} aria-label={t("查看请求 ") + row.requestId}>{t("查看")}</Button> },
        ]} />
        <p className="usage-footnote">{t("每条响应独立判断。上游声明与真实响应模型分开展示；探测详情只分析本条已保存证据，不发起新请求，不套用其他记录的结果。")}</p>
        <div className="usage-pagination"><span className="muted">{t("共")} {data.summary.totalRequests.toLocaleString(localeTag())} {t("条 · 每页")} {PAGE_SIZE} {t("条")}</span><div className="inline-actions"><Button variant="secondary" size="sm" disabled={page === 0 || query.isFetching} onClick={() => setPage(p => p - 1)} icon={<ChevronLeft size={15} />}>{t("上一页")}</Button><span>{page + 1} / {lastPage + 1}</span><Button variant="secondary" size="sm" disabled={page >= lastPage || query.isFetching} onClick={() => setPage(p => p + 1)} icon={<ChevronRight size={15} />}>{t("下一页")}</Button><form className="inline-actions" onSubmit={event => { event.preventDefault(); const next = Number(pageInput); if (Number.isInteger(next) && next >= 1 && next <= lastPage + 1) setPage(next - 1); }}><Input aria-label={t("跳转页码")} type="number" min={1} max={lastPage + 1} value={pageInput} onChange={event => setPageInput(event.target.value)} /><Button type="submit" size="sm" variant="quiet" disabled={query.isFetching}>{t("跳转")}</Button></form></div></div>
      </>}
      {data && tab === "providers" && <DataTable caption={t("供应商用量统计")} rows={data.providerStats} rowKey={row => row.providerId} columns={[
        { key: "provider", label: t("供应商"), render: row => names.get(row.providerId) ?? row.providerId },
        { key: "requests", label: t("请求数"), numeric: true, render: row => row.totalRequests.toLocaleString(localeTag()) },
        { key: "success", label: t("成功率"), numeric: true, render: row => row.successRate.toFixed(1) + "%" },
        { key: "tokens", label: t("输入 / 输出 token"), numeric: true, render: row => formatTokens(row.inputTokens) + " / " + formatTokens(row.outputTokens) },
        { key: "cost", label: t("成本"), numeric: true, render: row => formatUsd(row.totalCostUsd) },
        { key: "latency", label: t("平均耗时"), numeric: true, render: row => durationLabel(row.avgLatencyMs) },
      ]} />}
      {data && tab === "models" && <DataTable caption={t("模型用量统计")} rows={data.modelStats} rowKey={row => JSON.stringify([row.providerId, row.model])} columns={[
        { key: "model", label: t("模型"), render: row => <code>{row.model}</code> },
        { key: "provider", label: t("供应商"), render: row => names.get(row.providerId) ?? row.providerId },
        { key: "requests", label: t("请求数"), numeric: true, render: row => row.totalRequests.toLocaleString(localeTag()) },
        { key: "tokens", label: t("输入 / 输出 token"), numeric: true, render: row => formatTokens(row.inputTokens) + " / " + formatTokens(row.outputTokens) },
        { key: "cost", label: t("成本"), numeric: true, render: row => formatUsd(row.totalCostUsd) },
      ]} />}
    </div>
    <p className="usage-footnote">{t("数据来源包括本地路由，以及只读导入的 Codex、DeepSeek Harness 会话。Harness 支持 v4 JSONL / Zstandard 日志；输入 token 含缓存读取与写入，继承历史不重复计入。会话记录不包含真实 HTTP 状态、时延或账单，未提供的费用显示为未知。今天 / 昨天按系统本地日界线划分。")}</p>
    {selected && <Modal open title={t("请求详情 · 真实响应模型探测")} description={selected.requestId} size="lg" onClose={() => setSelected(null)}><div className="form-stack"><dl className="key-values"><dt>{t("供应商 / 客户端")}</dt><dd>{names.get(selected.providerId) ?? selected.providerId} / {selected.appType}</dd><dt>{isSessionRow(selected) ? t("会话状态 / 耗时") : t("HTTP / 耗时")}</dt><dd>{statusLabel(selected)} / {isSessionRow(selected) ? t("未知") : durationLabel(selected.latencyMs)}</dd><dt>{t("总 token / 推理 token")}</dt><dd>{formatTokens(selected.totalTokens ?? selected.inputTokens + selected.outputTokens)} / {selected.reasoningTokens == null ? "—" : formatTokens(selected.reasoningTokens)}</dd><dt>{t("成本倍率")}</dt><dd>{selected.costMultiplier ? selected.costMultiplier + "×" : "—"}</dd><dt>{t("输入 / 输出费用")}</dt><dd>{formatUsd(selected.inputCostUsd)} / {formatUsd(selected.outputCostUsd)}</dd><dt>{t("缓存读取 / 写入费用")}</dt><dd>{formatUsd(selected.cacheReadCostUsd)} / {formatUsd(selected.cacheCreationCostUsd)}</dd><dt>{t("总费用")}</dt><dd>{formatUsd(selected.totalCostUsd)}</dd></dl>{selected.errorMessage && <InlineError message={selected.errorMessage} />}<ResponseModelEvidence key={"identity-" + selected.requestId} audit={selected.audit} local={isSessionRow(selected)} />{!isSessionRow(selected) && <ModelAuditCell key={selected.requestId} audit={selected.audit} legacyModel={selected.model} />}</div></Modal>}
  </section>;
}

function UsageTrend({ points }: { points: UsageTrendPoint[] }) {
  useLanguage();
  const [metric, setMetric] = useState("requests");
  const value = (point: UsageTrendPoint) => metric === "cost" ? (point.cost ?? 0) : metric === "tokens" ? point.inputTokens + point.outputTokens + point.cacheReadTokens + point.cacheCreationTokens : point.requests;
  const maximum = Math.max(1, ...points.map(value));
  return <section className="usage-trend section" aria-labelledby="usage-trend-title"><div className="section-heading"><h2 id="usage-trend-title">{t("请求趋势")}</h2><Select aria-label={t("趋势指标")} value={metric} onChange={event => setMetric(event.target.value)}><option value="requests">{t("请求数")}</option><option value="tokens">{t("token 用量")}</option><option value="cost">{t("成本（USD）")}</option></Select></div>{points.length ? <><ol className="usage-trend__plot" aria-label={t("趋势柱状图")}>{points.map(point => <li key={point.timestamp} title={point.date + " · " + (metric === "cost" ? formatUsd(point.cost) : value(point).toLocaleString(localeTag()))}><span className="usage-trend__bar-slot"><span className="usage-trend__bar" style={{ height: (metric === "cost" && point.cost === null ? 0 : Math.max(2, value(point) / maximum * 100)) + "%" }} /></span><small>{point.label}</small><span className="sr-only">{value(point)}</span></li>)}</ol><details className="usage-trend__data"><summary>{t("查看趋势数据")}</summary><DataTable caption={t("趋势原始数据")} rows={points} rowKey={row => String(row.timestamp)} columns={[{ key: "date", label: t("时间"), render: row => row.date + " " + row.label }, { key: "requests", label: t("请求数"), numeric: true, render: row => row.requests }, { key: "tokens", label: t("输入 / 输出"), numeric: true, render: row => formatTokens(row.inputTokens) + " / " + formatTokens(row.outputTokens) }, { key: "cost", label: t("成本"), numeric: true, render: row => formatUsd(row.cost) }]} /></details></> : <EmptyState title={t("暂无趋势数据")} description={t("本地路由产生请求后，在这里查看变化。")} />}</section>;
}
