import { useLanguage, t } from "../../i18n/language";
import { useEffect, useRef, useState } from "react";
import { KeyRound, Search } from "lucide-react";
import { Button, Field, Input, Modal, Select, Toggle, InlineError } from "../../ui";
import { ConfirmActionDialog } from "../../components/ConfirmActionDialog";
import { deleteProviderApiKey, hasProviderApiKey, setProviderApiKey } from "../../lib/credential-service";
import { discoverProviderModels, type DiscoveredModel } from "../../lib/model-service";
import { formatJsonObject, parseBodyOverrides, parseHeaderOverrides, validateProviderAdvanced } from "../../lib/provider-advanced";
import { validateApiKey, validateProvider } from "../../lib/provider-validation";
import { createProviderAdvanced, DEFAULT_PROVIDER_ICON, inferProviderIcon } from "../../lib/providers";
import { refreshCodexProvider, saveProvider } from "../../lib/provider-service";
import { presetToProviderDraft } from "../../lib/provider-presets";
import { providerPresets } from "../../config/providerPresets";
import type { Provider } from "../../types/provider";
import { ProviderAdvancedEditor } from "./ProviderAdvancedEditor";
import { errorText } from "./useProviders";
export type ProviderDraft = Pick<Provider, "id" | "name" | "group" | "endpoint" | "model" | "enabled" | "advanced" | "icon" | "providerType">;
export function emptyProviderDraft(): ProviderDraft {
  return { id: "", name: "", group: "Codex", endpoint: "https://", model: "", enabled: true, advanced: createProviderAdvanced(), icon: DEFAULT_PROVIDER_ICON, providerType: "custom" };
}
interface Props { provider: Provider | null; providers: Provider[]; onClose: () => void; onSaved: (provider: Provider) => void; onRuntimeChanged: () => Promise<void>; }
export function ProviderEditor({ provider, providers, onClose, onSaved, onRuntimeChanged }: Props) {
  useLanguage();
  const [form, setForm] = useState<ProviderDraft>(() => provider ? { ...provider, advanced: structuredClone(provider.advanced) } : emptyProviderDraft());
  const [savedId, setSavedId] = useState(provider?.id ?? "");
  const [key, setKey] = useState("");
  const [keyConfigured, setKeyConfigured] = useState(false);
  const [keyBusy, setKeyBusy] = useState(Boolean(provider));
  const [saving, setSaving] = useState(false);
  const [models, setModels] = useState<DiscoveredModel[]>([]);
  const [modelBusy, setModelBusy] = useState(false);
  const [headers, setHeaders] = useState(() => formatJsonObject(form.advanced.headerOverrides));
  const [body, setBody] = useState(() => formatJsonObject(form.advanced.bodyOverrides));
  const [error, setError] = useState("");
  const [clearOpen, setClearOpen] = useState(false);
  const [clearError, setClearError] = useState("");
  const lock = useRef(false);
  const nameRef = useRef<HTMLInputElement>(null);
  const busy = saving || keyBusy || modelBusy;
  useEffect(() => {
    if (!provider) return;
    let cancelled = false;
    void hasProviderApiKey(provider.id).then(value => { if (!cancelled) setKeyConfigured(value); }).catch(e => { if (!cancelled) setError(t("凭据状态读取失败：") + errorText(e)); }).finally(() => { if (!cancelled) setKeyBusy(false); });
    return () => { cancelled = true; };
  }, [provider]);
  const patch = (change: Partial<ProviderDraft>) => setForm(value => ({ ...value, ...change }));
  const discover = async () => {
    if (busy || lock.current) return;
    if (!form.id.trim() || !form.endpoint.trim()) return setError(t("请先填写供应商 ID 和 API 地址。"));
    if (!key.trim() && !keyConfigured) return setError(t("请先输入或保存 API Key。"));
    setModelBusy(true); setError("");
    try {
      const result = await discoverProviderModels({ providerId: form.id.trim(), providerType: form.providerType ?? "custom", upstreamFormat: form.advanced.upstreamFormat, endpoint: form.endpoint.trim(), apiKey: key.trim() || undefined });
      setModels(result);
      if (!form.model.trim() && result[0]) patch({ model: result[0].id });
      if (!result.length) setError(t("探索完成，但上游未返回可用模型。可手动填写模型 ID。"));
    } catch (e) { setError(t("模型探索失败：") + errorText(e)); }
    finally { setModelBusy(false); }
  };
  const save = async () => {
    if (busy || lock.current) return;
    const validation = validateProvider(form, providers, savedId || null) || validateApiKey(key);
    if (validation) return setError(validation);
    let advanced = form.advanced;
    try { advanced = { ...advanced, headerOverrides: parseHeaderOverrides(headers), bodyOverrides: parseBodyOverrides(body) }; }
    catch (e) { return setError(errorText(e)); }
    const advancedError = validateProviderAdvanced(advanced);
    if (advancedError) return setError(advancedError);
    const next: Provider = {
      ...provider, ...form, id: form.id.trim(), name: form.name.trim(), endpoint: form.endpoint.trim().replace(/\/$/, ""), model: form.model.trim(), advanced,
      icon: form.icon !== DEFAULT_PROVIDER_ICON ? form.icon : inferProviderIcon(form.id, form.name) ?? provider?.icon ?? DEFAULT_PROVIDER_ICON,
      builtIn: provider?.builtIn ?? false, providerType: form.providerType || "custom", sortOrder: provider?.sortOrder ?? providers.length + 100,
    };
    lock.current = true; setSaving(true); setError("");
    let configurationSaved = false;
    try {
      await saveProvider(next); configurationSaved = true; setSavedId(next.id); onSaved(next);
      if (key.trim()) { await setProviderApiKey(next.id, key.trim()); setKeyConfigured(true); setKey(""); }
      if (next.enabled) await refreshCodexProvider(next.id);
      await onRuntimeChanged();
      onClose();
    } catch (e) { setError((configurationSaved ? t("供应商配置已保存，后续凭据或同步操作失败：") : t("保存失败：")) + errorText(e)); }
    finally { lock.current = false; setSaving(false); }
  };
  const clearKey = async () => {
    if (!savedId || keyBusy) return;
    setKeyBusy(true); setClearError("");
    try { await deleteProviderApiKey(savedId); setKey(""); setKeyConfigured(false); setClearOpen(false); }
    catch (e) { setClearError(t("凭据删除失败：") + errorText(e)); }
    finally { setKeyBusy(false); }
  };
  return <>
    <Modal open title={provider ? t("编辑供应商") : t("添加供应商")} description={t("先完成基础连接，再按需配置模型与协议。保存不会自动将新供应商设为当前。")} size="lg" busy={busy} initialFocusRef={nameRef} onClose={onClose} footer={<><span className="provider-editor-save-note">{savedId ? t("ID 已固定") : t("凭据独立安全存储")}</span><Button variant="secondary" disabled={busy} onClick={onClose}>{t("取消")}</Button><Button loading={saving} disabled={keyBusy || modelBusy} onClick={() => void save()}>{t("保存配置")}</Button></>}>
      <form className="form-stack" onSubmit={event => { event.preventDefault(); void save(); }}>
        <InlineError message={error} />
        <div className="form-grid">
          <Field className="span-two" label={t("供应商名称")} required><Input ref={nameRef} value={form.name} disabled={busy} placeholder={t("例如：团队 API")} onChange={event => patch({ name: event.target.value })} /></Field>
          {!savedId && <Field className="span-two" label={t("快速预设")} description={t("选择后填入端点、模型和协议，可继续修改。")}><Select value="" disabled={busy} onChange={event => { const preset = providerPresets.find(p => p.id === event.target.value); if (preset) { setForm(preset.id === "custom" ? emptyProviderDraft() : presetToProviderDraft(preset, providers.map(p => p.id))); setHeaders(""); setBody(""); setModels([]); setError(""); } }}><option value="" disabled>{t("选择预设")}</option>{providerPresets.map(preset => <option key={preset.id} value={preset.id}>{t(preset.name)}</option>)}</Select></Field>}
          <Field className="span-two" label={t("供应商 ID")} required description={t("唯一标识，只能包含字母、数字、下划线和短横线；保存后不可修改。")}><Input className="mono" value={form.id} disabled={busy || Boolean(savedId)} onChange={event => patch({ id: event.target.value })} placeholder="team-provider" /></Field>
          <Field className="span-two" label={t("API 地址")} required description={t("填写 HTTP / HTTPS 地址，协议路径规则在高级配置中设置。")}><Input className="mono" value={form.endpoint} disabled={busy} onChange={event => patch({ endpoint: event.target.value })} /></Field>
          <Field className="span-two" label="API Key" description={keyBusy ? t("正在读取凭据状态…") : keyConfigured ? t("已安全配置；留空保留现有密钥，输入新值替换。") : t("密钥不会写入供应商 JSON、日志或安全预览。")}><Input type="password" autoComplete="new-password" leading={<KeyRound size={14} />} value={key} disabled={busy} onChange={event => setKey(event.target.value)} placeholder={keyConfigured ? t("留空保留已保存密钥") : t("输入 API Key")} /></Field>
          {savedId && keyConfigured && <div className="span-two"><Button variant="danger" size="sm" disabled={busy} onClick={() => setClearOpen(true)}>{t("清除已保存密钥")}</Button></div>}
          <Field className="span-two" label={t("默认模型 ID")} required><Input className="mono" value={form.model} disabled={busy} onChange={event => patch({ model: event.target.value })} placeholder={t("填写上游支持的模型 ID")} /></Field>
        </div>
        <div className="inline-actions"><Button variant="secondary" size="sm" icon={<Search size={14} />} disabled={saving || keyBusy} loading={modelBusy} onClick={() => void discover()}>{t("探索可用模型")}</Button>{models.length > 0 && <Select aria-label={t("选择探索模型")} value={models.some(m => m.id === form.model) ? form.model : ""} onChange={event => { if (event.target.value) patch({ model: event.target.value }); }}><option value="">{t("选择模型（")}{models.length} {t("项）")}</option>{models.map(model => <option key={model.id} value={model.id}>{model.name === model.id ? model.id : model.name + " · " + model.id}</option>)}</Select>}</div>
        <div className="form-switch"><div className="form-switch__copy"><strong>{t("启用供应商")}</strong><small>{t("启用后才可设为当前；这不是连接健康状态。")}</small></div><Toggle aria-label={t("启用供应商")} checked={form.enabled} disabled={busy} onCheckedChange={enabled => patch({ enabled })} /></div>
        <ProviderAdvancedEditor value={form.advanced} onChange={advanced => patch({ advanced })} headerText={headers} bodyText={body} onHeader={setHeaders} onBody={setBody} models={models} onDiscover={() => void discover()} modelBusy={modelBusy} disabled={busy} apiKeyConfigured={keyConfigured || Boolean(key.trim())} />
        <button type="submit" className="sr-only" tabIndex={-1} disabled={busy}>{t("保存配置")}</button>
      </form>
    </Modal>
    {clearOpen && <ConfirmActionDialog title={t("清除 API Key")} message={t("仅删除此供应商已保存的安全凭据。后续请求可能因缺少密钥失败，供应商配置会保留。")} confirmLabel={t("确认清除")} busy={keyBusy} error={clearError} onCancel={() => setClearOpen(false)} onConfirm={() => void clearKey()} />}
  </>;
}
