import { useLanguage, t } from "../../i18n/language";
import { useEffect, useRef, useState } from "react";
import { ArrowLeft, Cable, Copy, Pencil, Trash2 } from "lucide-react";
import { Badge, Button, EmptyState, InlineError, LoadingState, Modal } from "../../ui";
import { ProviderSectionDisclosure, providerSections, providerSectionSummary, type ProviderAdvancedSection, type ProviderSection } from "./ProviderSectionNavigation";
import { ProviderEditor, type ProviderNavigationGuard } from "./ProviderEditor";
import { ProviderModelEditor } from "./ProviderModelEditor";
import { ProviderSwitchConfirmation } from "./ProviderChangeScope";
import { ProviderFeedback } from "./ProviderFeedback";
import { protocolLabels } from "./ProviderRow";
import type { ProvidersController } from "./useProviders";
import "./providers.css";

export function ProviderConfigurationPage({ controller: c, providerId, onBack, onDiagnostics, onRuntimeChanged, onNavigationGuardChange }: { onNavigationGuardChange?: (guard: ProviderNavigationGuard | null) => void; controller: ProvidersController; providerId: string; onBack: () => void; onDiagnostics: (id: string) => void; onRuntimeChanged: () => Promise<void> }) {
  useLanguage();
  const [editing, setEditing] = useState<ProviderSection | null>(null);
  const [openSections, setOpenSections] = useState<Partial<Record<ProviderAdvancedSection, boolean>>>({});
  const [modelOpen, setModelOpen] = useState(false);
  const [switching, setSwitching] = useState(false);
  const [deleting, setDeleting] = useState(false);
  const root = useRef<HTMLDivElement>(null);
  const deleteCancelRef = useRef<HTMLButtonElement>(null);
  const returnState = useRef<{ action: string; scrollTop: number } | null>(null);
  const remember = (action: string) => { returnState.current = { action, scrollTop: root.current?.closest<HTMLElement>("#workspace-content")?.scrollTop ?? 0 }; };
  const edit = (next: ProviderSection) => { remember(next); setEditing(next); };
  useEffect(() => {
    if (editing || modelOpen || switching || !returnState.current || !root.current) return;
    const { action, scrollTop } = returnState.current;
    const target = root.current.querySelector<HTMLButtonElement>('[data-edit-section="' + action + '"]');
    (target && !target.disabled ? target : root.current.querySelector<HTMLElement>("h1"))?.focus({ preventScroll: true });
    const scroll = root.current.closest<HTMLElement>("#workspace-content");
    if (scroll) scroll.scrollTop = scrollTop;
    returnState.current = null;
  }, [editing, modelOpen, switching]);
  const provider = c.providers.find(item => item.id === providerId);
  const busy = c.busy || Boolean(c.testing);
  const mutationBlocked = busy || c.loading || Boolean(c.readError);
  if (c.loading && !provider) return <div className="workspace-page"><LoadingState label={t("读取供应商配置…")} /></div>;
  if (!provider) return <div className="workspace-page"><ProviderFeedback controller={c} /><EmptyState title={t("所选供应商已不存在")} description={t("返回供应商列表，选择其他配置。")} action={<Button variant="secondary" onClick={onBack}>{t("返回供应商")}</Button>} /></div>;
  if (editing) return <ProviderEditor onNavigationGuardChange={onNavigationGuardChange} key={provider.id} provider={provider} providers={c.providers} initialSection={editing} disabled={busy || c.loading} readError={c.readError} onClose={() => setEditing(null)} onSaved={c.saved} onRuntimeChanged={onRuntimeChanged} />;
  return <div ref={root} className="workspace-page provider-configuration-page">
    <Button className="provider-back" variant="quiet" size="sm" icon={<ArrowLeft size={16} />} disabled={busy} onClick={onBack}>{t("返回供应商")}</Button>
    <header className="page-heading"><div className="page-heading__copy"><span className="page-eyebrow">{t("配置详情")}</span><div className="provider-page-title"><h1 tabIndex={-1}>{provider.name}</h1>{provider.id === c.currentId && <Badge tone="accent">{t("当前配置")}</Badge>}<Badge tone={provider.enabled ? "neutral" : "warning"}>{provider.enabled ? t("已启用") : t("已禁用")}</Badge></div><p className="page-description">{t("连接和默认模型在此维护；更多参数按任务展开。")}</p></div><div className="page-actions"><Button variant="secondary" data-edit-section="activate" disabled={mutationBlocked || !provider.enabled || provider.id === c.currentId} onClick={() => { remember("activate"); setSwitching(true); }}>{t("设为当前")}</Button><Button icon={<Pencil size={16} />} disabled={mutationBlocked} data-edit-section="connection" onClick={() => edit("connection")}>{t("编辑配置")}</Button></div></header>
    <ProviderFeedback controller={c} />
    <div className="provider-detail-main">
      <section className="provider-connection-summary" aria-labelledby="provider-view-connection">
        <h2 id="provider-view-connection">{t("连接配置")}</h2>
        <dl className="provider-config-values"><div className="span-two"><dt>{t("端点")}</dt><dd className="mono">{provider.endpoint}</dd></div><div><dt>{t("协议")}</dt><dd>{protocolLabels[provider.advanced.upstreamFormat]}</dd></div><div><dt>{t("URL 模式")}</dt><dd>{provider.advanced.isFullUrl ? t("完整 URL") : t("自动追加路径")}</dd></div><div><dt>{t("供应商 ID")}</dt><dd className="mono">{provider.id}</dd></div><div><dt>{t("供应商类型")}</dt><dd className="mono">{provider.providerType || "custom"}</dd></div></dl>
        <div className="provider-default-model"><div><span>{t("默认模型")}</span><code>{provider.model || t("未配置")}</code></div><Button variant="secondary" size="sm" icon={<Pencil size={14} />} data-edit-section="model" disabled={mutationBlocked} onClick={() => { remember("model"); setModelOpen(true); }}>{t("修改默认模型")}</Button></div>
        <p className="provider-privacy-note">{t("默认模型独立于模型映射；已有映射和请求配置会保留。")}</p>
      </section>
      <div className="provider-task-list" aria-label={t("可选配置")}>{providerSections.map(task => <ProviderSectionDisclosure key={task.id} id={"provider-view-" + task.id} title={t(task.label)} summary={providerSectionSummary(provider.advanced, task.id)} open={Boolean(openSections[task.id])} onOpenChange={open => setOpenSections(value => ({ ...value, [task.id]: open }))}>
        {task.id === "models" && <><p className="page-description">{t("按模型 ID 或显示名称匹配请求；默认模型单独设置。")}</p><div className="provider-model-table"><table><caption className="sr-only">{t("模型映射")}</caption><thead><tr><th scope="col">{t("模型 ID")}</th><th scope="col">{t("显示名称")}</th><th scope="col">{t("上下文窗口")}</th></tr></thead><tbody>{provider.advanced.modelMappings.length ? provider.advanced.modelMappings.map((mapping, index) => <tr key={mapping.model + ":" + index}><td className="mono">{mapping.model}</td><td>{mapping.displayName || "—"}</td><td className="mono">{mapping.contextWindow ?? "—"}</td></tr>) : <tr><td colSpan={3} className="muted">{t("未设置模型映射，使用默认模型。")}</td></tr>}</tbody></table></div></>}
        {task.id === "request" && <dl className="provider-config-values"><div className="span-two"><dt>User-Agent</dt><dd className="mono">{provider.advanced.customUserAgent || t("由代理管理")}</dd></div><div><dt>{t("记录分组")}</dt><dd>{provider.group}</dd></div></dl>}
        {(task.id === "headers" || task.id === "body") && <p className="page-description">{t("此处仅显示覆盖项数量，进入编辑后查看或修改内容。")}</p>}
        {task.id === "codex" && <dl className="provider-config-values"><div><dt>config.toml</dt><dd>{t(provider.advanced.codex.configToml.trim() ? "已配置" : "未设置")}</dd></div><div><dt>{t("启用远程压缩")}</dt><dd>{t(provider.advanced.codex.remoteCompression ? "已启用" : "已禁用")}</dd></div><div><dt>{t("应用通用配置")}</dt><dd>{t(provider.advanced.codex.applyCommonConfig ? "已启用" : "已禁用")}</dd></div><div><dt>{t("1M 上下文窗口")}</dt><dd>{t(provider.advanced.codex.context1m ? "已启用" : "已禁用")}</dd></div><div><dt>{t("压缩阈值")}</dt><dd className="mono">{provider.advanced.codex.compactionThreshold}</dd></div></dl>}
        {task.id === "claude" && <dl className="provider-config-values"><div><dt>{t("禁用 Artifact 工具")}</dt><dd>{t(provider.advanced.claude.disableArtifactTool ? "已启用" : "已禁用")}</dd></div></dl>}
        <Button variant="secondary" size="sm" icon={<Pencil size={14} />} disabled={mutationBlocked} data-edit-section={task.id} onClick={() => edit(task.id)}>{task.id === "models" ? t("编辑模型映射") : t("编辑{section}", { section: t(task.label) })}</Button>
      </ProviderSectionDisclosure>)}</div>
      <section className="provider-diagnostic-link" aria-labelledby="provider-diagnostic-title"><div><h2 id="provider-diagnostic-title">{t("连接测试与模型验证")}</h2><p className="page-description">{t("配置就绪不代表实际连通。测试结果请在连接诊断中查看。")}</p></div><Button variant="secondary" icon={<Cable size={16} />} disabled={busy} onClick={() => onDiagnostics(provider.id)}>{t("打开连接诊断")}</Button></section>
      <details className="provider-management-disclosure"><summary>{t("配置管理")}</summary><div className="provider-management"><p className="page-description">{t("此页面不展示 API Key。复制配置不会携带密钥。")}</p><div className="page-actions"><Button variant="secondary" icon={<Copy size={16} />} disabled={mutationBlocked} onClick={() => void c.copy(provider)}>{t("复制（不含密钥）")}</Button><Button variant="danger" icon={<Trash2 size={16} />} disabled={mutationBlocked || provider.builtIn} title={provider.builtIn ? t("内置供应商不能删除") : undefined} onClick={() => setDeleting(true)}>{t("删除供应商")}</Button></div></div></details>
    </div>
    {modelOpen && <ProviderModelEditor provider={provider} current={provider.id === c.currentId} disabled={busy || c.loading} readError={c.readError} onSaved={c.saved} onRuntimeChanged={onRuntimeChanged} onNavigationGuardChange={onNavigationGuardChange} onClose={() => setModelOpen(false)} />}
    {switching && <ProviderSwitchConfirmation provider={provider} controller={c} onClose={() => setSwitching(false)} />}
    {deleting && <Modal open size="sm" role="alertdialog" hideClose title={t("删除供应商")} description={t("确认删除“") + provider.name + t("”？供应商配置和安全凭据将一并删除，此操作无法撤销。")} busy={busy} initialFocusRef={deleteCancelRef} onClose={() => { if (!busy) setDeleting(false); }} footer={<>
      <Button ref={deleteCancelRef} variant="secondary" disabled={busy} onClick={() => setDeleting(false)}>{t("取消")}</Button>
      <Button variant="danger" disabled={mutationBlocked || provider.builtIn} onClick={() => { if (!mutationBlocked && !provider.builtIn) void c.remove(provider).then(removed => { if (removed) { setDeleting(false); onBack(); } }); }}>{t("确认删除")}</Button>
    </>}><InlineError message={c.readError || c.error} /></Modal>}
  </div>;
}
