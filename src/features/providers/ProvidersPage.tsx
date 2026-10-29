import { useLanguage, t } from "../../i18n/language";
import { useEffect, useMemo, useState } from "react";
import { DndContext, closestCenter } from "@dnd-kit/core";
import { SortableContext, verticalListSortingStrategy } from "@dnd-kit/sortable";
import { Gauge, Plus, RefreshCw, Search } from "lucide-react";
import { Button, Drawer, EmptyState, InlineError, Input, Inspector, LoadingState, ResizableWorkspace, Toast } from "../../ui";
import { ConfirmActionDialog } from "../../components/ConfirmActionDialog";
import { useDragSort } from "../../hooks/useDragSort";
import { PROVIDER_SPLIT_STORAGE_KEY } from "../../lib/split-workspace";
import type { Provider } from "../../types/provider";
import { ProviderEditor } from "./ProviderEditor";
import { ProviderInspector } from "./ProviderInspector";
import { ProviderRow } from "./ProviderRow";
import type { ProvidersController } from "./useProviders";
import "./providers.css";
export function ProvidersPage({ controller: c, onRuntimeChanged }: { controller: ProvidersController; onRuntimeChanged: () => Promise<void> }) {
  useLanguage();
  const [search, setSearch] = useState("");
  const [inspectedId, setInspectedId] = useState("");
  const [drawerOpen, setDrawerOpen] = useState(false);
  const [compact, setCompact] = useState(() => window.matchMedia("(max-width: 1000px)").matches);
  const [editor, setEditor] = useState<{ provider: Provider | null } | null>(null);
  const [pendingDelete, setPendingDelete] = useState<Provider | null>(null);
  const drag = useDragSort(c.providers, c.reorder);
  useEffect(() => { const media = window.matchMedia("(max-width: 1000px)"); const update = () => { setCompact(media.matches); if (!media.matches) setDrawerOpen(false); }; media.addEventListener("change", update); return () => media.removeEventListener("change", update); }, []);
  const visible = useMemo(() => c.providers.filter(provider => [provider.name, provider.id, provider.endpoint, provider.model].some(value => value.toLowerCase().includes(search.trim().toLowerCase()))), [c.providers, search]);
  const inspected = c.providers.find(p => p.id === (inspectedId || c.currentId)) ?? visible[0];
  const inspect = (provider: Provider) => { setInspectedId(provider.id); if (compact) setDrawerOpen(true); };
  const inspector = <ProviderInspector provider={inspected} health={inspected ? c.health[inspected.id] : undefined} current={inspected?.id === c.currentId} testing={c.testing === "all" || c.testing === inspected?.id} busy={c.busy || Boolean(c.testing)} onEdit={provider => { setDrawerOpen(false); setEditor({ provider }); }} onCopy={provider => void c.copy(provider)} onDelete={setPendingDelete} onTest={id => void c.test(id)} />;
  return <div className="workspace-page providers-page">
    <header className="page-heading"><div className="page-heading__copy"><h1>{t("供应商")}</h1><p className="page-description">{t("点击供应商卡片切换当前配置，右侧详情图标仅查看。")}</p></div><div className="page-actions"><Button variant="secondary" size="sm" icon={<Gauge size={14} />} loading={c.testing === "all"} disabled={c.busy || c.loading || Boolean(c.testing) || !c.providers.length} onClick={() => void c.test()}>{t("测试全部")}</Button><Button size="sm" icon={<Plus size={14} />} disabled={c.busy} onClick={() => setEditor({ provider: null })}>{t("添加供应商")}</Button></div></header>
    <InlineError message={c.error} onRetry={() => void c.refresh()} retrying={c.loading} />{c.message && <Toast tone="success" title={c.message} onDismiss={c.dismissMessage} />}
    <div className="toolbar"><span className="muted">{t("共")} {c.providers.length} {t("个供应商 · 所有客户端共用当前配置")}</span><div className="inline-actions"><Input className="toolbar__search" aria-label={t("搜索供应商")} leading={<Search size={14} />} placeholder={t("名称、模型或端点")} value={search} onChange={event => setSearch(event.target.value)} /><Button variant="quiet" size="sm" icon={<RefreshCw size={14} />} loading={c.loading} disabled={c.busy} onClick={() => void c.refresh()}>{t("刷新")}</Button></div></div>
    {c.loading ? <LoadingState label={t("读取供应商配置…")} /> : <ResizableWorkspace className="provider-workspace" storageKey={PROVIDER_SPLIT_STORAGE_KEY} compact={compact} primaryLabel={t("供应商列表")} secondaryLabel={t("配置检查")} primary={<><div className="provider-list-heading"><span>{t("供应商 / 端点")}</span><span>{t("默认模型")}</span><span>{t("连接测试")}</span><span>{t("详情")}</span></div>{!visible.length ? <EmptyState title={c.providers.length ? t("没有匹配的供应商") : t("尚未配置供应商")} description={c.providers.length ? t("更换搜索条件。") : t("添加供应商，配置端点、凭据和默认模型。")} action={<Button variant="secondary" size="sm" onClick={() => c.providers.length ? setSearch("") : setEditor({ provider: null })}>{c.providers.length ? t("清除筛选") : t("添加供应商")}</Button>} /> : <DndContext sensors={drag.sensors} collisionDetection={closestCenter} onDragStart={drag.handleDragStart} onDragEnd={drag.handleDragEnd}><SortableContext items={visible.map(p => p.id)} strategy={verticalListSortingStrategy}><div role="list" aria-label={t("供应商配置列表")}>{visible.map(provider => <ProviderRow key={provider.id} provider={provider} health={c.health[provider.id]} current={provider.id === c.currentId} inspected={provider.id === inspected?.id} testing={c.testing === "all" || c.testing === provider.id} busy={c.busy || Boolean(c.testing)} onInspect={inspect} onActivate={id => void c.activate(id)} />)}</div></SortableContext></DndContext>}<div className="provider-list-footer"><span>{visible.length} / {c.providers.length} {t("项")}</span><span>{compact ? t("拖动左侧手柄排序 · 点击卡片切换 · 右侧图标查看详情") : t("左侧手柄排序 · 中间分隔线调宽 · 双击复位")}</span></div></>} secondary={<Inspector title={t("配置检查")}>{inspector}</Inspector>} />}
    <Drawer open={drawerOpen && compact} title={t("配置检查")} onClose={() => setDrawerOpen(false)}>{inspector}</Drawer>
    {editor && <ProviderEditor key={editor.provider?.id ?? "new"} provider={editor.provider} providers={c.providers} onClose={() => setEditor(null)} onSaved={provider => { c.saved(provider); setInspectedId(provider.id); }} onRuntimeChanged={onRuntimeChanged} />}
    {pendingDelete && <ConfirmActionDialog title={t("删除供应商")} message={t("确认删除“") + pendingDelete.name + t("”？供应商配置和安全凭据将一并删除，此操作无法撤销。")} busy={c.busy} error={c.error} onCancel={() => setPendingDelete(null)} onConfirm={() => { void c.remove(pendingDelete).then(removed => { if (removed) setPendingDelete(null); }); }} />}
  </div>;
}
