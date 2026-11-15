import { useLanguage, t } from "../../i18n/language";
import { useEffect, useMemo, useRef, useState } from "react";
import { DndContext, closestCenter } from "@dnd-kit/core";
import { SortableContext, verticalListSortingStrategy } from "@dnd-kit/sortable";
import { Cable, Layers3, Plus, RefreshCw, Search } from "lucide-react";
import { Button, EmptyState, Input, LoadingState } from "../../ui";
import { useDragSort } from "../../hooks/useDragSort";
import { ProviderEditor, type ProviderNavigationGuard } from "./ProviderEditor";
import { ProviderFeedback } from "./ProviderFeedback";
import { ProviderRow } from "./ProviderRow";
import { ProviderModelEditor } from "./ProviderModelEditor";
import { ProviderSwitchConfirmation } from "./ProviderChangeScope";
import { ChannelGroupsPage } from "./ChannelGroupsPage";
import type { ProvidersController } from "./useProviders";
import "./providers.css";

type Filter = "all" | "enabled" | "disabled";
export interface ProviderDirectoryState {
  search: string;
  filter: Filter;
  selectedId: string | null;
  scrollTop: number;
  focusTarget: "identity" | "inspect" | "model" | "activate" | "add" | "diagnostics" | "channel-groups" | null;
}
export const DEFAULT_PROVIDER_DIRECTORY_STATE: ProviderDirectoryState = { search: "", filter: "all", selectedId: null, scrollTop: 0, focusTarget: null };
interface Props {
  controller: ProvidersController;
  onRuntimeChanged: () => Promise<void>;
  onConfigure: (id: string) => void;
  onDiagnostics: (id?: string) => void;
  directoryState?: ProviderDirectoryState;
  onDirectoryStateChange?: (state: ProviderDirectoryState) => void;
  onNavigationGuardChange?: (guard: ProviderNavigationGuard | null) => void;
}
export function ProvidersPage({ controller: c, onRuntimeChanged, onConfigure, onDiagnostics, directoryState, onDirectoryStateChange, onNavigationGuardChange }: Props) {
  useLanguage();
  const [localState, setLocalState] = useState(DEFAULT_PROVIDER_DIRECTORY_STATE);
  const state = directoryState ?? localState;
  const { search, filter } = state;
  const [adding, setAdding] = useState(false);
  const [showChannelGroups, setShowChannelGroups] = useState(false);
  const [editingModelId, setEditingModelId] = useState<string | null>(null);
  const [switchingId, setSwitchingId] = useState<string | null>(null);
  const editingModel = c.providers.find(provider => provider.id === editingModelId);
  const switchingProvider = c.providers.find(provider => provider.id === switchingId);
  const operationBusy = c.busy || Boolean(c.testing);
  const mutationBlocked = operationBusy || c.loading || Boolean(c.readError);
  const root = useRef<HTMLDivElement>(null);
  const restored = useRef(false);
  const updateDirectory = (patch: Partial<ProviderDirectoryState>) => {
    const next = { ...state, ...patch }; setLocalState(next); onDirectoryStateChange?.(next);
  };
  const remember = (selectedId: string | null, focusTarget: ProviderDirectoryState["focusTarget"]) => updateDirectory({ selectedId, focusTarget, scrollTop: root.current?.closest<HTMLElement>("#workspace-content")?.scrollTop ?? 0 });
  const inspect = (id: string) => { remember(id, document.activeElement?.getAttribute("data-row-action") === "inspect" ? "inspect" : "identity"); onConfigure(id); };
  const add = () => { remember(state.selectedId, "add"); restored.current = false; setAdding(true); };
  const editModel = (id: string) => { remember(id, "model"); restored.current = false; setEditingModelId(id); };
  const switchProvider = (id: string) => { remember(id, "activate"); restored.current = false; setSwitchingId(id); };
  useEffect(() => {
    if (adding || showChannelGroups || editingModel || switchingProvider || c.loading || restored.current || !root.current) return;
    restored.current = true;
    const row = [...root.current.querySelectorAll<HTMLElement>("[data-provider-id]")].find(item => item.dataset.providerId === state.selectedId);
    const target = state.focusTarget === "add" || state.focusTarget === "diagnostics" || state.focusTarget === "channel-groups" ? root.current.querySelector<HTMLButtonElement>('[data-directory-action="' + state.focusTarget + '"]')
      : row?.querySelector<HTMLButtonElement>(state.focusTarget === "identity" || !state.focusTarget ? ".provider-row__identity" : '[data-row-action="' + state.focusTarget + '"]');
    const focus = target && !target.disabled ? target : row?.querySelector<HTMLButtonElement>(".provider-row__identity") ?? root.current.querySelector<HTMLElement>('[data-directory-action="search"]');
    if (state.focusTarget) (focus ?? root.current.querySelector<HTMLElement>("h1"))?.focus({ preventScroll: true });
    const scroll = root.current.closest<HTMLElement>("#workspace-content");
    if (scroll) scroll.scrollTop = state.scrollTop;
  }, [adding, showChannelGroups, editingModel, switchingProvider, c.loading, state]);
  const drag = useDragSort(c.providers, async ordered => { if (!mutationBlocked) await c.reorder(ordered); });
  const enabled = c.providers.filter(provider => provider.enabled).length;
  const visible = useMemo(() => c.providers.filter(provider =>
    (filter === "all" || provider.enabled === (filter === "enabled")) &&
    [provider.name, provider.id, provider.endpoint, provider.model].some(value => value.toLowerCase().includes(search.trim().toLowerCase()))
  ), [c.providers, filter, search]);
  const filters = [{ id: "all", label: "全部", count: c.providers.length }, { id: "enabled", label: "已启用", count: enabled }, { id: "disabled", label: "已禁用", count: c.providers.length - enabled }] as const;
  const clear = () => updateDirectory({ search: "", filter: "all" });
  if (showChannelGroups) return <ChannelGroupsPage providers={c.providers} metadataLoading={c.loading} metadataError={c.readError} onBack={() => setShowChannelGroups(false)} onNavigationGuardChange={onNavigationGuardChange} />;
  if (adding) return <ProviderEditor onNavigationGuardChange={onNavigationGuardChange} provider={null} providers={c.providers} disabled={operationBusy || c.loading} readError={c.readError} onClose={id => { setAdding(false); if (id) onConfigure(id); }} onSaved={c.saved} onRuntimeChanged={onRuntimeChanged} />;
  return <div ref={root} className="workspace-page providers-page">
    <header className="page-heading"><div className="page-heading__copy"><h1 tabIndex={-1}>{t("供应商")}</h1><p className="page-description">{t("点击模型即可维护默认值；切换当前供应商会影响共享网关。")}</p></div><div className="page-actions"><Button variant="quiet" icon={<Layers3 size={16} />} data-directory-action="channel-groups" disabled={operationBusy} onClick={() => { remember(state.selectedId, "channel-groups"); restored.current = false; setShowChannelGroups(true); }}>{t("渠道组")}</Button><Button variant="quiet" icon={<Cable size={16} />} data-directory-action="diagnostics" onClick={() => { remember(state.selectedId, "diagnostics"); onDiagnostics(c.currentId || undefined); }}>{t("连接诊断")}</Button><Button icon={<Plus size={16} />} data-directory-action="add" disabled={mutationBlocked} onClick={add}>{t("添加供应商")}</Button></div></header>
    <ProviderFeedback controller={c} />
    <section className="provider-workspace" aria-label={t("供应商列表")}>
      <div className="provider-list-toolbar"><div className="provider-filters" role="group" aria-label={t("筛选供应商")}>{filters.map(item => <button key={item.id} type="button" aria-pressed={filter === item.id} onClick={() => updateDirectory({ filter: item.id })}>{t(item.label)}<span aria-hidden="true">{item.count}</span></button>)}</div><div className="provider-search-actions"><Input className="toolbar__search" data-directory-action="search" aria-label={t("搜索供应商")} leading={<Search size={16} />} placeholder={t("名称、模型或端点")} value={search} onChange={event => updateDirectory({ search: event.target.value })} /><Button variant="quiet" icon={<RefreshCw size={16} />} loading={c.loading} disabled={operationBusy} onClick={() => void c.refresh()}>{t("刷新")}</Button></div></div>
      {c.loading ? <LoadingState label={t("读取供应商配置…")} /> : <>
        {!visible.length ? <EmptyState title={c.providers.length ? t("没有匹配的供应商") : t("尚未配置供应商")} description={c.providers.length ? t("更换搜索条件或状态筛选。") : t("添加供应商，配置端点、凭据和默认模型。")} action={<Button variant="secondary" disabled={!c.providers.length && mutationBlocked} onClick={() => c.providers.length ? clear() : add()}>{c.providers.length ? t("清除筛选") : t("添加供应商")}</Button>} /> : <div className="provider-table"><div className="provider-list-columns" aria-hidden="true"><span /><span>{t("供应商")}</span><span>{t("协议 / 默认模型")}</span><span>{t("最近测试")}</span><span>{t("配置操作")}</span></div><DndContext sensors={drag.sensors} collisionDetection={closestCenter} onDragStart={drag.handleDragStart} onDragEnd={drag.handleDragEnd}><SortableContext items={visible.map(provider => provider.id)} strategy={verticalListSortingStrategy}><div className="provider-list" role="list" aria-label={t("供应商配置列表")}>{visible.map(provider => <ProviderRow key={provider.id} provider={provider} health={c.health[provider.id]} current={provider.id === c.currentId} testing={c.testing === "all" || c.testing === provider.id} busy={operationBusy} activationDisabled={c.loading || Boolean(c.readError)} onInspect={item => inspect(item.id)} onActivate={switchProvider} onEditModel={editModel} />)}</div></SortableContext></DndContext></div>}
        <div className="provider-list-footer"><span>{visible.length} / {c.providers.length} {t("项")}</span><span>{t("手柄支持拖动与键盘排序 · 查看配置不会切换供应商")}</span></div>
      </>}
    </section>
    {editingModel && <ProviderModelEditor key={editingModel.id} provider={editingModel} current={editingModel.id === c.currentId} disabled={operationBusy || c.loading} readError={c.readError} onSaved={c.saved} onRuntimeChanged={onRuntimeChanged} onClose={() => setEditingModelId(null)} onNavigationGuardChange={onNavigationGuardChange} />}
    {switchingProvider && <ProviderSwitchConfirmation provider={switchingProvider} controller={c} onClose={() => setSwitchingId(null)} />}
  </div>;
}
