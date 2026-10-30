import { useLanguage, t } from "../../i18n/language";
import { useMemo, useState } from "react";
import { DndContext, closestCenter } from "@dnd-kit/core";
import { SortableContext, verticalListSortingStrategy } from "@dnd-kit/sortable";
import { ArrowUpRight, Cable, Plus, RefreshCw, Search, Waypoints } from "lucide-react";
import { Badge, Button, EmptyState, Input, LoadingState } from "../../ui";
import { useDragSort } from "../../hooks/useDragSort";
import { ProviderEditor } from "./ProviderEditor";
import { ProviderFeedback } from "./ProviderFeedback";
import { ProviderRow } from "./ProviderRow";
import type { ProvidersController } from "./useProviders";
import "./providers.css";

type Filter = "all" | "enabled" | "disabled";
export function ProvidersPage({ controller: c, onRuntimeChanged, onConfigure, onDiagnostics }: { controller: ProvidersController; onRuntimeChanged: () => Promise<void>; onConfigure: (id: string) => void; onDiagnostics: (id?: string) => void }) {
  useLanguage();
  const [search, setSearch] = useState("");
  const [filter, setFilter] = useState<Filter>("all");
  const [adding, setAdding] = useState(false);
  const drag = useDragSort(c.providers, c.reorder);
  const enabled = c.providers.filter(provider => provider.enabled).length;
  const current = c.providers.find(provider => provider.id === c.currentId);
  const visible = useMemo(() => c.providers.filter(provider =>
    (filter === "all" || provider.enabled === (filter === "enabled")) &&
    [provider.name, provider.id, provider.endpoint, provider.model].some(value => value.toLowerCase().includes(search.trim().toLowerCase()))
  ), [c.providers, filter, search]);
  const filters = [{ id: "all", label: "全部", count: c.providers.length }, { id: "enabled", label: "已启用", count: enabled }, { id: "disabled", label: "已禁用", count: c.providers.length - enabled }] as const;
  const clear = () => { setSearch(""); setFilter("all"); };
  return <div className="workspace-page providers-page">
    <header className="page-heading"><div className="page-heading__copy"><span className="page-eyebrow">{t("配置工作台")}</span><div className="provider-page-title"><h1>{t("供应商")}</h1><Badge tone="neutral">{c.providers.length}</Badge></div><p className="page-description">{t("集中管理上游配置，点击供应商即可切换。配置详情与诊断在独立页面查看。")}</p></div><div className="page-actions"><Button variant="secondary" icon={<Cable size={16} />} onClick={() => onDiagnostics(c.currentId || undefined)}>{t("连接诊断")}</Button><Button icon={<Plus size={16} />} disabled={c.busy} onClick={() => setAdding(true)}>{t("添加供应商")}</Button></div></header>
    <ProviderFeedback controller={c} />
    <section className="provider-current" aria-label={t("当前配置")}>
      <span className="provider-current__mark" aria-hidden="true"><Waypoints size={24} strokeWidth={1.6} /></span>
      <div className="provider-current__identity"><span className="page-eyebrow">{t("当前配置")}</span><strong>{c.loading ? t("读取中") : c.error && !current ? t("状态未知") : current?.name || t("尚未选择当前配置")}</strong><span>{current ? current.group + " · " + (current.model || t("未配置模型")) : t("选择启用的供应商后，客户端将共用该配置。")}</span></div>
      <div className="provider-current__count"><strong>{enabled}</strong><span>{t("已启用")}</span></div>
      {current && <Button variant="quiet" icon={<ArrowUpRight size={16} />} onClick={() => onConfigure(current.id)}>{t("查看配置")}</Button>}
    </section>
    <section className="provider-workspace" aria-label={t("供应商列表")}>
      <div className="provider-list-toolbar"><div className="provider-filters" role="group" aria-label={t("筛选供应商")}>{filters.map(item => <button key={item.id} type="button" aria-pressed={filter === item.id} onClick={() => setFilter(item.id)}>{t(item.label)}<span aria-hidden="true">{item.count}</span></button>)}</div><div className="provider-search-actions"><Input className="toolbar__search" aria-label={t("搜索供应商")} leading={<Search size={16} />} placeholder={t("名称、模型或端点")} value={search} onChange={event => setSearch(event.target.value)} /><Button variant="quiet" icon={<RefreshCw size={16} />} loading={c.loading} disabled={c.busy} onClick={() => void c.refresh()}>{t("刷新")}</Button></div></div>
      {c.loading ? <LoadingState label={t("读取供应商配置…")} /> : <>
        <div className="provider-list-heading" aria-hidden="true"><span>{t("供应商 / 端点")}</span><span>{t("默认模型 / 协议")}</span><span>{t("连接测试")}</span><span>{t("配置")}</span></div>
        {!visible.length ? <EmptyState title={c.providers.length ? t("没有匹配的供应商") : t("尚未配置供应商")} description={c.providers.length ? t("更换搜索条件或状态筛选。") : t("添加供应商，配置端点、凭据和默认模型。")} action={<Button variant="secondary" onClick={() => c.providers.length ? clear() : setAdding(true)}>{c.providers.length ? t("清除筛选") : t("添加供应商")}</Button>} /> : <DndContext sensors={drag.sensors} collisionDetection={closestCenter} onDragStart={drag.handleDragStart} onDragEnd={drag.handleDragEnd}><SortableContext items={visible.map(provider => provider.id)} strategy={verticalListSortingStrategy}><div role="list" aria-label={t("供应商配置列表")}>{visible.map(provider => <ProviderRow key={provider.id} provider={provider} health={c.health[provider.id]} current={provider.id === c.currentId} testing={c.testing === "all" || c.testing === provider.id} busy={c.busy || Boolean(c.testing)} onInspect={item => onConfigure(item.id)} onActivate={id => void c.activate(id)} />)}</div></SortableContext></DndContext>}
        <div className="provider-list-footer"><span>{visible.length} / {c.providers.length} {t("项")}</span><span>{t("手柄支持拖动与键盘排序 · 查看配置不会切换供应商")}</span></div>
      </>}
    </section>
    {adding && <ProviderEditor key="new" provider={null} providers={c.providers} onClose={() => setAdding(false)} onSaved={c.saved} onRuntimeChanged={onRuntimeChanged} />}
  </div>;
}
