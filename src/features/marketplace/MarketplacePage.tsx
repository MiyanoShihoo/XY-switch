import { useEffect, useRef, useState } from "react";
import { useLanguage, t, translateMessage } from "../../i18n/language";
import { Card, Field, Button, Modal, Select, Input, Textarea } from "../../ui";
import { usePageOperation } from "../shared/usePageOperation";
import { MarketplaceClient, MarketplaceServiceError, canonicalMarketplacePackage, marketplaceErrorText, normalizeMarketplaceOrigin, parseMarketplacePackage } from "../../lib/marketplace-service";
import { readPluginDocument } from "../../lib/plugins-service";
import { pluginClientLabels as clientNames } from "../plugins/clients";
import { pluginClients, type PluginClient, type PluginKind, type PluginPermissions } from "../../types/plugins";
import { submissionStatuses, type MarketplaceAuditEvent, type MarketplaceCatalogEntry, type MarketplaceInstallPlan, type MarketplacePageResult, type MarketplaceReport, type MarketplaceSession, type MarketplaceSubmission, type MarketplaceSubmissionSummary, type ReviewDecision, type SubmissionStatus, type WithdrawalReason } from "../../types/marketplace";

const statusNames: Record<SubmissionStatus, string> = { submitted: "待审核", changes_requested: "需要修订", rejected: "已拒绝", approved: "已批准，待发布", published: "已发布", withdrawn: "已撤回" };
const withdrawalNames: Record<WithdrawalReason, string> = { security: "安全风险", privacy: "隐私问题", withdrawn: "作者撤回" };
const reportNames: Record<MarketplaceReport["reason"], string> = { security: "安全风险", privacy: "隐私问题", malware: "恶意内容", copyright: "版权问题", other: "其他问题" };
const emptyPage = <Item,>(): MarketplacePageResult<Item> => ({ items: [], limit: 20, offset: 0, hasMore: false });

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
  return <div className="page-toolbar" aria-label={t("市场分页")}>
    <Button variant="secondary" size="sm" disabled={busy || page.offset === 0} onClick={() => load(Math.max(0, page.offset - page.limit))}>{t("上一页")}</Button>
    <span>{t("起始位置")} {page.offset + 1}</span>
    <Button variant="secondary" size="sm" disabled={busy || !page.hasMore} onClick={() => load(page.offset + page.limit)}>{t("下一页")}</Button>
  </div>;
}

function initialOrigin(): string {
  const configured: unknown = import.meta.env.VITE_XY_MARKETPLACE_ORIGIN;
  if (typeof configured !== "string" || !configured) return "";
  try { return normalizeMarketplaceOrigin(configured, import.meta.env.DEV); } catch { return ""; }
}

export function MarketplacePage() {
  useLanguage();
  const operation = usePageOperation();
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
  const busy = operation.busy || connecting;

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
    setSession(null); setCatalog(emptyPage()); setVersionPage(null); setConnecting(true);
    const unsubscribe = active.subscribe(identity => {
      setSession(identity);
      if (!identity) {
        setOwn(emptyPage()); setQueue(emptyPage()); setReports(emptyPage()); setDetail(null); setAudit([]);
        setSource(""); setReplacing(null); setReviewNote(""); setReportTarget(null); setReportNote("");
        setPassword(""); setPlan(null); setInstallConfirmed(false);
      }
    });
    void active.catalog().then(result => {
      if (clientRef.current === active) { setCatalog(result); operation.setError(""); operation.setMessage("市场连接已验证。"); }
    }).catch(error => {
      if (clientRef.current === active) operation.setError(marketplaceErrorText(error));
    }).finally(() => { if (clientRef.current === active) setConnecting(false); });
    return () => { unsubscribe(); if (clientRef.current === active) clientRef.current = null; active.dispose(); };
  }, [origin, connectionSerial]);

  async function loadCatalog(offset = 0): Promise<void> {
    const active = activeClient();
    const result = await active.catalog({ offset, q: search || undefined, kind: kind || undefined, client: clientFilter || undefined });
    current(active); setCatalog(result);
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
  }
  async function openSubmission(id: string, scope: "own" | "review"): Promise<void> {
    const active = activeClient(); const [submission, events] = await Promise.all([active.submission(id), active.submissionAudit(id)]);
    current(active); setDetail(submission); setAudit(events.items); setDetailScope(scope); setReviewNote(""); setReviewConfirmed(false); setWithdrawReason("withdrawn");
  }
  async function updateSubmission(row: MarketplaceSubmissionSummary): Promise<void> {
    const active = activeClient(); const submission = await active.submission(row.submissionId);
    current(active); setSource(JSON.stringify(submission.package, null, 2)); setReplacing({ id: row.submissionId, version: row.version });
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

  function catalogCard(row: MarketplaceCatalogEntry) {
    return <Card key={`${row.id}-${row.version}`} className="client-card">
      <h3>{row.name}</h3><p>{row.description}</p>
      <dl className="key-values"><dt>{t("公开作者")}</dt><dd>{row.authorDisplayName}</dd><dt>{t("版本")}</dt><dd>{row.version}</dd>
        <dt>{t("支持客户端")}</dt><dd>{row.clients.map(client => clientNames[client]).join(" · ")}</dd><dt>SHA-256</dt><dd><code>{row.digest}</code></dd></dl>
      <div className="page-toolbar">
        <Button variant="secondary" size="sm" disabled={busy} onClick={() => run(async () => {
          const active = activeClient(); const result = await active.versions(row.id); current(active); setVersionPlugin(row.id); setVersionPage(result);
        })}>{t("版本历史")}</Button>
        <Button variant="secondary" size="sm" disabled={busy} onClick={() => run(() => saveDownload(row), "签名包已下载；安装时仍须通过本机公钥验证。")}>{t("下载签名包")}</Button>
        <Button size="sm" disabled={busy} onClick={() => run(() => previewInstall(row))}>{t("预览安装")}</Button>
        <Button variant="quiet" size="sm" disabled={busy || !session} onClick={() => { setVersionPage(null); setReportTarget(row); setReportNote(""); }}>{t("举报")}</Button>
      </div>
    </Card>;
  }

  return <section className="workspace-page marketplace-page" aria-label={t("共享插件市场")}>
    <header className="page-header"><div><h1>{t("共享插件市场")}</h1><p className="page-description">{t("每次投稿和更新均经过独立审核。公开目录只显示已发布版本。")}</p></div></header>
    {operation.error && <p className="workspace-route-notice" role="alert">{translateMessage(operation.error)}</p>}
    {operation.message && <p className="workspace-route-notice" role="status">{translateMessage(operation.message)}</p>}
    <Card>
      <h2>{t("市场连接")}</h2>
      <form className="form-grid" autoComplete="off" onSubmit={event => {
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
    </Card>
    <Card>
      <h2>{t("独立市场身份")}</h2><p>{t("市场账号独立，用户名仅用于登录；公共资料只展示公开名称。")}</p>
      {session ? <div className="page-toolbar"><strong>{session.user.displayName}</strong><span>{t(session.user.reviewer ? "独立审核者" : "注册投稿者")}</span>
        <Button variant="secondary" disabled={busy} onClick={() => run(async () => { await activeClient().refreshIdentity(); }, "市场身份已刷新。")}>{t("刷新身份")}</Button>
        <Button variant="quiet" disabled={busy} onClick={() => run(async () => { await activeClient().logout(); }, "市场会话已退出。")}>{t("退出市场")}</Button>
      </div> : <form className="form-grid" autoComplete="off" onSubmit={event => { event.preventDefault(); run(authenticate, "市场登录成功。会话仅保存在当前页面内存中。"); }}>
        <Field label={t("身份操作")}><Select value={authMode} disabled={busy} onChange={event => setAuthMode(event.currentTarget.value as "login" | "register")}>
          <option value="login">{t("登录市场")}</option><option value="register">{t("注册市场账号")}</option>
        </Select></Field>
        <Field label={t("市场用户名")} description={t("3 至 32 位英文字母、数字、下划线或连字符。") }><Input value={username} onChange={event => setUsername(event.currentTarget.value)} minLength={3} maxLength={32} required disabled={busy || !origin} autoComplete="off" /></Field>
        {authMode === "register" && <Field label={t("公开展示名")} description={t("请使用不包含个人信息的公开名称。") }><Input value={displayName} onChange={event => setDisplayName(event.currentTarget.value)} maxLength={80} required disabled={busy || !origin} autoComplete="off" /></Field>}
        <Field label={t("市场密码")} description={t("至少 12 字节；请勿使用订阅账号密码。") }><Input type="password" value={password} onChange={event => setPassword(event.currentTarget.value)} minLength={12} maxLength={128} required disabled={busy || !origin} autoComplete="off" /></Field>
        <Button type="submit" disabled={busy || !origin || !username || !password || authMode === "register" && !displayName}>{t(authMode === "register" ? "注册并登录" : "登录市场")}</Button>
      </form>}
      <p className="muted">{t("会话仅留在内存；退出、服务切换、会话过期或关闭页面会清除身份。")}</p>
    </Card>
    <Card>
      <h2>{t("公开已发布插件")}</h2>
      <form className="form-grid" onSubmit={event => { event.preventDefault(); run(() => loadCatalog()); }}>
        <Field label={t("搜索公开插件")}><Input value={search} onChange={event => setSearch(event.currentTarget.value)} maxLength={160} disabled={busy || !origin} /></Field>
        <Field label={t("插件类型")}><Select value={kind} disabled={busy} onChange={event => setKind(event.currentTarget.value as PluginKind | "")}><option value="">{t("全部类型")}</option><option value="mcp">MCP</option><option value="skill">{t("技能")}</option><option value="provider">{t("模型映射")}</option></Select></Field>
        <Field label={t("客户端")}><Select value={clientFilter} disabled={busy} onChange={event => setClientFilter(event.currentTarget.value as PluginClient | "")}><option value="">{t("全部客户端")}</option>{pluginClients.map(client => <option key={client} value={client}>{clientNames[client]}</option>)}</Select></Field>
        <Button type="submit" disabled={busy || !origin}>{t("刷新目录")}</Button>
      </form>
      {!catalog.items.length && <p>{t(connecting ? "正在读取公开目录…" : "当前没有可公开下载的已发布插件。")}</p>}
      <div className="client-grid">{catalog.items.map(catalogCard)}</div>
      <Pagination page={catalog} busy={busy || !origin} load={offset => run(() => loadCatalog(offset))} />
    </Card>
    {session && <>
      <Card>
        <h2>{t("投稿与新版本")}</h2><p>{t("所有注册用户均可投稿。修改必须使用新版本号，并重新接受独立审核。")}</p>
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
        <form onSubmit={event => { event.preventDefault(); run(async () => {
          const active = activeClient(); await active.submit(source, replacing?.id); current(active); setSource(""); setReplacing(null); await loadOwn();
        }, "投稿已保存并进入人工审核。公开目录将在批准并发布后更新。"); }}>
          <Field label={t("投稿包全文")} description={t("所有字段、描述、文件名和文件内容都会检查。发现隐私信息会阻止投稿，不会静默改写包。") }><Textarea rows={14} value={source} onChange={event => setSource(event.currentTarget.value)} disabled={busy} spellCheck={false} autoComplete="off" required /></Field>
          <Button type="submit" disabled={busy || !source.trim()}>{t("提交审核")}</Button>
        </form>
      </Card>
      <Card>
        <div className="page-toolbar"><h2>{t("我的审核状态")}</h2><Button variant="secondary" disabled={busy} onClick={() => run(() => loadOwn())}>{t("刷新投稿")}</Button></div>
        {!own.items.length && <p>{t("当前账号尚无投稿。")}</p>}
        <div className="client-grid">{own.items.map(row => <Card key={row.submissionId}>
          <h3>{row.name} · {row.version}</h3><p>{t(statusNames[row.status])}</p><code>{row.digest}</code>
          {row.reviewNote && <p>{t("审核说明")} {row.reviewNote}</p>}
          <div className="page-toolbar"><Button variant="secondary" size="sm" disabled={busy} onClick={() => run(() => openSubmission(row.submissionId, "own"))}>{t("查看投稿")}</Button>
            <Button size="sm" disabled={busy || !["changes_requested", "rejected", "published", "withdrawn"].includes(row.status)} onClick={() => run(() => updateSubmission(row))}>{t("提交新版本")}</Button></div>
        </Card>)}</div><Pagination page={own} busy={busy} load={offset => run(() => loadOwn(offset))} />
      </Card>
    </>}
    {session?.user.reviewer && <>
      <Card>
        <h2>{t("独立审核队列")}</h2><p>{t("作者不能自审。批准绑定所审阅全文的精确版本与摘要，批准后不能换包。")}</p>
        <div className="page-toolbar"><Field label={t("审核状态筛选")}><Select value={queueStatus} disabled={busy} onChange={event => setQueueStatus(event.currentTarget.value as SubmissionStatus)}>{submissionStatuses.map(status => <option key={status} value={status}>{t(statusNames[status])}</option>)}</Select></Field>
          <Button variant="secondary" disabled={busy} onClick={() => run(() => loadQueue())}>{t("刷新审核队列")}</Button></div>
        {!queue.items.length && <p>{t("此状态下没有投稿。")}</p>}
        <div className="client-grid">{queue.items.map(row => <Card key={row.submissionId}><h3>{row.name} · {row.version}</h3>
          <p>{row.authorDisplayName} · {t(statusNames[row.status])}</p><code>{row.digest}</code>
          <h4>{t("本次新增权限")}</h4><MarketplacePermissions value={row.permissionChanges} />
          <Button disabled={busy} onClick={() => run(() => openSubmission(row.submissionId, "review"))}>{t("审阅完整投稿")}</Button>
        </Card>)}</div><Pagination page={queue} busy={busy} load={offset => run(() => loadQueue(offset))} />
      </Card>
      <Card>
        <div className="page-toolbar"><h2>{t("举报处理")}</h2><Button variant="secondary" disabled={busy} onClick={() => run(async () => {
          const active = activeClient(); const result = await active.reports(); current(active); setReports(result);
        })}>{t("刷新举报")}</Button></div>
        {!reports.items.length && <p>{t("当前没有待处理举报。")}</p>}
        {reports.items.map(report => <Card key={report.reportId}><h3>{report.pluginId} · {report.version}</h3><p>{report.reporterDisplayName} · {t(reportNames[report.reason])}</p><p>{report.note}</p>
          <div className="page-toolbar">{(["resolved", "dismissed"] as const).map(resolution => <Button key={resolution} variant="secondary" disabled={busy || report.status !== "open"} onClick={() => run(async () => {
            const active = activeClient(); await active.resolveReport(report.reportId, resolution); const result = await active.reports(); current(active); setReports(result);
          }, "举报处理已记录。")}>{t(resolution === "resolved" ? "标记已处理" : "驳回举报")}</Button>)}</div>
        </Card>)}
        <Pagination page={reports} busy={busy} load={offset => run(async () => {
          const active = activeClient(); const result = await active.reports({ offset }); current(active); setReports(result);
        })} />
      </Card>
    </>}
    <Modal open={versionPage !== null} title={t("已发布版本历史")} size="lg" busy={busy} onClose={() => setVersionPage(null)}>
      <p><code>{versionPlugin}</code></p>{versionPage && <><div className="client-grid">{versionPage.items.map(catalogCard)}</div>
        <Pagination page={versionPage} busy={busy} load={offset => run(async () => {
          const active = activeClient(); const result = await active.versions(versionPlugin, { offset }); current(active); setVersionPage(result);
        })} /></>}
    </Modal>
    <Modal open={plan !== null} title={t("本机签名安装预览")} size="lg" busy={busy} onClose={() => setPlan(null)} footer={<>
      <Button variant="secondary" disabled={busy} onClick={() => setPlan(null)}>{t("取消安装")}</Button>
      <Button disabled={busy || !plan || !installConfirmed || !installClients.length} onClick={() => run(async () => {
        if (!plan) throw new MarketplaceServiceError("confirmation");
        const active = activeClient(); await active.confirmInstall(plan, { clients: installClients, confirmed: installConfirmed, enable: enableInstall });
        current(active); setPlan(null); setInstallConfirmed(false);
      }, "签名与权限已通过本机确认，插件已安装。")}>{t("确认安装此签名版本")}</Button>
    </>}>
      {plan && <>
        <h3>{plan.published.package.manifest.name} · {plan.published.package.manifest.version}</h3>
        <p>{t("公钥须从独立可信渠道核对并在本机信任；市场返回的密钥不会自动获得信任。")}</p>
        <dl className="key-values"><dt>{t("签名密钥标识")}</dt><dd>{plan.preview.signingKeyId}</dd><dt>{t("本机已验证公钥指纹")}</dt><dd><code>{plan.preview.signingKeyFingerprint}</code></dd>
          <dt>SHA-256</dt><dd><code>{plan.published.digest}</code></dd><dt>{t("上一个本机版本")}</dt><dd>{plan.preview.previousVersion || t("无")}</dd></dl>
        <h4>{t("完整权限")}</h4><MarketplacePermissions value={plan.published.package.manifest.permissions} />
        <h4>{t("本次新增权限")}</h4>{plan.preview.permissionChanges.map(change => <section key={change.client}><strong>{clientNames[change.client]}</strong><MarketplacePermissions value={change.added} /></section>)}
        <fieldset disabled={busy}><legend>{t("明确选择安装客户端")}</legend>{plan.published.package.manifest.clients.map(client => <label key={client}>
          <input type="checkbox" checked={installClients.includes(client)} onChange={event => { const checked = event.currentTarget.checked; setInstallClients(previous => checked ? [...previous, client] : previous.filter(item => item !== client)); }} /> {clientNames[client]}
        </label>)}</fieldset>
        <p><label><input type="checkbox" checked={enableInstall} onChange={event => setEnableInstall(event.currentTarget.checked)} disabled={busy} /> {t("安装后启用所选客户端；运行仍受本机隔离和权限限制。")}</label></p>
        <p><label><input type="checkbox" checked={installConfirmed} onChange={event => setInstallConfirmed(event.currentTarget.checked)} disabled={busy} /> {t("我已核对完整权限、签名指纹和所选客户端，同意安装此精确版本。")}</label></p>
        <p className="muted">{t("确认时会重新检查公开发布内容和撤回通知；检查失败会停止安装。")}</p>
      </>}
    </Modal>
    <Modal open={reportTarget !== null} title={t("举报已发布版本")} busy={busy} onClose={() => setReportTarget(null)} footer={<Button disabled={busy || !session || !reportTarget} onClick={() => run(async () => {
      if (!reportTarget) throw new MarketplaceServiceError("invalid_request");
      const active = activeClient(); await active.report(reportTarget.id, reportTarget.version, reportReason, reportNote); current(active); setReportTarget(null); setReportNote("");
    }, "举报已保存，独立审核者将处理。")}>{t("提交举报")}</Button>}>
      {reportTarget && <p>{reportTarget.name} · {reportTarget.version}</p>}
      <Field label={t("举报原因")}><Select value={reportReason} onChange={event => setReportReason(event.currentTarget.value as MarketplaceReport["reason"])} disabled={busy}>{Object.entries(reportNames).map(([reason, label]) => <option key={reason} value={reason}>{t(label)}</option>)}</Select></Field>
      <Field label={t("必要说明")} description={t("请仅填写必要说明，不要填写个人信息或凭据。") }><Textarea value={reportNote} onChange={event => setReportNote(event.currentTarget.value)} rows={4} maxLength={4096} disabled={busy} /></Field>
    </Modal>
    <Modal open={detail !== null} title={t("投稿审阅")} size="lg" busy={busy} onClose={() => setDetail(null)}>
      {detail && <>
        <h3>{detail.package.manifest.name} · {detail.package.manifest.version}</h3><p>{detail.authorDisplayName} · {t(statusNames[detail.status])}</p>
        <Field label={t("所审阅版本的精确摘要")}><Input value={detail.digest} readOnly /></Field>
        <h4>{t("完整权限")}</h4><MarketplacePermissions value={detail.package.manifest.permissions} />
        <h4>{t("本次新增权限")}</h4><MarketplacePermissions value={detail.permissionChanges} />
        <details className="details-group" open><summary>{t("只读包全文")}</summary><pre style={{ whiteSpace: "pre-wrap", overflowWrap: "anywhere" }}>{canonicalMarketplacePackage(detail.package)}</pre></details>
        {detail.reviewNote && <p>{t("审核说明")} {detail.reviewNote}</p>}
        <Field label={t("审核或撤回说明")} description={t("请仅填写必要说明，不要填写个人信息或凭据。") }><Textarea value={reviewNote} rows={3} onChange={event => setReviewNote(event.currentTarget.value)} maxLength={4096} disabled={busy} /></Field>
        {detailScope === "review" && session?.user.reviewer && detail.status === "submitted" && <>
          <label><input type="checkbox" checked={reviewConfirmed} onChange={event => setReviewConfirmed(event.currentTarget.checked)} disabled={busy} /> {t("我已审阅此版本的完整内容和权限。")}</label>
          <div className="page-toolbar"><Button disabled={busy || !reviewConfirmed} onClick={() => run(() => decide("approve"), "精确版本和摘要已批准，等待发布。")}>{t("批准此版本")}</Button>
            <Button variant="secondary" disabled={busy} onClick={() => run(() => decide("request_changes"), "已要求作者使用新版本修订。")}>{t("要求修订")}</Button>
            <Button variant="danger" disabled={busy} onClick={() => run(() => decide("reject"), "此投稿已拒绝。")}>{t("拒绝投稿")}</Button></div>
        </>}
        {detail.status === "approved" && <Button disabled={busy} onClick={() => run(publish, "已批准的精确包已签名发布。")}>{t("发布已批准版本")}</Button>}
        {detail.status !== "withdrawn" && <section><h4>{t("撤回版本")}</h4><p>{t("撤回后停止公开下载；已发布版本的签名撤回记录会保留。")}</p>
          <Field label={t("撤回原因")}><Select value={withdrawReason} onChange={event => setWithdrawReason(event.currentTarget.value as WithdrawalReason)} disabled={busy}>{Object.entries(withdrawalNames).map(([reason, label]) => <option key={reason} value={reason}>{t(label)}</option>)}</Select></Field>
          <Button variant="danger" disabled={busy} onClick={() => run(withdraw, "此版本已撤回，公开下载已停止。")}>{t("确认撤回此版本")}</Button>
        </section>}
        <details className="details-group"><summary>{t("此投稿的审计记录")}</summary>{audit.map(event => <p key={event.sequence}>{event.actorDisplayName} · {event.action} · {event.version} · {event.digest} {event.note}</p>)}</details>
      </>}
    </Modal>
  </section>;
}
