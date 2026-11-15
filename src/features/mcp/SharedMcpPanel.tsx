import { useEffect, useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { t, useLanguage } from "../../i18n/language";
import { errorText } from "../../lib/error";
import { SHARED_MCP_QUERY_KEYS, decideSharedMcp, isSharedMcpDesktopRuntime, loadSharedMcpPending, loadSharedMcpStatus, registerSharedMcp, revokeSharedMcp, setSharedMcpEnabled, sharedMcpErrorText } from "../../lib/shared-mcp-service";
import { sharedClientLabels, sharedMcpConfigTarget, sharedMcpOwners, sharedMcpScopeNeedsReview, type SharedMcpConnection, type SharedMcpOwner, type SharedToolApproval } from "../../types/shared-mcp";
import { Badge, Button, EmptyState, Field, InlineError, LoadingState, Modal, Select, Tabs, Textarea, Toast, Toggle } from "../../ui";
import { usePageOperation } from "../shared/usePageOperation";

type PendingChange = { type: "enabled"; value: boolean } | { type: "register"; owner: SharedMcpOwner } | { type: "revoke"; connection: SharedMcpConnection };

export function SharedApprovalDetails({ approval }: { approval: SharedToolApproval }) {
  useLanguage();
  const permissions = approval.preview.requiredPermissions;
  return <div className="shared-mcp-review">
    <dl className="key-values"><dt>{t("客户端授权范围")}</dt><dd>{approval.clients.map(client => sharedClientLabels[client]).join(" / ")}</dd>
      <dt>{t("插件")}</dt><dd>{approval.preview.pluginId} · {approval.preview.version}</dd>
      <dt>{t("调用工具")}</dt><dd>{approval.preview.tool}</dd><dt>{t("调用影响")}</dt><dd>{approval.preview.effect}</dd></dl>
    <Field label={t("脱敏后的调用参数")}><Textarea rows={7} readOnly value={JSON.stringify(approval.arguments, null, 2) ?? "null"} spellCheck={false} autoComplete="off" /></Field>
    <h3>{t("本次所需权限")}</h3>
    <dl className="key-values"><dt>{t("网络访问")}</dt><dd>{permissions.network.join(", ") || t("无")}</dd>
      <dt>{t("读取路径")}</dt><dd>{permissions.readPaths.join(", ") || t("无")}</dd>
      <dt>{t("写入路径")}</dt><dd>{permissions.writePaths.join(", ") || t("无")}</dd>
      <dt>{t("启动进程")}</dt><dd>{permissions.process ? t("需要") : t("无")}</dd>
      <dt>{t("凭据访问")}</dt><dd>{permissions.credentials.join(", ") || t("无")}</dd></dl>
    <dl className="key-values"><dt>{t("凭据替换数")}</dt><dd>{approval.privacy.secretRedactions}</dd>
      <dt>{t("个人信息替换数")}</dt><dd>{approval.privacy.personalRedactions}</dd>
      <dt>{t("敏感结构字段数")}</dt><dd>{approval.privacy.protectedFields}</dd>
      <dt>{t("未检查附件数")}</dt><dd>{approval.privacy.uninspectedAttachments}</dd></dl>
    <p className="muted">{t("仅执行当前预览的脱敏参数；插件版本、客户端权限或连接变化后须重新审核。")}</p>
  </div>;
}

export function SharedMcpPanel() {
  useLanguage();
  const desktop = isSharedMcpDesktopRuntime();
  const queryClient = useQueryClient();
  const operation = usePageOperation();
  const status = useQuery({ queryKey: SHARED_MCP_QUERY_KEYS.status, queryFn: loadSharedMcpStatus, enabled: desktop, retry: false, refetchInterval: 5000 });
  const pending = useQuery({ queryKey: SHARED_MCP_QUERY_KEYS.pending, queryFn: loadSharedMcpPending, enabled: desktop && status.data?.running === true, retry: false, gcTime: 0, refetchInterval: 2000 });
  const [task, setTask] = useState<"connections" | null>(null);
  const [owner, setOwner] = useState<SharedMcpOwner>("codex");
  const [panel, setPanel] = useState<"register" | "settings" | null>(null);
  const [change, setChange] = useState<PendingChange | null>(null);
  const [sharedScopeReviewed, setSharedScopeReviewed] = useState(false);
  const [selectedApprovalId, setSelectedApprovalId] = useState<string | null>(null);
  const [reviewedCall, setReviewedCall] = useState<string | null>(null);
  const [now, setNow] = useState(Date.now);
  const approvals = desktop && status.data?.running ? pending.data ?? [] : [];
  const reviewCount = status.data?.running ? pending.isSuccess ? approvals.length : Math.max(approvals.length, status.data.pendingCount) : 0;
  const activeTask = task === "connections" || !reviewCount ? "connections" : "review";
  const selectedApproval = approvals.find(approval => approval.id === selectedApprovalId);
  const scope = change?.type === "register" ? sharedMcpOwners.find(candidate => candidate.id === change.owner) : null;
  const requiresSharedScope = !!scope && scope.clients.length > 1;
  const expired = !!selectedApproval && selectedApproval.preview.expiresAt <= now;
  const registered = status.data?.connections.find(connection => sharedMcpConfigTarget(connection.owner) === sharedMcpConfigTarget(owner));
  useEffect(() => {
    if (!approvals.length) return;
    const timer = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(timer);
  }, [approvals.length]);
  useEffect(() => {
    if (desktop && status.data && !status.data.running) {
      queryClient.setQueryData(SHARED_MCP_QUERY_KEYS.pending, []);
      setSelectedApprovalId(null); setReviewedCall(null);
    }
  }, [desktop, status.data?.running, queryClient]);
  useEffect(() => { if (!reviewCount) setTask(null); }, [reviewCount]);
  useEffect(() => {
    if (selectedApprovalId && !selectedApproval) { setSelectedApprovalId(null); setReviewedCall(null); }
  }, [selectedApprovalId, selectedApproval]);
  const refresh = async () => {
    await Promise.all([queryClient.invalidateQueries({ queryKey: ["shared-mcp"] }), queryClient.invalidateQueries({ queryKey: ["mcp"] })]);
  };
  const openChange = (next: PendingChange) => { setSharedScopeReviewed(false); operation.setError(""); setChange(next); };
  const confirmChange = async () => {
    if (!change || requiresSharedScope && !sharedScopeReviewed) return;
    const current = change;
    const done = await operation.run(async () => {
      if (current.type === "enabled") await setSharedMcpEnabled(current.value, true);
      else if (current.type === "register") await registerSharedMcp(current.owner, true, sharedScopeReviewed);
      else await revokeSharedMcp(current.connection.id, true);
      await refresh();
    }, current.type === "register" ? t("共享客户端连接已注册") : current.type === "revoke" ? t("共享客户端连接已撤销") : current.value ? t("共享 MCP 已启动") : t("共享 MCP 已停用"));
    if (done) { setChange(null); if (current.type === "register") setPanel(null); }
  };
  const decide = async (approved: boolean) => {
    if (!selectedApproval || approved && (reviewedCall !== selectedApproval.id || selectedApproval.preview.expiresAt <= Date.now())) return;
    const done = await operation.run(async () => {
      try { await decideSharedMcp(selectedApproval.id, approved, approved); }
      finally { await refresh(); }
    }, approved ? t("本次调用已批准") : t("本次调用已拒绝"));
    if (done) { setSelectedApprovalId(null); setReviewedCall(null); }
  };
  const changeTitle = change?.type === "register" ? t("注册共享客户端") : change?.type === "revoke" ? t("撤销共享客户端") : change?.value ? t("启动共享 MCP") : t("停用共享 MCP");
  const statusError = status.error ? errorText(status.error) : pending.error ? errorText(pending.error) : status.data?.lastError ? sharedMcpErrorText(status.data.lastError) : "";
  const selectedScope = sharedMcpOwners.find(candidate => candidate.id === owner);
  const modalOpen = panel !== null || change !== null || !!selectedApproval;
  return <section className="shared-mcp-panel" aria-label={t("共享插件服务")}>
    <div className="shared-mcp-heading"><div><h2>{t("共享插件服务")}</h2><p className="muted">{t("客户端复用本机已验证插件，每次工具调用均在这里审核。")}</p></div>
      <div className="shared-mcp-status"><Badge tone={status.data?.running ? "success" : status.data?.enabled ? "warning" : "neutral"}>{!desktop ? t("桌面端可用") : status.isError ? t("状态读取失败") : !status.data ? t("正在加载…") : status.data.running ? t("运行中") : status.data.enabled ? t("启动失败") : t("已停用")}</Badge>
        <Button variant="quiet" size="sm" disabled={!desktop || !status.data || operation.busy} onClick={() => { operation.setError(""); setPanel("settings"); }}>{t("服务设置")}</Button>
        <Button size="sm" disabled={!desktop || !status.data || operation.busy} onClick={() => { operation.setError(""); setPanel("register"); }}>{t("添加共享连接")}</Button>
      </div></div>
    {!desktop && <p className="muted">{t("共享 MCP 操作需要桌面应用")}</p>}
    {desktop && status.isPending && <LoadingState label={t("正在读取共享 MCP 状态…")} />}
    <InlineError message={(modalOpen ? "" : operation.error) || statusError} onRetry={desktop ? () => void refresh() : undefined} retrying={status.isFetching || pending.isFetching} />
    {operation.message && !modalOpen && <Toast title={operation.message} tone="success" onDismiss={() => operation.setMessage("")} />}
    <Tabs id="shared-mcp-tasks" label={t("共享服务任务")} value={activeTask} onChange={value => setTask(value === "connections" ? "connections" : null)} items={[{ value: "review", label: t("待审核工具调用"), count: reviewCount, disabled: !reviewCount || operation.busy }, { value: "connections", label: t("共享客户端连接"), count: status.data?.connections.length, disabled: operation.busy }]} />
    {activeTask === "review" ? <section className="shared-mcp-section shared-mcp-review-queue" id="shared-mcp-tasks-panel-review" role="tabpanel" aria-labelledby="shared-mcp-tasks-tab-review">
      <div className="shared-mcp-heading"><h3>{t("待审核工具调用")} <Badge>{approvals.length}</Badge></h3><Button variant="quiet" size="sm" disabled={!desktop || operation.busy} loading={status.isFetching || pending.isFetching} onClick={() => void refresh()}>{t("刷新")}</Button></div>
      {desktop && status.data?.running && pending.isPending && <LoadingState label={t("正在读取待审核调用…")} />}
      {desktop && status.data?.running && !pending.error && !pending.isPending && !approvals.length && <p className="muted">{t("暂无待审核调用")}</p>}
      {desktop && status.data && !status.data.running && <p className="muted">{t("启动共享服务后，待审核调用会显示在这里。")}</p>}
      {approvals.map(approval => <div className="shared-mcp-connection" key={approval.id}><div><strong>{approval.preview.pluginId} / {approval.preview.tool}</strong><p className="muted">{approval.clients.map(client => sharedClientLabels[client]).join(" / ")}</p></div><Badge tone="warning">{approval.preview.expiresAt <= now ? t("已过期") : t("还剩 {seconds} 秒", { seconds: Math.ceil((approval.preview.expiresAt - now) / 1000) })}</Badge><Button size="sm" disabled={operation.busy} onClick={() => { operation.setError(""); setPanel(null); setSelectedApprovalId(approval.id); setReviewedCall(null); setNow(Date.now()); }}>{t("审阅调用")}</Button></div>)}
    </section> : <section className="shared-mcp-section shared-mcp-target-list" id="shared-mcp-tasks-panel-connections" role="tabpanel" aria-labelledby="shared-mcp-tasks-tab-connections">
      <div className="shared-mcp-heading"><h3>{t("共享客户端连接")}</h3><Button variant="quiet" size="sm" disabled={!desktop || operation.busy} loading={status.isFetching || pending.isFetching} onClick={() => void refresh()}>{t("刷新")}</Button></div>
      {status.data && !status.data.connections.length && <EmptyState title={t("尚无共享连接")} description={t("添加客户端连接后，可在这里查看授权范围和撤销访问。")} />}
      <div className="shared-mcp-connections">{status.data?.connections.map(connection => <div key={connection.id} className="shared-mcp-connection"><div><strong>{sharedMcpOwners.find(candidate => candidate.id === connection.owner)?.label ?? t("客户端")}</strong><p className="muted">{connection.clients.map(client => sharedClientLabels[client]).join(" / ")}</p>{sharedMcpScopeNeedsReview(connection) && <p className="muted">{t("Claude Desktop 与 3P 共用工具配置。旧连接已停用，请清理后重新确认两种模式的授权范围。")}</p>}</div>
        <Badge tone={connection.active ? "success" : "warning"}>{connection.active ? t("已授权") : sharedMcpScopeNeedsReview(connection) ? t("需重新授权") : t("待清理")}</Badge><Button size="sm" variant="secondary" disabled={!desktop || operation.busy} onClick={() => openChange({ type: "revoke", connection })}>{connection.active ? t("撤销连接") : t("重试清理")}</Button></div>)}</div>
    </section>}
    <Modal className="mcp-dialog" open={panel === "settings" && !change && !selectedApproval} title={t("共享服务设置")} busy={operation.busy} onClose={() => setPanel(null)}>
      <div className="shared-mcp-settings">
        <div className="form-switch"><div><strong>{t("启用共享 MCP")}</strong><p className="muted">{t("重启应用后按此设置启动。")}</p></div><Toggle aria-label={t("启用共享 MCP")} checked={status.data?.enabled ?? false} disabled={!desktop || !status.data || operation.busy} onCheckedChange={value => openChange({ type: "enabled", value })} /></div>
        {desktop && status.data?.enabled && !status.data.running && <Button variant="secondary" disabled={operation.busy} onClick={() => openChange({ type: "enabled", value: true })}>{t("重新启动")}</Button>}
        <section className="shared-mcp-settings"><h3>{t("共享权限策略")}</h3><p>{t("共享连接只提供已安装且获得全部相关客户端授权的工具。")}</p>
          <p>{t("所选桌面端和 CLI 共用配置，该连接同时涵盖两端；可用工具取两端授权的交集。")}</p>
          <p>{t("Claude Desktop 的标准模式与 3P 模式共用 MCP 配置，该连接同时涵盖两种模式；可用工具取两种模式授权的交集。")}</p>
          <p className="muted">{t("连接令牌仅保存在本机凭据库；客户端配置只保存凭据引用。")}</p>
        </section>
        <InlineError message={operation.error || statusError} />
      </div>
    </Modal>
    <Modal className="mcp-dialog" open={panel === "register" && !change && !selectedApproval} title={t("添加共享连接")} busy={operation.busy} onClose={() => setPanel(null)} footer={<Button disabled={!desktop || !status.data?.running || operation.busy || !!registered} onClick={() => openChange({ type: "register", owner })}>{registered ? registered.active ? t("已注册") : t("请先清理旧连接") : t("注册连接")}</Button>}>
      <div className="shared-mcp-settings">
        <Field label={t("连接客户端")}><Select value={owner} disabled={!desktop || operation.busy} onChange={event => setOwner(event.target.value as SharedMcpOwner)}>{sharedMcpOwners.map(candidate => <option key={candidate.id} value={candidate.id}>{candidate.label}</option>)}</Select></Field>
        <dl className="key-values"><dt>{t("客户端授权范围")}</dt><dd>{selectedScope?.clients.map(client => sharedClientLabels[client]).join(" / ")}</dd></dl>
        <p className="muted">{t("注册前会再次确认客户端范围；共享配置使用权限交集。")}</p>
        {!status.data?.running && <div><p>{t("请先启动共享服务，再注册客户端连接。")}</p><Button variant="secondary" disabled={!desktop || operation.busy} onClick={() => setPanel("settings")}>{t("打开服务设置")}</Button></div>}
        {registered && <div><p>{t(registered.active ? "此客户端已有共享连接。" : "此客户端有待清理的旧连接。")}</p><Button variant="secondary" disabled={!desktop || operation.busy} onClick={() => openChange({ type: "revoke", connection: registered })}>{t(registered.active ? "撤销连接" : "重试清理")}</Button></div>}
        <InlineError message={operation.error || statusError} />
      </div>
    </Modal>
    {change && <Modal className="mcp-dialog" open title={changeTitle} busy={operation.busy} onClose={() => { if (!operation.busy) setChange(null); }} footer={<><Button variant="secondary" disabled={operation.busy} onClick={() => setChange(null)}>{t("取消")}</Button><Button variant={change.type === "revoke" || change.type === "enabled" && !change.value ? "danger" : "primary"} loading={operation.busy} disabled={requiresSharedScope && !sharedScopeReviewed} onClick={() => void confirmChange()}>{changeTitle}</Button></>}>
      {change.type === "enabled" ? <p>{change.value ? t("服务仅监听本机。客户端需要连接凭据；工具调用需逐次审阅，重启应用后会按保存的开关启动。") : t("停用会取消所有等待和执行中的工具调用；已注册配置会保留，重新启动后可继续使用。")}</p> : change.type === "revoke" ? <p>{t("撤销会立即停用此连接，取消关联调用，并清理客户端配置及本机连接凭据。")}</p> : <><p>{t("将共享插件连接写入所选客户端配置。请在客户端刷新工具列表或重新启动。")}</p><p>{scope?.clients.map(client => sharedClientLabels[client]).join(" / ")}</p>{requiresSharedScope && <><p>{scope?.clients.includes("claude-desktop-3p") ? t("Claude Desktop 的标准模式与 3P 模式共用 MCP 配置，该连接同时涵盖两种模式；可用工具取两种模式授权的交集。") : t("所选桌面端和 CLI 共用配置，该连接同时涵盖两端；可用工具取两端授权的交集。")}</p><label className="shared-mcp-checkbox"><input type="checkbox" checked={sharedScopeReviewed} onChange={event => setSharedScopeReviewed(event.target.checked)} disabled={operation.busy} />{t("确认授权上述全部客户端")}</label></>}</>}
      <InlineError message={operation.error} />
    </Modal>}
    {selectedApproval && <Modal className="mcp-dialog" open size="lg" title={t("审阅共享工具调用")} busy={operation.busy} onClose={() => { if (!operation.busy) { setSelectedApprovalId(null); setReviewedCall(null); } }} footer={<><Button variant="secondary" disabled={operation.busy} onClick={() => { setSelectedApprovalId(null); setReviewedCall(null); }}>{t("暂不处理")}</Button><Button variant="danger" loading={operation.busy} onClick={() => void decide(false)}>{t("拒绝本次调用")}</Button><Button loading={operation.busy} disabled={expired || reviewedCall !== selectedApproval.id} onClick={() => void decide(true)}>{t("允许本次调用")}</Button></>}>
      <SharedApprovalDetails approval={selectedApproval} />
      {expired ? <InlineError message={t("调用已取消或审核已过期，请让客户端重新发起。")} /> : <label className="shared-mcp-checkbox"><input type="checkbox" checked={reviewedCall === selectedApproval.id} disabled={operation.busy} onChange={event => setReviewedCall(event.target.checked ? selectedApproval.id : null)} />{t("已审阅本次参数和权限，同意执行")}</label>}
      <InlineError message={operation.error} />
    </Modal>}
  </section>;
}
