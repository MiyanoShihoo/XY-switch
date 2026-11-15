import { useEffect, useMemo, useRef, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { Check, ChevronDown, RefreshCw, Search } from "lucide-react";
import { t, useLanguage } from "../../i18n/language";
import { loadAppTakeovers } from "../../lib/takeover-service";
import { Button, InlineError, Input, Modal } from "../../ui";
import type { ProvidersController } from "./useProviders";
import "./configuration-switcher.css";

export function ConfigurationSwitcher({ controller, onManage }: { controller: ProvidersController; onManage: () => void }) {
  useLanguage();
  const [open, setOpen] = useState(false);
  const [search, setSearch] = useState("");
  const [requestedId, setRequestedId] = useState<string | null>(null);
  const searchRef = useRef<HTMLInputElement>(null);
  const scope = useQuery({ queryKey: ["takeovers"], queryFn: loadAppTakeovers });
  const current = controller.providers.find(provider => provider.id === controller.currentId);
  const affected = scope.data?.filter(status => status.active) ?? [];
  const scopeKnown = scope.data !== undefined && !scope.isError;
  const busy = controller.busy || Boolean(controller.testing);
  const locked = busy || controller.loading || Boolean(controller.readError) || !scopeKnown || scope.isFetching;
  const visible = useMemo(() => {
    const needle = search.trim().toLowerCase();
    return controller.providers.filter(provider => [provider.name, provider.model, provider.group, provider.endpoint].some(value => value.toLowerCase().includes(needle)));
  }, [controller.providers, search]);
  useEffect(() => {
    if (!requestedId || controller.busy) return;
    if (controller.currentId === requestedId && !controller.error) { setOpen(false); setRequestedId(null); }
    else if (controller.error) setRequestedId(null);
  }, [requestedId, controller.busy, controller.currentId, controller.error]);
  const retry = () => { void controller.refresh(); void scope.refetch(); };
  const currentLabel = controller.loading ? t("读取中") : controller.readError ? t("状态未知") : current?.name ?? t("选择配置");
  return <>
    <div className="configuration-switcher">
      <div className="configuration-switcher__label"><span>{t("网关配置")}</span><small>{scopeKnown ? affected.length ? t("用于 {clients}", { clients: affected.map(status => status.label).join(" / ") }) : t("尚无已接管客户端") : t("接管范围待确认")}</small></div>
      <button type="button" className="configuration-switcher__trigger" aria-haspopup="dialog" aria-label={t("切换网关配置：{name}", { name: currentLabel })} disabled={busy || controller.loading} onClick={() => { setSearch(""); setOpen(true); }}>
        <span><strong>{currentLabel}</strong>{current?.model && !controller.readError && <code title={current.model}>{current.model}</code>}</span><ChevronDown size={16} aria-hidden="true" />
      </button>
    </div>
    {!open && controller.readError && <InlineError message={controller.readError} onRetry={retry} />}
    {!open && controller.message && <p className="configuration-switcher__feedback" role="status">{controller.message}</p>}
    <Modal open={open} title={t("切换网关配置")} size="sm" className="configuration-picker" onClose={() => { if (!busy) { setOpen(false); setRequestedId(null); } }} busy={busy} initialFocusRef={searchRef} footer={<Button variant="quiet" onClick={() => { setOpen(false); onManage(); }} disabled={busy}>{t("管理供应商与模型")}</Button>}>
      <Input ref={searchRef} value={search} onChange={event => setSearch(event.target.value)} placeholder={t("搜索名称、模型或地址")} aria-label={t("搜索网关配置")} leading={<Search size={16} />} />
      <div className="configuration-picker__scope" aria-live="polite"><p>{!scopeKnown ? t("无法确认接管范围，请刷新后再切换。") : affected.length ? t("将同步所有已接管配置：") + affected.map(status => status.label).join("、") : t("仅更改当前配置；没有客户端被接管。")}</p><Button variant="quiet" size="sm" aria-label={t("刷新接管状态")} title={t("刷新接管状态")} loading={scope.isFetching} disabled={busy} icon={<RefreshCw size={14} />} onClick={() => void scope.refetch()} /></div>
      {controller.error && <InlineError message={controller.error} onRetry={controller.readError ? retry : undefined} />}
      <div className="configuration-picker__list" role="list" aria-label={t("可用网关配置")} aria-busy={busy || controller.loading}>
        {visible.map(provider => {
          const selected = provider.id === controller.currentId;
          return <div role="listitem" key={provider.id}><button type="button" className="configuration-picker__option" data-selected={selected} disabled={locked || selected || !provider.enabled} aria-label={t("设为当前配置：{name}", { name: provider.name })} onClick={() => { if (locked || selected) return; setRequestedId(provider.id); void controller.activate(provider.id); }}>
            <span className="configuration-picker__identity"><strong>{provider.name}</strong><span>{provider.group}<span aria-hidden="true"> · </span><code>{provider.model || t("未配置模型")}</code></span></span>
            <span className="configuration-picker__state">{selected ? <><Check size={16} aria-hidden="true" /><span>{t("当前")}</span></> : !provider.enabled ? t("已禁用") : requestedId === provider.id ? t("切换中…") : null}</span>
          </button></div>;
        })}
        {!visible.length && <p className="configuration-picker__empty">{search ? t("没有匹配的供应商") : t("添加供应商后，即可在这里切换配置。")}</p>}
      </div>
    </Modal>
  </>;
}
