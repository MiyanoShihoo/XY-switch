import { useLanguage, t } from "../../i18n/language";
import { useState } from "react";
import { ArrowLeft, Cable, Copy, Pencil, Settings2, Trash2 } from "lucide-react";
import { Badge, Button, EmptyState, LoadingState } from "../../ui";
import { ConfirmActionDialog } from "../../components/ConfirmActionDialog";
import { ProviderEditor } from "./ProviderEditor";
import { ProviderFeedback } from "./ProviderFeedback";
import { protocolLabels } from "./ProviderRow";
import type { ProvidersController } from "./useProviders";
import "./providers.css";

export function ProviderConfigurationPage({ controller: c, providerId, onBack, onDiagnostics, onRuntimeChanged }: { controller: ProvidersController; providerId: string; onBack: () => void; onDiagnostics: (id: string) => void; onRuntimeChanged: () => Promise<void> }) {
  useLanguage();
  const [editing, setEditing] = useState(false);
  const [deleting, setDeleting] = useState(false);
  const provider = c.providers.find(item => item.id === providerId);
  const busy = c.busy || Boolean(c.testing);
  if (c.loading) return <div className="workspace-page"><LoadingState label={t("读取供应商配置…")} /></div>;
  if (!provider) return <div className="workspace-page"><ProviderFeedback controller={c} /><EmptyState title={t("所选供应商已不存在")} description={t("返回供应商列表，选择其他配置。")} action={<Button variant="secondary" onClick={onBack}>{t("返回供应商")}</Button>} /></div>;
  return <div className="workspace-page provider-configuration-page">
    <Button className="provider-back" variant="quiet" size="sm" icon={<ArrowLeft size={16} />} onClick={onBack}>{t("返回供应商")}</Button>
    <header className="page-heading"><div className="page-heading__copy"><span className="page-eyebrow">{t("配置详情")}</span><div className="provider-page-title"><h1>{provider.name}</h1>{provider.id === c.currentId && <Badge tone="accent">{t("当前配置")}</Badge>}<Badge tone={provider.enabled ? "neutral" : "warning"}>{provider.enabled ? t("已启用") : t("已禁用")}</Badge></div><p className="page-description">{t("查看配置不会激活供应商；只有明确切换才会改变客户端配置。")}</p></div><div className="page-actions"><Button variant="secondary" disabled={busy || !provider.enabled || provider.id === c.currentId} onClick={() => void c.activate(provider.id)}>{t("设为当前")}</Button><Button icon={<Pencil size={16} />} disabled={busy} onClick={() => setEditing(true)}>{t("编辑配置")}</Button></div></header>
    <ProviderFeedback controller={c} />
    <section className="provider-detail-card" aria-labelledby="provider-connection-title"><div className="section-heading"><h2 id="provider-connection-title">{t("连接配置")}</h2><Settings2 size={18} aria-hidden="true" /></div><dl className="provider-config-values"><div><dt>{t("供应商 ID")}</dt><dd className="mono">{provider.id}</dd></div><div><dt>{t("客户端配置")}</dt><dd>{provider.group}</dd></div><div className="span-two"><dt>{t("端点")}</dt><dd className="mono">{provider.endpoint}</dd></div><div><dt>{t("协议")}</dt><dd>{protocolLabels[provider.advanced.upstreamFormat]}</dd></div><div><dt>{t("URL 模式")}</dt><dd>{provider.advanced.isFullUrl ? t("完整 URL") : t("自动追加路径")}</dd></div></dl></section>
    <section className="provider-detail-card" aria-labelledby="provider-model-title"><div className="section-heading"><h2 id="provider-model-title">{t("模型与请求参数")}</h2><Button variant="quiet" size="sm" disabled={busy} onClick={() => setEditing(true)}>{t("编辑模型映射")}</Button></div><dl className="provider-config-values"><div><dt>{t("默认模型")}</dt><dd className="mono">{provider.model || t("未配置")}</dd></div><div><dt>{t("自定义请求头 / 请求体覆盖")}</dt><dd>{Object.keys(provider.advanced.headerOverrides).length} / {Object.keys(provider.advanced.bodyOverrides).length}</dd></div></dl><div className="provider-model-table"><table><caption className="sr-only">{t("模型映射")}</caption><thead><tr><th scope="col">{t("模型 ID")}</th><th scope="col">{t("显示名称")}</th><th scope="col">{t("上下文窗口")}</th></tr></thead><tbody>{provider.advanced.modelMappings.length ? provider.advanced.modelMappings.map((mapping, index) => <tr key={mapping.model + ":" + index}><td className="mono">{mapping.model}</td><td>{mapping.displayName || "—"}</td><td className="mono">{mapping.contextWindow ?? "—"}</td></tr>) : <tr><td colSpan={3} className="muted">{t("未设置模型映射，使用默认模型。")}</td></tr>}</tbody></table></div></section>
    <section className="provider-detail-card provider-diagnostic-link" aria-labelledby="provider-diagnostic-title"><div><h2 id="provider-diagnostic-title">{t("连接测试与模型验证")}</h2><p className="page-description">{t("最近测试、延迟、错误信息和低 Token 模型验证已集中到连接诊断。")}</p></div><Button variant="secondary" icon={<Cable size={16} />} onClick={() => onDiagnostics(provider.id)}>{t("打开连接诊断")}</Button></section>
    <section className="provider-detail-card provider-management" aria-labelledby="provider-management-title"><div><h2 id="provider-management-title">{t("配置管理")}</h2><p className="page-description">{t("此页面不展示 API Key。复制配置不会携带密钥。")}</p></div><div className="page-actions"><Button variant="secondary" icon={<Copy size={16} />} disabled={busy} onClick={() => void c.copy(provider)}>{t("复制（不含密钥）")}</Button><Button variant="danger" icon={<Trash2 size={16} />} disabled={busy || provider.builtIn} title={provider.builtIn ? t("内置供应商不能删除") : undefined} onClick={() => setDeleting(true)}>{t("删除供应商")}</Button></div></section>
    {editing && <ProviderEditor key={provider.id} provider={provider} providers={c.providers} onClose={() => setEditing(false)} onSaved={c.saved} onRuntimeChanged={onRuntimeChanged} />}
    {deleting && <ConfirmActionDialog title={t("删除供应商")} message={t("确认删除“") + provider.name + t("”？供应商配置和安全凭据将一并删除，此操作无法撤销。")} busy={c.busy} error={c.error} onCancel={() => setDeleting(false)} onConfirm={() => { void c.remove(provider).then(removed => { if (removed) { setDeleting(false); onBack(); } }); }} />}
  </div>;
}
