import { localeTag, useLanguage } from "../../i18n/language";
import { t } from "../../i18n/language";
import { useEffect, useId, useMemo, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { ChevronLeft, ChevronRight, RefreshCw } from "lucide-react";
import { Button, Field, Input, InlineError, LoadingState, Select, Tabs } from "../../ui";
import { getModelStats, getProviderStats, getUsageLogs, getUsageSummary, getUsageTrends, syncLocalUsage } from "../../lib/usage-service";
import { errorText } from "../../lib/error";
import type { Provider } from "../../types/provider";
import type { DateRange, UsageFilter, UsageLogEntry } from "../../types/usage";
import { ModelPricingModal } from "./ModelPricingModal";
import { UsageRequestWorkspace } from "./UsageRequestWorkspace";
import { UsageTrend } from "./UsageTrend";
import { UsageContributions } from "./UsageContributions";
import { UsageLedger } from "./UsageLedger";
import { UsageScope } from "./UsageScope";
export { UsageRequestDetails } from "./UsageRequestDetails";
import { AuditToolsPanel, ModelAuditPanel } from "./ModelAuditPanel";
import { ResponseModelEvidence } from "./ResponseModelEvidence";
import { lastUsagePage, normalizeUsageFilter } from "./usage-format";
import "./usage.css";

const PAGE_SIZE = 20;
const INITIAL_FILTER: UsageFilter = { dateRange: "today" };
const clients = [["claude", "Claude Code"], ["codex", "Codex"], ["deepseek_harness", "DeepSeek Harness"], ["gemini", "Gemini CLI"], ["grok", "Grok"], ["opencode", "OpenCode"], ["model_probe", "模型探测"]];



export type UsageView = "overview" | "logs" | "audit" | "configuration";

export function UsagePage({ providers, initialView = "overview" }: { providers: Provider[]; initialView?: UsageView }) {
  useLanguage();
  const [draft, setDraft] = useState<UsageFilter>(INITIAL_FILTER);
  const [filter, setFilter] = useState<UsageFilter>(INITIAL_FILTER);
  const [page, setPage] = useState(0);
  const [pageInput, setPageInput] = useState("1");
  const [interval, setIntervalSeconds] = useState(30);
  const [tab, setTab] = useState<UsageView>(initialView);
  const [configurationVisited, setConfigurationVisited] = useState(initialView === "configuration");
  const [configurationBusy, setConfigurationBusy] = useState(false);
  const [pricingOpen, setPricingOpen] = useState(false);
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
    enabled: tab !== "configuration",
    refetchInterval: interval > 0 ? interval * 1000 : false,
    refetchOnWindowFocus: false,
  });
  const data = query.data;
  const lastPage = lastUsagePage(data?.summary.totalRequests ?? 0, PAGE_SIZE);
  useEffect(() => { if (data && page > lastPage) setPage(lastPage); }, [data, page, lastPage]);
  useEffect(() => { setPageInput(String(page + 1)); }, [page]);
  const names = new Map<string, string>([["", t("未选择供应商")], ["local:codex", t("Codex · 本地用量")], ["local:deepseek_harness", t("DeepSeek Harness · 本地用量")], ...providers.map(provider => [provider.id, provider.name] as [string, string])]);
  const providerIds = [...new Set([...providers.map(p => p.id), ...(data?.providerStats.map(p => p.providerId) ?? []), draft.providerId].filter((id): id is string => Boolean(id)))];
  const models = useMemo(() => [...new Set([...(data?.modelStats.map(item => item.model) ?? []), ...(data?.logs.map(item => item.model) ?? [])])].sort(), [data]);
  const patch = (value: Partial<UsageFilter>) => setDraft(previous => ({ ...previous, ...value }));
  const apply = (value: UsageFilter) => { setDraft(value); setFilter(normalizeUsageFilter(value)); setPage(0); setSelected(null); };
  const changeDateRange = (dateRange: DateRange) => {
    setDraft(previous => ({ ...previous, dateRange, startTimeMs: undefined }));
    setFilter(previous => normalizeUsageFilter({ ...previous, dateRange, startTimeMs: undefined }));
    setPage(0);
    setSelected(null);
  };
  const inspectContribution = (scope: Partial<UsageFilter>) => { apply({ ...filter, ...scope }); setTab("logs"); };
  const filterDirty = JSON.stringify(normalizeUsageFilter(draft)) !== JSON.stringify(filter);
  const additionalFilters = [draft.appType, draft.providerId, draft.model, draft.status].filter(value => value?.trim()).length;

  return <section className="workspace-page usage-page" data-inspecting={tab === "logs" && Boolean(selected)} aria-labelledby="usage-title">
    <header className="page-heading"><div className="page-heading__copy"><h1 id="usage-title">{t("用量与审计")}</h1><p className="page-description">{t("查看本地路由与 Codex / DeepSeek Harness 会话的用量及可观测证据。")}</p></div><div className="page-actions"><Button variant="secondary" disabled={configurationBusy} onClick={() => setPricingOpen(true)}>{t("模型定价")}</Button>{tab !== "configuration" && <Button variant="secondary" icon={<RefreshCw size={15} />} loading={query.isFetching} onClick={() => void query.refetch()}>{t("刷新")}</Button>}</div></header>
    <Tabs id="usage-data" label={t("用量视图")} value={tab} onChange={value => { if (configurationBusy) return; if (value === "configuration") setConfigurationVisited(true); setTab(value as UsageView); setSelected(null); }} items={[{ value: "overview", label: t("概览") }, { value: "logs", label: t("请求记录") }, { value: "audit", label: t("审计分析") }, { value: "configuration", label: t("探测配置") }].map(item => ({ ...item, disabled: configurationBusy && item.value !== tab }))} />
    {tab !== "configuration" && <div className="usage-controls">
      <form className="usage-filter" onSubmit={event => { event.preventDefault(); apply(draft); }}>
        <div className="usage-filter__primary">
          <div className="usage-date-shortcuts" role="group" aria-label={t("日期快捷范围")}>
            {([["today", "今天"], ["7d", "近 7 天"], ["30d", "近 30 天"]] as const).map(([value, label]) => <Button key={value} size="sm" variant="quiet" aria-pressed={filter.dateRange === value && filter.startTimeMs == null} onClick={() => changeDateRange(value)}>{t(label)}</Button>)}
          </div>
          <div className="usage-filter__actions">{filterDirty && <Button type="submit" size="sm">{t("应用筛选")}</Button>}<Button size="sm" variant="quiet" onClick={() => apply(INITIAL_FILTER)}>{t("重置")}</Button></div>
        </div>
        <details className="usage-filter__advanced"><summary>{t("更多筛选")}{additionalFilters > 0 && <span className="usage-filter__count">{t("{count} 项", { count: additionalFilters })}</span>}</summary>
        <div className="usage-filter__fields">
      <Field label={t("日期范围")}><Select value={draft.dateRange ?? ""} onChange={event => patch({ dateRange: (event.target.value || undefined) as DateRange | undefined, startTimeMs: undefined })}><option value="today">{t("今天")}</option><option value="yesterday">{t("昨天")}</option><option value="1d">{t("近 24 小时")}</option><option value="7d">{t("近 7 天")}</option><option value="14d">{t("近 14 天")}</option><option value="30d">{t("近 30 天")}</option><option value="">{t("全部时间")}</option></Select></Field>
      <Field label={t("客户端")}><Select value={draft.appType ?? ""} onChange={event => patch({ appType: event.target.value })}><option value="">{t("全部客户端")}</option>{clients.map(([id,label]) => <option key={id} value={id}>{t(label)}</option>)}</Select></Field>
      <Field label={t("供应商")}><Select value={draft.providerId ?? ""} onChange={event => patch({ providerId: event.target.value })}><option value="">{t("全部供应商")}</option>{providerIds.map(id => <option key={id} value={id}>{names.get(id) ?? id}</option>)}</Select></Field>
      <Field label={t("模型 ID")}><Input list={modelList} value={draft.model ?? ""} placeholder={t("全部模型")} onChange={event => patch({ model: event.target.value })} /></Field>
      <datalist id={modelList}>{models.map(model => <option key={model} value={model} />)}</datalist>
      <Field label={t("请求状态")}><Select value={draft.status ?? ""} onChange={event => patch({ status: event.target.value })}><option value="">{t("全部状态")}</option><option value="success">{t("成功")}</option><option value="error">{t("失败")}</option></Select></Field>
        </div>
        <div className="usage-filter__footer">
          <label className="usage-refresh">{t("自动刷新")}<Select aria-label={t("自动刷新间隔")} value={interval} onChange={event => setIntervalSeconds(Number(event.target.value))}><option value={0}>{t("关闭")}</option><option value={15}>{t("15 秒")}</option><option value={30}>{t("30 秒")}</option><option value={60}>{t("60 秒")}</option></Select></label>
        </div>
        </details>
      </form>
    {filterDirty && <p className="usage-filter__pending" role="status">{t("筛选尚未应用；下方仍显示上一次筛选结果。")}</p>}
    <UsageScope filter={filter} providerNames={names} />
    {query.error && <InlineError message={(data ? t("刷新失败，保留下方上次结果：") : t("用量读取失败：")) + errorText(query.error)} onRetry={() => void query.refetch()} />}
    {data?.sync.warnings.length ? <InlineError message={t("本地会话同步提醒：") + data.sync.warnings.join("；")} /> : null}
    {query.isPending && <LoadingState label={t("正在读取用量和请求日志…")} />}
    </div>}
    {tab === "configuration" && <UsageScope filter={filter} providerNames={names} />}
    {tab !== "configuration" && <div role="tabpanel" id={"usage-data-panel-" + tab} aria-labelledby={"usage-data-tab-" + tab} className="usage-view" aria-busy={query.isFetching}>
      {tab === "overview" && data && <>
        <UsageLedger summary={data.summary} onInspect={status => inspectContribution(status ? { status } : {})} />
        <UsageTrend points={data.trends} dateRange={filter.dateRange} />
        <UsageContributions providerStats={data.providerStats} modelStats={data.modelStats} providerNames={names} onInspect={inspectContribution} />
      </>}
      {data && tab === "logs" && <UsageRequestWorkspace rows={data.logs} providerNames={names} selected={selected} onSelect={setSelected} />}

      {tab === "audit" && <ModelAuditPanel filter={filter} logs={data?.logs ?? []} providerNames={names} />}
      {data && (tab === "logs" || tab === "audit") && (
        <div className="usage-pagination"><span className="muted">{t("共")} {data.summary.totalRequests.toLocaleString(localeTag())} {t("条 · 每页")} {PAGE_SIZE} {t("条")}</span><div className="inline-actions"><Button variant="secondary" size="sm" disabled={page === 0 || query.isFetching} onClick={() => setPage(p => p - 1)} icon={<ChevronLeft size={15} />}>{t("上一页")}</Button><span>{page + 1} / {lastPage + 1}</span><Button variant="secondary" size="sm" disabled={page >= lastPage || query.isFetching} onClick={() => setPage(p => p + 1)} icon={<ChevronRight size={15} />}>{t("下一页")}</Button><form className="inline-actions" onSubmit={event => { event.preventDefault(); const next = Number(pageInput); if (Number.isInteger(next) && next >= 1 && next <= lastPage + 1) setPage(next - 1); }}><Input aria-label={t("跳转页码")} type="number" min={1} max={lastPage + 1} value={pageInput} onChange={event => setPageInput(event.target.value)} /><Button type="submit" size="sm" variant="quiet" disabled={query.isFetching}>{t("跳转")}</Button></form></div></div>
      )}
    </div>}
    {configurationVisited && <div role="tabpanel" id="usage-data-panel-configuration" aria-labelledby="usage-data-tab-configuration" className="usage-view" hidden={tab !== "configuration"}>
      <AuditToolsPanel filter={filter} logs={data?.logs ?? []} onBusyChange={setConfigurationBusy} />
    </div>}
    {tab !== "configuration" && <details className="workspace-disclosure"><summary>{t("数据来源与统计说明")}</summary><div className="workspace-disclosure__body">
    <p className="usage-footnote">{t("数据来源包括本地路由，以及只读导入的 Codex、DeepSeek Harness 会话。Harness 支持 v4 JSONL / Zstandard 日志；输入 token 含缓存读取与写入，继承历史不重复计入。会话记录不包含真实 HTTP 状态、时延或账单，未提供的费用显示为未知。今天 / 昨天按系统本地日界线划分。")}</p>
    </div></details>}
    {pricingOpen && <ModelPricingModal onClose={() => setPricingOpen(false)} />}
  </section>;
}
