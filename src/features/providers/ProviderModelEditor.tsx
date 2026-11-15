import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { Search } from "lucide-react";
import { t, useLanguage } from "../../i18n/language";
import { Button, Field, InlineError, Input, Modal, Select } from "../../ui";
import { ConfirmActionDialog } from "../../components/ConfirmActionDialog";
import { discoverProviderModels, type DiscoveredModel } from "../../lib/model-service";
import { refreshCodexProvider, saveProvider } from "../../lib/provider-service";
import type { Provider } from "../../types/provider";
import { TaskNavigation, useTaskDraft } from "../settings/TaskNavigation";
import type { ProviderNavigationGuard } from "./ProviderEditor";
import { ProviderChangeScope, useProviderChangeScope } from "./ProviderChangeScope";
import { errorText } from "./useProviders";

export async function synchronizeProviderModel(provider: Provider, onRuntimeChanged: () => Promise<void>) {
  if (provider.enabled) await refreshCodexProvider(provider.id);
  await onRuntimeChanged();
}

export async function updateProviderDefaultModel(provider: Provider, model: string, onPersisted: (provider: Provider) => void, onRuntimeChanged: () => Promise<void>) {
  const modelId = model.trim();
  if (!modelId) throw new Error(t("请填写默认模型 ID。"));
  const next = { ...provider, model: modelId };
  await saveProvider(next);
  onPersisted(next);
  await synchronizeProviderModel(next, onRuntimeChanged);
  return next;
}

interface Props {
  provider: Provider; current: boolean; disabled?: boolean; readError?: string;
  onSaved: (provider: Provider) => void; onRuntimeChanged: () => Promise<void>; onClose: () => void;
  onNavigationGuardChange?: (guard: ProviderNavigationGuard | null) => void;
}

export function ProviderModelEditor(props: Props) {
  return <TaskNavigation onLeaveApproved={() => props.onNavigationGuardChange?.(null)}>{() => <ProviderModelForm {...props} />}</TaskNavigation>;
}

function ProviderModelForm({ provider, current, disabled = false, readError = "", onSaved, onRuntimeChanged, onClose, onNavigationGuardChange }: Props) {
  useLanguage();
  const [model, setModel] = useState(provider.model);
  const [models, setModels] = useState<DiscoveredModel[]>([]);
  const [modelBusy, setModelBusy] = useState(false);
  const [saving, setSaving] = useState(false);
  const [pendingSync, setPendingSync] = useState<Provider | null>(null);
  const [error, setError] = useState("");
  const [discardOpen, setDiscardOpen] = useState(false);
  const baseline = useRef(provider.model);
  const pendingLeave = useRef<(() => void) | null>(null);
  const lock = useRef(false);
  const inputRef = useRef<HTMLInputElement>(null);
  const errorRef = useRef<HTMLDivElement>(null);
  const applies = current && provider.enabled;
  const scope = useProviderChangeScope(applies, provider.id);
  const busy = disabled || saving || modelBusy;
  const dirty = model !== baseline.current;
  const needsGuard = dirty || Boolean(pendingSync);
  useTaskDraft(needsGuard, busy);
  const requestLeave = useCallback<ProviderNavigationGuard>(proceed => {
    if (busy || lock.current) return;
    if (needsGuard) { pendingLeave.current = proceed; setDiscardOpen(true); }
    else { onNavigationGuardChange?.(null); proceed(); }
  }, [busy, needsGuard, onNavigationGuardChange]);
  useLayoutEffect(() => {
    onNavigationGuardChange?.(requestLeave);
    return () => onNavigationGuardChange?.(null);
  }, [onNavigationGuardChange, requestLeave]);
  useEffect(() => { if (error) errorRef.current?.focus(); }, [error]);
  const choices = useMemo(() => {
    const options = new Map<string, string>();
    if (provider.model) options.set(provider.model, provider.model);
    provider.advanced.modelMappings.forEach(mapping => { if (mapping.model) options.set(mapping.model, mapping.displayName || mapping.model); });
    models.forEach(item => options.set(item.id, item.name || item.id));
    return [...options].map(([id, name]) => ({ id, name }));
  }, [provider.model, provider.advanced.modelMappings, models]);
  const discover = async () => {
    if (busy || readError || lock.current) return;
    lock.current = true; setModelBusy(true); setError("");
    try {
      const result = await discoverProviderModels({ providerId: provider.id, providerType: provider.providerType || "custom", upstreamFormat: provider.advanced.upstreamFormat, endpoint: provider.endpoint });
      setModels(result);
      if (!result.length) setError(t("探索完成，但上游未返回可用模型。可手动填写模型 ID。"));
    } catch (cause) { setError(t("模型探索失败：") + errorText(cause)); }
    finally { lock.current = false; setModelBusy(false); }
  };
  const save = async () => {
    if (busy || readError || lock.current || !scope.ready) return;
    if (!model.trim()) { setError(t("请填写默认模型 ID。")); inputRef.current?.focus(); return; }
    if (!dirty && !pendingSync) return;
    lock.current = true; setSaving(true); setError("");
    let persisted: Provider | null = null;
    try {
      if (pendingSync && !dirty) { persisted = pendingSync; await synchronizeProviderModel(pendingSync, onRuntimeChanged); }
      else await updateProviderDefaultModel(provider, model, next => {
        persisted = next; baseline.current = next.model; setModel(next.model); setPendingSync(next); onSaved(next);
      }, onRuntimeChanged);
      setPendingSync(null); onNavigationGuardChange?.(null); onClose();
    } catch (cause) {
      setError((persisted ? t("默认模型已保存，客户端同步或状态刷新失败：") : t("默认模型保存失败：")) + errorText(cause));
    } finally { lock.current = false; setSaving(false); }
  };
  return <>
    <Modal open size="sm" className="provider-model-editor" title={t("修改默认模型")} description={provider.name} initialFocusRef={inputRef} busy={busy}
      onClose={() => requestLeave(onClose)} footer={<>
        <Button variant="secondary" disabled={busy} onClick={() => requestLeave(onClose)}>{t("取消")}</Button>
        <Button type="submit" form="provider-default-model-form" loading={saving} disabled={busy || Boolean(readError) || !scope.ready || (!dirty && !pendingSync)}>{t(pendingSync && !dirty ? "重试同步" : "保存模型")}</Button>
      </>}>
      <form id="provider-default-model-form" className="form-stack" onSubmit={event => { event.preventDefault(); void save(); }}>
        <Field label={t("默认模型 ID")} required description={t("默认模型独立于模型映射；已有映射和请求配置会保留。") }><Input ref={inputRef} className="mono" value={model} disabled={busy || Boolean(readError)} onChange={event => setModel(event.target.value)} placeholder={t("填写上游支持的模型 ID")} /></Field>
        <div className="provider-model-discovery"><Button variant="secondary" size="sm" icon={<Search size={14} />} disabled={busy || Boolean(readError)} loading={modelBusy} onClick={() => void discover()}>{t("探索可用模型")}</Button>{choices.length > 0 && <Select aria-label={t("选择已配置或探索到的模型")} value={choices.some(item => item.id === model) ? model : ""} disabled={busy || Boolean(readError)} onChange={event => { if (event.target.value) setModel(event.target.value); }}><option value="">{t("选择模型")}</option>{choices.map(item => <option key={item.id} value={item.id}>{item.name === item.id ? item.id : item.name + " · " + item.id}</option>)}</Select>}</div>
        <p className="page-description">{t("模型探索使用已保存的凭据；此处不读取、修改或提交 API Key。")}</p>
        <ProviderChangeScope scope={scope} applies={applies} disabled={busy} />
        <div ref={errorRef} tabIndex={-1}><InlineError message={readError || error} />{pendingSync && <p role="status">{t("配置已保存。可重试同步，或关闭后在连接诊断中检查。")}</p>}</div>
      </form>
    </Modal>
    {discardOpen && <ConfirmActionDialog title={t(dirty ? "放弃未保存的修改？" : "离开模型编辑？")}
      message={t(pendingSync ? "默认模型已保存，但同步尚未完成。离开不会撤销已保存的配置。" : "未保存的默认模型修改将丢弃。")}
      confirmLabel={t(dirty ? "放弃修改" : "离开")} busy={busy} onCancel={() => { pendingLeave.current = null; setDiscardOpen(false); }} onConfirm={() => {
        if (busy || lock.current) return;
        const proceed = pendingLeave.current; pendingLeave.current = null; onNavigationGuardChange?.(null); setDiscardOpen(false); proceed?.();
      }} />}
  </>;
}
