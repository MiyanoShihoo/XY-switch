import { useCallback, useEffect, useRef, useState } from "react";
import { t, useLanguage } from "../../i18n/language";
import { Button, InlineError, Modal } from "../../ui";
import { loadAppTakeovers } from "../../lib/takeover-service";
import type { Provider } from "../../types/provider";
import { errorText, type ProvidersController } from "./useProviders";

export function useProviderChangeScope(required: boolean, providerId: string) {
  const scopeKey = required ? providerId : "";
  const generation = useRef(0);
  const [state, setState] = useState({ key: scopeKey, affected: [] as string[], loading: required, error: "" });
  const refresh = useCallback(async () => {
    const request = ++generation.current;
    if (!required) {
      setState({ key: scopeKey, affected: [], loading: false, error: "" });
      return;
    }
    setState({ key: scopeKey, affected: [], loading: true, error: "" });
    try {
      const statuses = await loadAppTakeovers();
      if (request === generation.current) setState({ key: scopeKey, affected: statuses.filter(status => status.active).map(status => status.label), loading: false, error: "" });
    } catch (error) {
      if (request === generation.current) setState({ key: scopeKey, affected: [], loading: false, error: t("影响范围读取失败：") + errorText(error) });
    }
  }, [required, scopeKey]);
  useEffect(() => { void refresh(); return () => { generation.current += 1; }; }, [refresh]);
  const loading = required && (state.key !== scopeKey || state.loading);
  const error = required && state.key === scopeKey ? state.error : "";
  return { affected: required && state.key === scopeKey ? state.affected : [], loading, error, ready: !loading && !error, refresh };
}

export function ProviderChangeScope({ scope, applies, disabled = false }: {
  scope: ReturnType<typeof useProviderChangeScope>; applies: boolean; disabled?: boolean;
}) {
  useLanguage();
  return <div className="provider-change-scope" aria-label={t("共享网关影响范围")}>
    <strong>{t(applies ? "共享网关影响范围" : "仅保存此配置")}</strong>
    {!applies ? <p>{t("此供应商未启用或尚未设为当前。保存不会改变当前连接，也不会开启客户端接管。")}</p> : <>
      <p>{t("当前供应商由共享网关使用。变更会同步以下已接管客户端，不会开启新的接管；客户端配置更新后请重启相关客户端。")}</p>
      {scope.loading ? <p role="status">{t("正在读取影响范围…")}</p> : scope.error ? <><InlineError message={scope.error} /><Button variant="secondary" size="sm" disabled={disabled} onClick={() => void scope.refresh()}>{t("重试读取范围")}</Button></> : <>
        {scope.affected.length ? <ul>{scope.affected.map(label => <li key={label}>{label}</li>)}</ul> : <p>{t("没有已接管客户端。直接连接共享网关的请求仍会使用当前配置。")}</p>}
      </>}
    </>}
  </div>;
}

export function ProviderSwitchConfirmation({ provider, controller: c, onClose }: {
  provider: Provider; controller: ProvidersController; onClose: () => void;
}) {
  useLanguage();
  const scope = useProviderChangeScope(true, provider.id);
  const [submitting, setSubmitting] = useState(false);
  const [attempted, setAttempted] = useState(false);
  const [error, setError] = useState("");
  const lock = useRef(false);
  const cancelRef = useRef<HTMLButtonElement>(null);
  const busy = submitting || c.busy || Boolean(c.testing);
  const blocked = busy || c.loading || Boolean(c.readError) || !scope.ready || !provider.enabled || provider.id === c.currentId;
  useEffect(() => { if (!busy && provider.id === c.currentId) onClose(); }, [busy, c.currentId, provider.id, onClose]);
  const confirm = async () => {
    if (blocked || lock.current) return;
    lock.current = true; setSubmitting(true); setAttempted(true); setError("");
    try { await c.activate(provider.id); }
    catch (cause) { setError(errorText(cause)); }
    finally { lock.current = false; setSubmitting(false); }
  };
  return <Modal open size="sm" title={t("切换当前供应商")} description={t("确认后，共享网关将使用所选供应商及其默认模型。")}
    busy={busy} initialFocusRef={cancelRef} onClose={() => { if (!busy && !lock.current) onClose(); }} footer={<>
      <Button ref={cancelRef} variant="secondary" disabled={busy} onClick={onClose}>{t("取消")}</Button>
      <Button disabled={blocked} loading={submitting} onClick={() => void confirm()}>{t("确认切换")}</Button>
    </>}>
    <dl className="provider-change-target"><div><dt>{t("供应商")}</dt><dd>{provider.name}</dd></div><div><dt>{t("默认模型")}</dt><dd className="mono">{provider.model || t("未配置")}</dd></div><div><dt>{t("端点")}</dt><dd className="mono">{provider.endpoint}</dd></div></dl>
    <ProviderChangeScope scope={scope} applies disabled={busy} />
    {!provider.enabled && <InlineError message={t("供应商已禁用，请先在配置中启用。")} />}
    <InlineError message={c.readError || error || (attempted ? c.error : "")} />
  </Modal>;
}
