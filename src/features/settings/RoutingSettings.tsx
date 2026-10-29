import { useLanguage, t } from "../../i18n/language";
import { useEffect, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { Badge, Button, Field, Input, InlineError, LoadingState, Toast, Toggle } from "../../ui";
import { loadAppTakeovers, setAppTakeoverEnabled } from "../../lib/takeover-service";
import { saveProxySettings, setProxyEnabled } from "../../lib/proxy-service";
import { validateProxySettings } from "../../lib/proxy-validation";
import { errorText } from "../../lib/error";
import { routeTargetLabels, routeTargets, type ProxySettings, type ProxyStatus } from "../../types/proxy";
import type { AppTakeoverStatus } from "../../types/takeover";
import { usePageOperation } from "../shared/usePageOperation";

export type ProxyMutation = (work: () => Promise<ProxyStatus>) => Promise<ProxyStatus>;
export function RoutingSettings({ desktop, status, runtimeBusy, runtimeError, mutateProxy, onRefresh }: { desktop: boolean; status: ProxyStatus | null; runtimeBusy: boolean; runtimeError: string; mutateProxy: ProxyMutation; onRefresh: () => Promise<void> }) {
  useLanguage();
  const [draft, setDraft] = useState<ProxySettings | null>(status?.settings ?? null);
  const operation = usePageOperation();
  const takeovers = useQuery({ queryKey: ["takeovers", status?.running, status?.address], queryFn: loadAppTakeovers, retry: false, refetchOnWindowFocus: false });
  // Running-state updates must not discard an unsaved port/target edit.
  const savedConfiguration = status ? JSON.stringify({ ...status.settings, enabled: undefined }) : "";
  useEffect(() => { if (savedConfiguration) setDraft(JSON.parse(savedConfiguration) as ProxySettings); }, [savedConfiguration]);
  const busy = operation.busy || runtimeBusy;
  const patch = (value: Partial<ProxySettings>) => setDraft(previous => previous ? { ...previous, ...value } : previous);
  const refresh = async () => { await onRefresh(); await takeovers.refetch({ throwOnError: true }); };
  if (!status || !draft) return <div className="section">{runtimeError || operation.error ? <><Badge tone="warning">{t("状态未知")}</Badge><p className="muted">{t("无法确认本地路由的实际运行状态，请重新读取后再操作。")}</p></> : <LoadingState label={t("正在读取本地路由状态…")} />}<Button variant="secondary" loading={busy} onClick={() => void operation.run(refresh)}>{t("重新读取")}</Button><InlineError message={operation.error} /></div>;
  const request = { ...draft, enabled: status.settings.enabled };
  const invalid = validateProxySettings(request);
  const dirty = request.host !== status.settings.host || request.port !== status.settings.port || request.showInHome !== status.settings.showInHome || JSON.stringify([...request.routeTargets].sort()) !== JSON.stringify([...status.settings.routeTargets].sort());
  const toggle = () => operation.run(async () => {
    const requested = !status.running;
    const next = await mutateProxy(() => setProxyEnabled(requested));
    if (next.running !== requested) throw new Error(next.lastError || t("路由实际状态与请求不一致。"));
    try { await takeovers.refetch({ throwOnError: true }); }
    catch (cause) { throw new Error(t("路由状态已更新，但客户端接管刷新失败：") + errorText(cause)); }
  }, status.running ? t("本地路由已停止") : t("本地路由已启动"));
  const save = () => operation.run(async () => {
    if (invalid) throw new Error(invalid);
    const next = await mutateProxy(() => saveProxySettings({ ...request, host: request.host.trim() }));
    setDraft(next.settings);
    try { await takeovers.refetch({ throwOnError: true }); }
    catch (cause) { throw new Error(t("路由配置已保存，但客户端接管刷新失败：") + errorText(cause)); }
    if (next.lastError) throw new Error(t("路由配置已保存，运行状态异常：") + next.lastError);
  }, t("路由配置已保存"));
  const toggleTakeover = (item: AppTakeoverStatus) => operation.run(async () => {
    try {
      const next = await setAppTakeoverEnabled(item.app, !item.active);
      if (next.active === item.active || next.lastError) throw new Error(next.lastError || t("客户端回读状态与请求不一致。"));
    } catch (cause) {
      await takeovers.refetch();
      throw cause;
    }
    try { await refresh(); }
    catch (cause) { throw new Error(t("客户端配置已更新，但状态刷新失败：") + errorText(cause)); }
  }, item.label + (item.active ? t(" 已恢复原配置") : t(" 已接入本地路由")));
  return <div className="settings-sections">
    <section className="settings-section"><div className="settings-section__heading"><h2>{t("本地路由")}</h2><p>{t("保存的启用偏好与实际进程状态分别展示。")}</p></div><div className="form-stack">
      <div className="section-heading"><div className="inline-actions"><Badge dot tone={status.running ? "success" : status.lastError ? "danger" : "neutral"}>{status.running ? t("运行中") : status.lastError ? t("启动异常") : t("已停止")}</Badge><span className="muted">{t("启动偏好：")}{status.settings.enabled ? t("启用") : t("关闭")}</span></div><div className="inline-actions"><Button variant="secondary" disabled={!desktop || busy} onClick={() => void toggle()}>{status.running ? t("停止路由") : t("启动路由")}</Button><Button variant="quiet" loading={busy} onClick={() => void operation.run(refresh)}>{t("刷新状态")}</Button></div></div>
      <code className="settings-address">{status.address}</code><InlineError message={status.lastError ?? ""} />
      <form className="form-stack" onSubmit={event => { event.preventDefault(); void save(); }}><fieldset className="settings-fieldset form-stack" disabled={!desktop || busy}>
        <div className="form-grid"><Field label={t("监听地址")} description={t("仅支持回环地址。")}><Input value={draft.host} onChange={event => patch({ host: event.target.value })} required /></Field><Field label={t("监听端口")}><Input type="number" min={1024} max={65535} step={1} required value={Number.isNaN(draft.port) ? "" : draft.port} onChange={event => patch({ port: event.target.value === "" ? NaN : Number(event.target.value) })} /></Field></div>
        <fieldset className="settings-targets"><legend>{t("协议路由目标")}</legend>{routeTargets.map(target => <label key={target}><input type="checkbox" checked={draft.routeTargets.includes(target)} onChange={event => patch({ routeTargets: event.target.checked ? [...draft.routeTargets, target] : draft.routeTargets.filter(value => value !== target) })} />{routeTargetLabels[target]}<code>/{target}</code></label>)}</fieldset>
        <div className="form-switch"><div className="form-switch__copy"><strong>{t("在工作台突出路由状态")}</strong><small>{t("保存首页路由显示偏好；底部运行状态始终可见。")}</small></div><Toggle aria-label={t("在工作台突出路由状态")} checked={draft.showInHome} onCheckedChange={showInHome => patch({ showInHome })} /></div>
        {dirty && invalid && <InlineError message={invalid} />}<div className="inline-actions"><Button type="submit" disabled={!dirty || Boolean(invalid)} loading={operation.busy}>{t("保存路由配置")}</Button><Button variant="secondary" disabled={!dirty} onClick={() => setDraft(status.settings)}>{t("撤销修改")}</Button></div>
      </fieldset></form>
      <InlineError message={operation.error} />{operation.message && <Toast title={operation.message} tone="success" onDismiss={() => operation.setMessage("")} />}
    </div></section>
    <section className="settings-section"><div className="settings-section__heading"><h2>{t("客户端接管")}</h2><p>{t("写入或恢复对应 CLI 配置；不会仅凭安装状态推断已接管。")}</p></div><div className="form-stack">
      {takeovers.isPending && <LoadingState label={t("正在读取客户端配置…")} />}{takeovers.error && <InlineError message={t("客户端状态读取失败：") + errorText(takeovers.error)} onRetry={() => void takeovers.refetch()} />}
      {takeovers.data?.map(item => <article className="settings-takeover" key={item.app}><div className="section-heading"><div className="inline-actions"><h3>{item.label}</h3><Badge tone={item.lastError ? "danger" : item.active ? "success" : "neutral"}>{item.lastError ? t("状态异常") : item.active ? t("已接管") : t("未接管")}</Badge></div><Button size="sm" variant="secondary" disabled={!desktop || busy || takeovers.isFetching || takeovers.isError} onClick={() => void toggleTakeover(item)}>{item.active ? t("恢复原配置") : t("接入路由")}</Button></div><p className="muted">{item.installed ? t("已检测到客户端配置") : t("未检测到现有配置")}</p>{item.configPaths.map(config => <code className="settings-path" key={config}>{config}</code>)}<InlineError message={item.lastError ?? ""} /></article>)}
    </div></section>
  </div>;
}
