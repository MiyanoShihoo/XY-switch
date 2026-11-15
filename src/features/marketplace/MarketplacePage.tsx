import { useEffect, useRef, useState } from "react";
import { Search } from "lucide-react";
import { useLanguage, t, translateMessage } from "../../i18n/language";
import { Badge, EmptyState, Field, Button, InlineError, Modal, Select, Input, Tabs, Textarea } from "../../ui";
import { usePageOperation } from "../shared/usePageOperation";
import { MarketplaceClient, MarketplaceServiceError, canonicalMarketplacePackage, marketplaceErrorText, normalizeMarketplaceOrigin, parseMarketplacePackage } from "../../lib/marketplace-service";
import { readPluginDocument } from "../../lib/plugins-service";
import { MarketplaceCatalog } from "./MarketplaceCatalog";
import "./marketplace.css";
import { pluginClientLabels as clientNames } from "../plugins/clients";
import { pluginClients, type PluginClient, type PluginKind, type PluginPermissions } from "../../types/plugins";
import { submissionStatuses, type MarketplaceAuditEvent, type MarketplaceCatalogEntry, type MarketplaceInstallPlan, type MarketplacePageResult, type MarketplaceReport, type MarketplaceSession, type MarketplaceSubmission, type MarketplaceSubmissionSummary, type ReviewDecision, type SubmissionStatus, type WithdrawalReason } from "../../types/marketplace";

const statusNames: Record<SubmissionStatus, string> = { submitted: "待审核", changes_requested: "需要修订", rejected: "已拒绝", approved: "已批准，待发布", published: "已发布", withdrawn: "已撤回" };
const withdrawalNames: Record<WithdrawalReason, string> = { security: "安全风险", privacy: "隐私问题", withdrawn: "作者撤回" };
const reportNames: Record<MarketplaceReport["reason"], string> = { security: "安全风险", privacy: "隐私问题", malware: "恶意内容", copyright: "版权问题", other: "其他问题" };
const emptyPage = <Item,>(): MarketplacePageResult<Item> => ({ items: [], limit: 20, offset: 0, hasMore: false });
type MarketplaceArea = "explore" | "submissions" | "review";
type MarketplacePanel = "source" | "account" | "submit";

function MarketplacePermissions({ value }: { value: PluginPermissions }) {
  return <dl className="key-values" aria-label={t("完整权限")}>
    <dt>{t("网络站点")}</dt><dd>{value.network.join(" · ") || t("无")}</dd>
    <dt>{t("读取路径")}</dt><dd>{value.readPaths.join(" · ") || t("无")}</dd>
    <dt>{t("写入路径")}</dt><dd>{value.writePaths.join(" · ") || t("无")}</dd>
    <dt>{t("运行进程")}</dt><dd>{t(value.process ? "申请访问" : "无")}</dd>
    <dt>{t("凭据名称")}</dt><dd>{value.credentials.join(" · ") || t("无")}</dd>
  </dl>;
}

function Pagination({ page, busy, load }: { page: MarketplacePageResult<unknown>; busy: boolean; load: (offset: number) => void }) {
  return <nav className="marketplace-pagination" aria-label={t("市场分页")}>
    <Button variant="secondary" size="sm" disabled={busy || page.offset === 0} onClick={() => load(Math.max(0, page.offset - page.limit))}>{t("上一页")}</Button>
    <span>{t("起始位置")} {page.offset + 1}</span>
    <Button variant="secondary" size="sm" disabled={busy || !page.hasMore} onClick={() => load(page.offset + page.limit)}>{t("下一页")}</Button>
  </nav>;
}

function initialOrigin(): string {
  const configured: unknown = import.meta.env.VITE_XY_MARKETPLACE_ORIGIN;
  if (typeof configured !== "string" || !configured) return "";
  try { return normalizeMarketplaceOrigin(configured, import.meta.env.DEV); } catch { return ""; }
}

export function MarketplacePage() {
  useLanguage();
  const operation = usePageOperation();
  const [area, setArea] = useState<MarketplaceArea>("explore");
  const [reviewArea, setReviewArea] = useState("queue");
  const [panel, setPanel] = useState<MarketplacePanel | null>(null);
  const [accountNext, setAccountNext] = useState<"submit" | null>(null);
  const [selectedEntry, setSelectedEntry] = useState<MarketplaceCatalogEntry | null>(null);
  const [origin, setOrigin] = useState(initialOrigin);
  const [address, setAddress] = useState(initialOrigin);
  const [connectionSerial, setConnectionSerial] = useState(0);
  const [connecting, setConnecting] = useState(false);
  const clientRef = useRef<MarketplaceClient | null>(null);
  const [session, setSession] = useState<MarketplaceSession | null>(null);
  const [authMode, setAuthMode] = useState<"login" | "register">("login");
  const [username, setUsername] = useState("");
  const [displayName, setDisplayName] = useState("");
  const [password, setPassword] = useState("");
  const [catalog, setCatalog] = useState(emptyPage<MarketplaceCatalogEntry>);
  const [search, setSearch] = useState("");
  const [kind, setKind] = useState<PluginKind | "">("");
  const [clientFilter, setClientFilter] = useState<PluginClient | "">("");
  const [filtersExpanded, setFiltersExpanded] = useState(false);
  const [appliedFilters, setAppliedFilters] = useState<{ search: string; kind: PluginKind | ""; client: PluginClient | "" }>({ search: "", kind: "", client: "" });
  const [versionPage, setVersionPage] = useState<MarketplacePageResult<MarketplaceCatalogEntry> | null>(null);
  const [versionPlugin, setVersionPlugin] = useState("");
  const [own, setOwn] = useState(emptyPage<MarketplaceSubmissionSummary>);
  const [queue, setQueue] = useState(emptyPage<MarketplaceSubmissionSummary>);
  const [queueStatus, setQueueStatus] = useState<SubmissionStatus>("submitted");
  const [source, setSource] = useState("");
  const [replacing, setReplacing] = useState<{ id: string; version: string } | null>(null);
  const [detail, setDetail] = useState<MarketplaceSubmission | null>(null);
  const [detailScope, setDetailScope] = useState<"own" | "review">("own");
  const [audit, setAudit] = useState<MarketplaceAuditEvent[]>([]);
  const [reviewNote, setReviewNote] = useState("");
  const [reviewConfirmed, setReviewConfirmed] = useState(false);
  const [withdrawReason, setWithdrawReason] = useState<WithdrawalReason>("withdrawn");
  const [plan, setPlan] = useState<MarketplaceInstallPlan | null>(null);
  const [installClients, setInstallClients] = useState<PluginClient[]>([]);
  const [installConfirmed, setInstallConfirmed] = useState(false);
  const [enableInstall, setEnableInstall] = useState(false);
  const [reportTarget, setReportTarget] = useState<MarketplaceCatalogEntry | null>(null);
  const [reportReason, setReportReason] = useState<MarketplaceReport["reason"]>("security");
  const [reportNote, setReportNote] = useState("");
  const [reports, setReports] = useState(emptyPage<MarketplaceReport>);
  const [installExpired, setInstallExpired] = useState(false);
  useEffect(() => {
    const remaining = plan ? plan.preview.expiresAt - Date.now() : 0;
    setInstallExpired(!!plan && remaining <= 0);
    if (!plan || remaining <= 0) return;
    const timer = setTimeout(() => setInstallExpired(true), remaining + 1);
    return () => clearTimeout(timer);
  }, [plan]);
  const busy = operation.busy || connecting;
  const activeFilterLabels = [appliedFilters.kind && (appliedFilters.kind === "mcp" ? "MCP" : t(appliedFilters.kind === "skill" ? "技能" : "模型映射")), appliedFilters.client && clientNames[appliedFilters.client]].filter(Boolean);

  function activeClient(): MarketplaceClient {
    if (!clientRef.current) throw new MarketplaceServiceError("session_changed");
    return clientRef.current;
  }
  function current(active: MarketplaceClient): void {
    if (clientRef.current !== active) throw new MarketplaceServiceError("session_changed");
  }
  function run(work: () => Promise<void>, success = ""): void {
    void operation.run(async () => { try { await work(); } catch (error) { throw new Error(marketplaceErrorText(error)); } }, success);
  }

  useEffect(() => {
    if (!origin) return;
    const active = new MarketplaceClient(origin, { allowDevelopmentHttp: import.meta.env.DEV });
    clientRef.current = active;
    setSession(null); setCatalog(emptyPage()); setVersionPage(null); setSelectedEntry(null); setArea("explore"); setConnecting(true);
    setAppliedFilters({ search: "", kind: "", client: "" });
    const unsubscribe = active.subscribe(identity => {
      setSession(identity);
      if (!identity) {
        setOwn(emptyPage()); setQueue(emptyPage()); setReports(emptyPage()); setDetail(null); setAudit([]);
        setSource(""); setReplacing(null); setReviewNote(""); setReportTarget(null); setReportNote("");
        setPassword(""); setPlan(null); setInstallConfirmed(false);
        setArea("explore"); setReviewConfirmed(false);
        setPanel(previous => previous === "submit" ? "account" : previous);
      }
    });
    void active.catalog().then(result => {
      if (clientRef.current === active) { setCatalog(result); setSelectedEntry(result.items[0] ?? null); setPanel(previous => previous === "source" ? null : previous); operation.setError(""); operation.setMessage("市场连接已验证。"); }
    }).catch(error => {
      if (clientRef.current === active) operation.setError(marketplaceErrorText(error));
    }).finally(() => { if (clientRef.current === active) setConnecting(false); });
    return () => { unsubscribe(); if (clientRef.current === active) clientRef.current = null; active.dispose(); };
  }, [origin, connectionSerial]);

  async function loadCatalog(offset = 0, filters = appliedFilters): Promise<void> {
    const active = activeClient();
    const result = await active.catalog({ offset, q: filters.search || undefined, kind: filters.kind || undefined, client: filters.client || undefined });
    current(active); setCatalog(result); setAppliedFilters(filters);
    setSelectedEntry(previous => result.items.find(entry => entry.id === previous?.id) ?? result.items[0] ?? null);
  }
  async function loadOwn(offset = 0): Promise<void> {
    const active = activeClient(); const result = await active.submissions({ offset }); current(active); setOwn(result);
  }
  async function loadQueue(offset = 0): Promise<void> {
    const active = activeClient(); const result = await active.reviewQueue({ offset, status: queueStatus }); current(active); setQueue(result);
  }
  async function authenticate(): Promise<void> {
    const active = activeClient(); const suppliedPassword = password; setPassword("");
    const identity = authMode === "register" ? await active.register(username, displayName, suppliedPassword) : await active.login(username, suppliedPassword);
    current(active); setSession(identity);
    await loadOwn();
    if (identity.user.reviewer) { await loadQueue(); const result = await active.reports(); current(active); setReports(result); }
    setPanel(accountNext === "submit" ? "submit" : null); setAccountNext(null);
  }
  async function openSubmission(id: string, scope: "own" | "review"): Promise<void> {
    const active = activeClient(); const [submission, events] = await Promise.all([active.submission(id), active.submissionAudit(id)]);
    current(active); setDetail(submission); setAudit(events.items); setDetailScope(scope); setReviewNote(""); setReviewConfirmed(false); setWithdrawReason("withdrawn");
  }
  async function updateSubmission(row: MarketplaceSubmissionSummary): Promise<void> {
    const active = activeClient(); const submission = await active.submission(row.submissionId);
    current(active); setSource(JSON.stringify(submission.package, null, 2)); setReplacing({ id: row.submissionId, version: row.version }); setPanel("submit");
  }
  async function decide(decision: ReviewDecision): Promise<void> {
    if (!detail || decision === "approve" && !reviewConfirmed) throw new MarketplaceServiceError("confirmation");
    const active = activeClient(); const result = await active.decide(detail.submissionId, { version: detail.package.manifest.version, digest: detail.digest, decision, note: reviewNote });
    current(active); setDetail(result); setReviewConfirmed(false); await loadQueue(); await loadOwn();
  }
  async function publish(): Promise<void> {
    if (!detail) throw new MarketplaceServiceError("invalid_request");
    const active = activeClient(); await active.publish(detail.submissionId, { version: detail.package.manifest.version, digest: detail.digest });
    current(active); await openSubmission(detail.submissionId, detailScope); await loadCatalog(); await loadOwn(); if (session?.user.reviewer) await loadQueue();
  }
  async function withdraw(): Promise<void> {
    if (!detail) throw new MarketplaceServiceError("invalid_request");
    const active = activeClient(); const result = await active.withdraw(detail.submissionId, { version: detail.package.manifest.version, digest: detail.digest, reason: withdrawReason, note: reviewNote });
    current(active); setDetail(result); await loadCatalog(); await loadOwn(); if (session?.user.reviewer) await loadQueue();
  }
  async function saveDownload(row: MarketplaceCatalogEntry): Promise<void> {
    const active = activeClient(); const release = await active.download(row.id, row.version); current(active);
    const url = URL.createObjectURL(new Blob([JSON.stringify(release, null, 2)], { type: "application/json" }));
    const anchor = document.createElement("a"); anchor.href = url; anchor.download = `${row.id}-${row.version}.json`; anchor.click();
    setTimeout(() => URL.revokeObjectURL(url), 1000);
  }
  async function previewInstall(row: MarketplaceCatalogEntry): Promise<void> {
    const active = activeClient(); const result = await active.prepareInstall(row.id, row.version);
    current(active); setVersionPage(null); setPlan(result); setInstallClients([]); setInstallConfirmed(false); setEnableInstall(false);
  }

  function openPanel(next: MarketplacePanel): void {
    operation.setError(""); operation.setMessage(""); setPanel(next);
  }
  function closePanel(): void {
    if (panel === "account") { setPassword(""); setAccountNext(null); }
    setPanel(null);
  }
  function startSubmission(): void {
    if (session) openPanel("submit");
    else { setAccountNext("submit"); openPanel("account"); }
  }
  function catalogRow(row: MarketplaceCatalogEntry) {
    return <article key={row.id + "-" + row.version} className="marketplace-catalog-row marketplace-discovery-row" role="listitem">
      <div className="marketplace-catalog-copy"><div className="marketplace-row-title"><h4><button type="button" className="marketplace-entry-title" aria-label={t("查看插件 {name}", { name: row.name })} disabled={busy} onClick={() => { operation.setError(""); operation.setMessage(""); setVersionPage(null); setSelectedEntry(row); }}>{row.name}</button></h4><code className="marketplace-version">{row.version}</code></div>
        <p className="marketplace-description">{row.description}</p>
        <p className="marketplace-meta">{row.authorDisplayName} · {row.clients.map(client => clientNames[client]).join(" · ")}</p></div>
    </article>;
  }
  const hasModal = panel !== null || versionPage !== null || plan !== null || reportTarget !== null || detail !== null;
  const feedback = <><InlineError message={translateMessage(operation.error)} />
    {operation.message && <p className="marketplace-notice" role="status">{translateMessage(operation.message)}</p>}</>;

  return <section className="workspace-page marketplace-page" aria-label={t("共享插件市场")}>
    <header className="page-heading"><div className="page-heading__copy"><h1>{t("共享插件市场")}</h1><p className="page-description">{t("按 MCP、技能和模型映射浏览公开发布的插件。")}</p></div>
      <div className="page-actions">
        {area !== "explore" && <Button variant="quiet" size="sm" disabled={busy} onClick={() => openPanel("source")}>{t("市场来源")}</Button>}
        <Button variant="secondary" size="sm" disabled={busy || !origin} onClick={() => { setAccountNext(null); openPanel("account"); }}>{t(session ? "市场账户" : "登录市场")}</Button>
        <Button variant="secondary" size="sm" disabled={busy || !origin} onClick={startSubmission}>{t("投稿插件")}</Button>
      </div>
    </header>
    {!hasModal && feedback}
    {session && <Tabs id="marketplace-areas" label={t("市场工作区")} value={area} onChange={value => setArea(value as MarketplaceArea)} items={[
      { value: "explore", label: t("探索插件") },
      ...(session ? [{ value: "submissions", label: t("我的投稿") }] : []),
      ...(session?.user.reviewer ? [{ value: "review", label: t("审核工作台") }] : []),
    ]} />}
    {area === "explore" && <section className="marketplace-discovery" id="marketplace-areas-panel-explore" role={session ? "tabpanel" : "region"} aria-labelledby={session ? "marketplace-areas-tab-explore" : undefined} aria-label={session ? undefined : t("探索插件")}>
      {!origin ? <EmptyState title={t("连接一个插件市场")} description={t("设置市场来源后，即可浏览和下载公开发布的插件。")}
        action={<Button onClick={() => openPanel("source")}>{t("设置市场来源")}</Button>} /> : <MarketplaceCatalog
        page={catalog} selected={selectedEntry} busy={busy} loading={connecting || busy && !catalog.items.length}
        hasError={!!operation.error} filtered={!!(appliedFilters.search || appliedFilters.kind || appliedFilters.client)} canReport={!!session}
        filters={<div className="marketplace-filter-rail">
        <form className="marketplace-search" aria-label={t("筛选插件")} onSubmit={event => { event.preventDefault(); run(async () => { await loadCatalog(0, { search, kind, client: clientFilter }); setFiltersExpanded(false); }); }}>
          <Field className="marketplace-search-query" label={t("搜索公开插件")}><Input placeholder={t("搜索插件名称或用途")} leading={<Search size={16} />} value={search} onChange={event => setSearch(event.currentTarget.value)} maxLength={160} disabled={busy} /></Field>
          <Button className="marketplace-filter-toggle" variant="quiet" size="sm" aria-expanded={filtersExpanded} aria-controls="marketplace-filter-options" disabled={busy} onClick={() => setFiltersExpanded(previous => !previous)}><span>{t("筛选插件")}</span>{activeFilterLabels.length > 0 && <span className="marketplace-active-filters">{activeFilterLabels.join(" · ")}</span>}</Button>
          <div className="marketplace-filter-options" id="marketplace-filter-options" data-expanded={filtersExpanded}>
            <Field label={t("插件类型")}><Select value={kind} disabled={busy} onChange={event => setKind(event.currentTarget.value as PluginKind | "")}><option value="">{t("全部类型")}</option><option value="mcp">MCP</option><option value="skill">{t("技能")}</option><option value="provider">{t("模型映射")}</option></Select></Field>
            <Field label={t("客户端")}><Select value={clientFilter} disabled={busy} onChange={event => setClientFilter(event.currentTarget.value as PluginClient | "")}><option value="">{t("全部客户端")}</option>{pluginClients.map(client => <option key={client} value={client}>{clientNames[client]}</option>)}</Select></Field>
          </div>
          <Button className="marketplace-search-submit" variant="secondary" type="submit" disabled={busy}>{t("应用筛选")}</Button>
          <details className="marketplace-source"><summary>{t("市场来源")}</summary><div><code>{origin}</code><Button variant="quiet" size="sm" disabled={busy} onClick={() => openPanel("source")}>{t("设置市场来源")}</Button></div></details>
        </form>
      </div>}
        onSelect={entry => { operation.setError(""); operation.setMessage(""); setVersionPage(null); setSelectedEntry(entry); }}
        onRefresh={() => run(() => loadCatalog(catalog.offset))} onLoad={offset => run(() => loadCatalog(offset))}
        onPreview={() => { if (selectedEntry) run(() => previewInstall(selectedEntry)); }}
        onVersions={() => { if (selectedEntry) run(async () => {
          const active = activeClient(); const result = await active.versions(selectedEntry.id); current(active); setVersionPlugin(selectedEntry.id); setVersionPage(result);
        }); }}
        onDownload={() => { if (selectedEntry) run(() => saveDownload(selectedEntry), "签名包已下载；安装时仍须通过本机公钥验证。"); }}
        onReport={() => { if (selectedEntry) { setReportTarget(selectedEntry); setReportNote(""); } }} />}
    </section>}
    {area === "submissions" && session && <section className="marketplace-area" id="marketplace-areas-panel-submissions" role="tabpanel" aria-labelledby="marketplace-areas-tab-submissions">
      <div className="marketplace-section-heading"><div><h2>{t("我的审核状态")}</h2><p className="marketplace-meta">{t("所有注册用户均可投稿。修改必须使用新版本号，并重新接受独立审核。")}</p></div>
        <Button variant="secondary" disabled={busy} onClick={() => run(() => loadOwn())}>{t("刷新投稿")}</Button></div>
      {!own.items.length ? <EmptyState title={t("当前账号尚无投稿。")} action={<Button onClick={startSubmission}>{t("投稿插件")}</Button>} /> : <div className="marketplace-list" role="list">{own.items.map(row => <article className="marketplace-catalog-row" role="listitem" key={row.submissionId}>
        <div className="marketplace-catalog-copy"><div className="marketplace-row-title"><h3>{row.name}</h3><Badge>{row.version}</Badge><Badge>{t(statusNames[row.status])}</Badge></div>
          {row.reviewNote && <p className="marketplace-description">{t("审核说明")} {row.reviewNote}</p>}</div>
        <div className="marketplace-row-actions"><Button variant="secondary" size="sm" disabled={busy} onClick={() => run(() => openSubmission(row.submissionId, "own"))}>{t("查看投稿")}</Button>
          <Button variant="quiet" size="sm" disabled={busy || !["changes_requested", "rejected", "published", "withdrawn"].includes(row.status)} onClick={() => run(() => updateSubmission(row))}>{t("提交新版本")}</Button></div>
      </article>)}</div>}
      {(own.offset > 0 || own.hasMore) && <Pagination page={own} busy={busy} load={offset => run(() => loadOwn(offset))} />}
    </section>}
    {area === "review" && session?.user.reviewer && <section className="marketplace-area" id="marketplace-areas-panel-review" role="tabpanel" aria-labelledby="marketplace-areas-tab-review">
      <Tabs id="marketplace-review" label={t("审核工作台")} value={reviewArea} onChange={setReviewArea} items={[{ value: "queue", label: t("独立审核队列") }, { value: "reports", label: t("举报处理") }]} />
      {reviewArea === "queue" ? <section className="marketplace-area" id="marketplace-review-panel-queue" role="tabpanel" aria-labelledby="marketplace-review-tab-queue">
        <p className="marketplace-meta">{t("作者不能自审。批准绑定所审阅全文的精确版本与摘要，批准后不能换包。")}</p>
        <div className="marketplace-toolbar"><Field label={t("审核状态筛选")}><Select value={queueStatus} disabled={busy} onChange={event => setQueueStatus(event.currentTarget.value as SubmissionStatus)}>{submissionStatuses.map(status => <option key={status} value={status}>{t(statusNames[status])}</option>)}</Select></Field>
          <Button variant="secondary" disabled={busy} onClick={() => run(() => loadQueue())}>{t("刷新审核队列")}</Button></div>
        {!queue.items.length ? <EmptyState title={t("此状态下没有投稿。")} /> : <div className="marketplace-list" role="list">{queue.items.map(row => <article className="marketplace-catalog-row" role="listitem" key={row.submissionId}>
          <div className="marketplace-catalog-copy"><div className="marketplace-row-title"><h3>{row.name}</h3><Badge>{row.version}</Badge><Badge>{t(statusNames[row.status])}</Badge></div><p className="marketplace-meta">{row.authorDisplayName}</p></div>
          <Button variant="secondary" size="sm" disabled={busy} onClick={() => run(() => openSubmission(row.submissionId, "review"))}>{t("审阅完整投稿")}</Button>
        </article>)}</div>}
        {(queue.offset > 0 || queue.hasMore) && <Pagination page={queue} busy={busy} load={offset => run(() => loadQueue(offset))} />}
      </section> : <section className="marketplace-area" id="marketplace-review-panel-reports" role="tabpanel" aria-labelledby="marketplace-review-tab-reports">
        <div className="marketplace-section-heading"><h2>{t("举报处理")}</h2><Button variant="secondary" disabled={busy} onClick={() => run(async () => {
          const active = activeClient(); const result = await active.reports(); current(active); setReports(result);
        })}>{t("刷新举报")}</Button></div>
        {!reports.items.length && <EmptyState title={t("当前没有待处理举报。")} />}
        {reports.items.map(report => <article className="marketplace-report" key={report.reportId}><h3>{report.pluginId} · {report.version}</h3><p>{report.reporterDisplayName} · {t(reportNames[report.reason])}</p><p>{report.note}</p>
          <div className="page-toolbar">{(["resolved", "dismissed"] as const).map(resolution => <Button key={resolution} variant="secondary" disabled={busy || report.status !== "open"} onClick={() => run(async () => {
            const active = activeClient(); await active.resolveReport(report.reportId, resolution); const result = await active.reports(); current(active); setReports(result);
          }, "举报处理已记录。")}>{t(resolution === "resolved" ? "标记已处理" : "驳回举报")}</Button>)}</div>
        </article>)}
        <Pagination page={reports} busy={busy} load={offset => run(async () => {
          const active = activeClient(); const result = await active.reports({ offset }); current(active); setReports(result);
        })} />
      </section>}
    </section>}
    <Modal className="marketplace-dialog" open={panel === "source"} title={t("市场来源")} busy={busy} onClose={closePanel}>
      {feedback}
      <form className="form-stack" autoComplete="off" onSubmit={event => {
        event.preventDefault(); run(async () => {
          const normalized = normalizeMarketplaceOrigin(address, import.meta.env.DEV);
          clientRef.current?.dispose(); clientRef.current = null;
          setUsername(""); setDisplayName(""); setPassword(""); setSearch(""); setKind(""); setClientFilter("");
          setOrigin(normalized); setAddress(normalized); setConnectionSerial(value => value + 1);
        });
      }}>
        <Field label={t("HTTPS 服务地址")} description={t("使用部署负责人提供的 HTTPS 地址；开发 HTTP 仅允许本机回环地址。") }>
          <Input type="url" value={address} onChange={event => setAddress(event.currentTarget.value)} maxLength={256} placeholder="https://market.example.test" required disabled={busy} />
        </Field>
        <Button type="submit" disabled={busy || !address.trim()} loading={connecting}>{t("连接市场")}</Button>
      </form>
      {origin && <p className="muted">{t("当前市场")} <code>{origin}</code></p>}
    </Modal>
    <Modal className="marketplace-dialog" open={panel === "account"} title={t("市场账户")} busy={busy} onClose={closePanel}>
      {feedback}
      {accountNext === "submit" && !session && <p>{t("登录后继续投稿，公开浏览无需登录。")}</p>}
      <h3>{t("独立市场身份")}</h3><p>{t("市场账号独立，用户名仅用于登录；公共资料只展示公开名称。")}</p>
      {session ? <div className="page-toolbar"><strong>{session.user.displayName}</strong><span>{t(session.user.reviewer ? "独立审核者" : "注册投稿者")}</span>
        <Button variant="secondary" disabled={busy} onClick={() => run(async () => { await activeClient().refreshIdentity(); }, "市场身份已刷新。")}>{t("刷新身份")}</Button>
        <Button variant="quiet" disabled={busy} onClick={() => run(async () => { await activeClient().logout(); }, "市场会话已退出。")}>{t("退出市场")}</Button>
      </div> : <form className="form-stack" autoComplete="off" onSubmit={event => { event.preventDefault(); run(authenticate, "市场登录成功。会话仅保存在当前页面内存中。"); }}>
        <Field label={t("身份操作")}><Select value={authMode} disabled={busy} onChange={event => setAuthMode(event.currentTarget.value as "login" | "register")}>
          <option value="login">{t("登录市场")}</option><option value="register">{t("注册市场账号")}</option>
        </Select></Field>
        <Field label={t("市场用户名")} description={t("3 至 32 位英文字母、数字、下划线或连字符。") }><Input value={username} onChange={event => setUsername(event.currentTarget.value)} minLength={3} maxLength={32} required disabled={busy || !origin} autoComplete="off" /></Field>
        {authMode === "register" && <Field label={t("公开展示名")} description={t("请使用不包含个人信息的公开名称。") }><Input value={displayName} onChange={event => setDisplayName(event.currentTarget.value)} maxLength={80} required disabled={busy || !origin} autoComplete="off" /></Field>}
        <Field label={t("市场密码")} description={t("至少 12 字节；请勿使用订阅账号密码。") }><Input type="password" value={password} onChange={event => setPassword(event.currentTarget.value)} minLength={12} maxLength={128} required disabled={busy || !origin} autoComplete="off" /></Field>
        <Button type="submit" disabled={busy || !origin || !username || !password || authMode === "register" && !displayName}>{t(authMode === "register" ? "注册并登录" : "登录市场")}</Button>
      </form>}
      <p className="muted">{t("会话仅留在内存；退出、服务切换、会话过期或关闭页面会清除身份。")}</p>
    </Modal>
    <Modal className="marketplace-dialog" open={panel === "submit" && !!session} title={t("投稿与新版本")} size="lg" busy={busy} onClose={closePanel}>
      {feedback}
      {session && <div className="marketplace-area">
        <p>{t("所有注册用户均可投稿。修改必须使用新版本号，并重新接受独立审核。")}</p>
        <p>{t("包内 author 必须与当前公开展示名一致。") } <strong>{session.user.displayName}</strong></p>
        {replacing && <p role="status">{t("正在提交替代版本，原投稿保持不变。") } {t("原版本")} <code>{replacing.version}</code>
          <Button variant="quiet" size="sm" disabled={busy} onClick={() => { setReplacing(null); setSource(""); }}>{t("取消新版本")}</Button></p>}
        <Field label={t("导入插件包 JSON")} description={t("文件只在本机读取；完整隐私检查通过后才会上传。") }><Input type="file" accept=".json,application/json" disabled={busy} onChange={event => {
          const file = event.currentTarget.files?.[0]; event.currentTarget.value = "";
          if (file) run(async () => {
            let document: string;
            try { document = await readPluginDocument(file); } catch { throw new MarketplaceServiceError("invalid_package"); }
            const active = activeClient();
            if (!active.session) throw new MarketplaceServiceError("authentication_required");
            parseMarketplacePackage(document, [active.session.user.userId]);
            current(active); setSource(document);
          });
        }} /></Field>
        <form className="form-stack" onSubmit={event => { event.preventDefault(); run(async () => {
          const active = activeClient(); await active.submit(source, replacing?.id); current(active); setSource(""); setReplacing(null); setPanel(null); setArea("submissions"); await loadOwn();
        }, "投稿已保存并进入人工审核。公开目录将在批准并发布后更新。"); }}>
          <Field label={t("投稿包全文")} description={t("所有字段、描述、文件名和文件内容都会检查。发现隐私信息会阻止投稿，不会静默改写包。") }><Textarea rows={14} value={source} onChange={event => setSource(event.currentTarget.value)} disabled={busy} spellCheck={false} autoComplete="off" required /></Field>
          <Button type="submit" disabled={busy || !source.trim()}>{t("提交审核")}</Button>
        </form>
      </div>}
    </Modal>
    <Modal className="marketplace-dialog" open={versionPage !== null} title={t("已发布版本历史")} size="lg" busy={busy} onClose={() => setVersionPage(null)}>
      {feedback}
      <p><code>{versionPlugin}</code></p>{versionPage && <><div className="marketplace-list" role="list">{versionPage.items.map(catalogRow)}</div>
        <Pagination page={versionPage} busy={busy} load={offset => run(async () => {
          const active = activeClient(); const result = await active.versions(versionPlugin, { offset }); current(active); setVersionPage(result);
        })} /></>}
    </Modal>
    <Modal className="marketplace-dialog" open={plan !== null} title={t("本机签名安装预览")} size="lg" busy={busy} onClose={() => setPlan(null)} footer={<>
      <Button variant="secondary" disabled={busy} onClick={() => setPlan(null)}>{t("取消安装")}</Button>
      <Button disabled={busy || !plan || installExpired || !installConfirmed || !installClients.length} onClick={() => run(async () => {
        if (!plan || plan.preview.expiresAt <= Date.now() || enableInstall && !plan.preview.validation.runtimeSupported) throw new MarketplaceServiceError("confirmation");
        const active = activeClient(); await active.confirmInstall(plan, { clients: installClients, confirmed: installConfirmed, enable: enableInstall });
        current(active); setPlan(null); setInstallConfirmed(false);
      }, "签名与权限已通过本机确认，插件已安装。")}>{t("确认安装此签名版本")}</Button>
    </>}>
      {feedback}
      {plan && <>
        <h3>{plan.published.package.manifest.name} · {plan.published.package.manifest.version}</h3>
        <p>{t("公钥须从独立可信渠道核对并在本机信任；市场返回的密钥不会自动获得信任。")}</p>
        <dl className="key-values"><dt>{t("签名密钥标识")}</dt><dd>{plan.preview.signingKeyId}</dd><dt>{t("本机已验证公钥指纹")}</dt><dd><code>{plan.preview.signingKeyFingerprint}</code></dd>
          <dt>SHA-256</dt><dd><code>{plan.published.digest}</code></dd><dt>{t("上一个本机版本")}</dt><dd>{plan.preview.previousVersion || t("无")}</dd></dl>
        {installExpired && <p className="marketplace-notice" role="alert">{t("安装预览已过期，请取消并重新预览。")}</p>}
        {!plan.preview.validation.runtimeSupported && <p className="marketplace-notice">{t("此插件的运行方式暂不受支持，可以安装但不能启用。")}</p>}
        <h4>{t("完整权限")}</h4><MarketplacePermissions value={plan.published.package.manifest.permissions} />
        <h4>{t("本次新增权限")}</h4>{plan.preview.permissionChanges.map(change => <section key={change.client}><strong>{clientNames[change.client]}</strong><MarketplacePermissions value={change.added} /></section>)}
        <fieldset className="marketplace-client-choices" disabled={busy}><legend>{t("明确选择安装客户端")}</legend>{plan.published.package.manifest.clients.map(client => <label className="marketplace-check" key={client}>
          <input type="checkbox" checked={installClients.includes(client)} onChange={event => { const checked = event.currentTarget.checked; setInstallClients(previous => checked ? [...previous, client] : previous.filter(item => item !== client)); }} /> {clientNames[client]}
        </label>)}</fieldset>
        <p><label className="marketplace-check"><input type="checkbox" checked={enableInstall} onChange={event => setEnableInstall(event.currentTarget.checked)} disabled={busy || installExpired || !plan.preview.validation.runtimeSupported} /> {t("安装后启用所选客户端；运行仍受本机隔离和权限限制。")}</label></p>
        <p><label className="marketplace-check"><input type="checkbox" checked={installConfirmed} onChange={event => setInstallConfirmed(event.currentTarget.checked)} disabled={busy} /> {t("我已核对完整权限、签名指纹和所选客户端，同意安装此精确版本。")}</label></p>
        <p className="muted">{t("确认时会重新检查公开发布内容和撤回通知；检查失败会停止安装。")}</p>
      </>}
    </Modal>
    <Modal className="marketplace-dialog" open={reportTarget !== null} title={t("举报已发布版本")} busy={busy} onClose={() => setReportTarget(null)} footer={<Button disabled={busy || !session || !reportTarget} onClick={() => run(async () => {
      if (!reportTarget) throw new MarketplaceServiceError("invalid_request");
      const active = activeClient(); await active.report(reportTarget.id, reportTarget.version, reportReason, reportNote); current(active); setReportTarget(null); setReportNote("");
    }, "举报已保存，独立审核者将处理。")}>{t("提交举报")}</Button>}>
      {feedback}
      {reportTarget && <p>{reportTarget.name} · {reportTarget.version}</p>}
      <Field label={t("举报原因")}><Select value={reportReason} onChange={event => setReportReason(event.currentTarget.value as MarketplaceReport["reason"])} disabled={busy}>{Object.entries(reportNames).map(([reason, label]) => <option key={reason} value={reason}>{t(label)}</option>)}</Select></Field>
      <Field label={t("必要说明")} description={t("请仅填写必要说明，不要填写个人信息或凭据。") }><Textarea value={reportNote} onChange={event => setReportNote(event.currentTarget.value)} rows={4} maxLength={4096} disabled={busy} /></Field>
    </Modal>
    <Modal className="marketplace-dialog" open={detail !== null} title={t("投稿审阅")} size="lg" busy={busy} onClose={() => setDetail(null)} footer={detailScope === "review" && session?.user.reviewer && detail?.status === "submitted" ? <>
      <Button variant="danger" disabled={busy} onClick={() => run(() => decide("reject"), "此投稿已拒绝。")}>{t("拒绝投稿")}</Button>
      <Button variant="secondary" disabled={busy} onClick={() => run(() => decide("request_changes"), "已要求作者使用新版本修订。")}>{t("要求修订")}</Button>
      <Button disabled={busy || !reviewConfirmed} onClick={() => run(() => decide("approve"), "精确版本和摘要已批准，等待发布。")}>{t("批准此版本")}</Button>
    </> : detail?.status === "approved" ? <Button disabled={busy} onClick={() => run(publish, "已批准的精确包已签名发布。")}>{t("发布已批准版本")}</Button> : undefined}>
      {feedback}
      {detail && <>
        <h3>{detail.package.manifest.name} · {detail.package.manifest.version}</h3><p>{detail.authorDisplayName} · {t(statusNames[detail.status])}</p>
        <Field label={t("所审阅版本的精确摘要")}><Input value={detail.digest} readOnly /></Field>
        <h4>{t("完整权限")}</h4><MarketplacePermissions value={detail.package.manifest.permissions} />
        <h4>{t("本次新增权限")}</h4><MarketplacePermissions value={detail.permissionChanges} />
        <details className="workspace-disclosure" open><summary>{t("只读包全文")}</summary><pre className="marketplace-code-preview">{canonicalMarketplacePackage(detail.package)}</pre></details>
        {detail.reviewNote && <p>{t("审核说明")} {detail.reviewNote}</p>}
        <Field label={t("审核或撤回说明")} description={t("请仅填写必要说明，不要填写个人信息或凭据。") }><Textarea value={reviewNote} rows={3} onChange={event => setReviewNote(event.currentTarget.value)} maxLength={4096} disabled={busy} /></Field>
        {detailScope === "review" && session?.user.reviewer && detail.status === "submitted" &&
          <label className="marketplace-check"><input type="checkbox" checked={reviewConfirmed} onChange={event => setReviewConfirmed(event.currentTarget.checked)} disabled={busy} /> {t("我已审阅此版本的完整内容和权限。")}</label>
        }
        {detail.status !== "withdrawn" && <details className="workspace-disclosure"><summary>{t("撤回版本")}</summary><div className="workspace-disclosure__body"><p>{t("撤回后停止公开下载；已发布版本的签名撤回记录会保留。")}</p>
          <Field label={t("撤回原因")}><Select value={withdrawReason} onChange={event => setWithdrawReason(event.currentTarget.value as WithdrawalReason)} disabled={busy}>{Object.entries(withdrawalNames).map(([reason, label]) => <option key={reason} value={reason}>{t(label)}</option>)}</Select></Field>
          <Button variant="danger" disabled={busy} onClick={() => run(withdraw, "此版本已撤回，公开下载已停止。")}>{t("确认撤回此版本")}</Button>
        </div></details>}
        <details className="workspace-disclosure"><summary>{t("此投稿的审计记录")}</summary><div className="marketplace-audit">{audit.map(event => <p key={event.sequence}>{event.actorDisplayName} · {event.action} · {event.version} · {event.digest} {event.note}</p>)}</div></details>
      </>}
    </Modal>
  </section>;
}
