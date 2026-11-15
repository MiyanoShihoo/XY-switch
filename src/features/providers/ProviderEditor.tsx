import { useLanguage, t } from "../../i18n/language";
import { useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";
import { ArrowLeft, KeyRound, Search } from "lucide-react";
import { Button, Field, Input, Select, Toggle, InlineError, Modal } from "../../ui";
import { ConfirmActionDialog } from "../../components/ConfirmActionDialog";
import { deleteProviderApiKey, hasProviderApiKey, setProviderApiKey } from "../../lib/credential-service";
import { discoverProviderModels, type DiscoveredModel } from "../../lib/model-service";
import { formatJsonObject, parseBodyOverrides, parseHeaderOverrides, validateProviderAdvanced } from "../../lib/provider-advanced";
import { validateApiKey, validateProvider } from "../../lib/provider-validation";
import { createProviderAdvanced, DEFAULT_PROVIDER_ICON, inferProviderIcon } from "../../lib/providers";
import { refreshCodexProvider, saveProvider } from "../../lib/provider-service";
import { presetToProviderDraft } from "../../lib/provider-presets";
import { providerPresets } from "../../config/providerPresets";
import type { Provider, UpstreamFormat } from "../../types/provider";
import { initialProviderSections, ProviderSectionDisclosure, providerSections, providerSectionSummary, type ProviderAdvancedSection, type ProviderSection } from "./ProviderSectionNavigation";
import { ProviderAdvancedEditor } from "./ProviderAdvancedEditor";
import { errorText } from "./useProviders";
import { TaskNavigation, useTaskDraft } from "../settings/TaskNavigation";
export type ProviderDraft = Pick<Provider, "id" | "name" | "group" | "endpoint" | "model" | "enabled" | "advanced" | "icon" | "providerType">;
export function emptyProviderDraft(): ProviderDraft {
  return { id: "", name: "", group: "Codex", endpoint: "https://", model: "", enabled: true, advanced: createProviderAdvanced(), icon: DEFAULT_PROVIDER_ICON, providerType: "custom" };
}
export type ProviderNavigationGuard = (proceed: () => void) => void;
interface Props { onNavigationGuardChange?: (guard: ProviderNavigationGuard | null) => void; provider: Provider | null; providers: Provider[]; initialSection?: ProviderSection; disabled?: boolean; readError?: string; onClose: (savedProviderId?: string) => void; onSaved: (provider: Provider) => void; onRuntimeChanged: () => Promise<void>; }
export function ProviderEditor(props: Props) {
  return <TaskNavigation onLeaveApproved={() => props.onNavigationGuardChange?.(null)}>{() => <ProviderEditorForm {...props} />}</TaskNavigation>;
}
function ProviderEditorForm({ provider, providers, initialSection = "connection", disabled = false, readError = "", onClose, onSaved, onRuntimeChanged, onNavigationGuardChange }: Props) {
  useLanguage();
  const [form, setForm] = useState<ProviderDraft>(() => provider ? { ...provider, advanced: structuredClone(provider.advanced) } : emptyProviderDraft());
  const [savedId, setSavedId] = useState(provider?.id ?? "");
  const [key, setKey] = useState("");
  const [keyConfigured, setKeyConfigured] = useState(false);
  const [keyBusy, setKeyBusy] = useState(Boolean(provider));
  const [keyError, setKeyError] = useState("");
  const [keyReadVersion, setKeyReadVersion] = useState(0);
  const [saving, setSaving] = useState(false);
  const [models, setModels] = useState<DiscoveredModel[]>([]);
  const [modelBusy, setModelBusy] = useState(false);
  const [headers, setHeaders] = useState(() => formatJsonObject(form.advanced.headerOverrides));
  const [body, setBody] = useState(() => formatJsonObject(form.advanced.bodyOverrides));
  const [error, setError] = useState("");
  const [clearOpen, setClearOpen] = useState(false);
  const [clearError, setClearError] = useState("");
  const [openSections, setOpenSections] = useState(() => initialProviderSections(initialSection));
  const [focusSection, setFocusSection] = useState<ProviderSection | null>(initialSection === "advanced" ? "request" : initialSection);
  const [discardOpen, setDiscardOpen] = useState(false);
  const pendingLeave = useRef<(() => void) | null>(null);
  const baseline = useRef(JSON.stringify([form, headers, body]));
  const dirty = Boolean(key) || JSON.stringify([form, headers, body]) !== baseline.current;
  const errorRef = useRef<HTMLDivElement>(null);
  const clearCancelRef = useRef<HTMLButtonElement>(null);
  const lock = useRef(false);
  const nameRef = useRef<HTMLInputElement>(null);
  const busy = disabled || saving || keyBusy || modelBusy;
  const saveBlocked = busy || Boolean(readError) || Boolean(keyError);
  useTaskDraft(dirty, busy);
  const revealSection = (next: ProviderSection) => {
    setOpenSections(value => ({ ...value, ...initialProviderSections(next) }));
    setFocusSection(next === "advanced" ? "request" : next);
  };
  const requestLeave = useCallback<ProviderNavigationGuard>(proceed => {
    if (busy || lock.current) return;
    if (dirty) { pendingLeave.current = proceed; setDiscardOpen(true); }
    else { onNavigationGuardChange?.(null); setKey(""); proceed(); }
  }, [busy, dirty, onNavigationGuardChange]);
  const close = () => requestLeave(() => onClose(savedId || undefined));
  useLayoutEffect(() => {
    onNavigationGuardChange?.(requestLeave);
    return () => onNavigationGuardChange?.(null);
  }, [onNavigationGuardChange, requestLeave]);
  useLayoutEffect(() => {
    revealSection(initialSection);
  }, [initialSection]);
  useLayoutEffect(() => {
    if (!focusSection) return;
    const heading = document.getElementById("provider-edit-" + focusSection);
    heading?.focus({ preventScroll: true }); heading?.scrollIntoView({ block: "start" });
    setFocusSection(null);
  }, [focusSection, openSections]);
  useEffect(() => { if (error) errorRef.current?.focus({ preventScroll: true }); }, [error]);
  useEffect(() => {
    if (!provider) return;
    let cancelled = false;
    setKeyBusy(true); setKeyError("");
    void hasProviderApiKey(provider.id).then(value => { if (!cancelled) setKeyConfigured(value); }).catch(e => { if (!cancelled) setKeyError(t("凭据状态读取失败：") + errorText(e)); }).finally(() => { if (!cancelled) setKeyBusy(false); });
    return () => { cancelled = true; };
  }, [provider?.id, keyReadVersion]);
  const patch = (change: Partial<ProviderDraft>) => setForm(value => ({ ...value, ...change }));
  const discover = async () => {
    if (saveBlocked || lock.current) return;
    if (!form.id.trim() || !form.endpoint.trim()) return setError(t("请先填写供应商 ID 和 API 地址。"));
    if (!key.trim() && !keyConfigured) return setError(t("请先输入或保存 API Key。"));
    lock.current = true; setModelBusy(true); setError("");
    try {
      const result = await discoverProviderModels({ providerId: form.id.trim(), providerType: form.providerType ?? "custom", upstreamFormat: form.advanced.upstreamFormat, endpoint: form.endpoint.trim(), apiKey: key.trim() || undefined });
      setModels(result);
      if (!form.model.trim() && result[0]) patch({ model: result[0].id });
      if (!result.length) setError(t("探索完成，但上游未返回可用模型。可手动填写模型 ID。"));
    } catch (e) { setError(t("模型探索失败：") + errorText(e)); }
    finally { lock.current = false; setModelBusy(false); }
  };
  const save = async () => {
    if (saveBlocked || lock.current) return;
    const validation = validateProvider(form, providers, savedId || null) || validateApiKey(key);
    if (validation) { revealSection("connection"); return setError(validation); }
    let advanced = form.advanced;
    try { advanced = { ...advanced, headerOverrides: parseHeaderOverrides(headers) }; }
    catch (e) { revealSection("headers"); return setError(errorText(e)); }
    try { advanced = { ...advanced, bodyOverrides: parseBodyOverrides(body) }; }
    catch (e) { revealSection("body"); return setError(errorText(e)); }
    const advancedError = validateProviderAdvanced(advanced);
    if (advancedError) {
      const defaults = createProviderAdvanced();
      const tasks: [ProviderAdvancedSection, Provider["advanced"]][] = [["request", { ...defaults, customUserAgent: advanced.customUserAgent }], ["models", { ...defaults, modelMappings: advanced.modelMappings }], ["codex", { ...defaults, codex: advanced.codex }]];
      revealSection(tasks.find(([, value]) => validateProviderAdvanced(value))?.[0] ?? "request");
      return setError(advancedError);
    }
    const next: Provider = {
      ...provider, ...form, id: form.id.trim(), name: form.name.trim(), endpoint: form.advanced.isFullUrl ? form.endpoint.trim() : form.endpoint.trim().replace(/\/$/, ""), model: form.model.trim(), advanced,
      icon: form.icon !== DEFAULT_PROVIDER_ICON ? form.icon : inferProviderIcon(form.id, form.name) ?? provider?.icon ?? DEFAULT_PROVIDER_ICON,
      builtIn: provider?.builtIn ?? false, providerType: form.providerType || "custom", sortOrder: provider?.sortOrder ?? providers.length + 100,
    };
    lock.current = true; setSaving(true); setError("");
    let configurationSaved = false;
    try {
      await saveProvider(next); configurationSaved = true; setSavedId(next.id); baseline.current = JSON.stringify([form, headers, body]); onSaved(next);
      if (key.trim()) { await setProviderApiKey(next.id, key.trim()); setKeyConfigured(true); setKey(""); }
      if (next.enabled) await refreshCodexProvider(next.id);
      await onRuntimeChanged();
      onNavigationGuardChange?.(null);
      onClose(next.id);
    } catch (e) { setError((configurationSaved ? t("供应商配置已保存，后续凭据或同步操作失败：") : t("保存失败：")) + errorText(e)); }
    finally { lock.current = false; setSaving(false); }
  };
  const clearKey = async () => {
    if (!savedId || saveBlocked || lock.current) return;
    lock.current = true; setKeyBusy(true); setClearError("");
    try { await deleteProviderApiKey(savedId); setKey(""); setKeyConfigured(false); setClearOpen(false); }
    catch (e) { setClearError(t("凭据删除失败：") + errorText(e)); }
    finally { lock.current = false; setKeyBusy(false); }
  };
  const advancedProps = { value: form.advanced, onChange: (advanced: Provider["advanced"]) => patch({ advanced }), headerText: headers, bodyText: body, onHeader: setHeaders, onBody: setBody, models, onDiscover: () => void discover(), modelBusy, disabled: busy || Boolean(readError), apiKeyConfigured: !keyError && (keyConfigured || Boolean(key.trim())) };
  return <>
    <div className="workspace-page provider-editor-page" aria-busy={busy}>
      <Button className="provider-back" variant="quiet" icon={<ArrowLeft size={16} />} disabled={busy} onClick={close}>{t(savedId ? "返回配置" : "返回供应商")}</Button>
      <header className="page-heading"><div className="page-heading__copy"><span className="page-eyebrow">{t(savedId ? "编辑供应商" : "添加供应商")}</span><h1>{form.name || t("添加供应商")}</h1><p className="page-description">{t("先配置连接与默认模型，再按需要展开更多选项。")}</p></div></header>
      <form className="provider-editor-form" onSubmit={event => { event.preventDefault(); void save(); }}>
        {!savedId && <div className="provider-preset"><label htmlFor="provider-preset">{t("快速预设")}</label><Select id="provider-preset" aria-describedby="provider-preset-hint" value="" disabled={busy} onChange={event => { const preset = providerPresets.find(item => item.id === event.target.value); if (preset) { setForm(preset.id === "custom" ? emptyProviderDraft() : presetToProviderDraft(preset, providers.map(item => item.id))); setHeaders(""); setBody(""); setModels([]); setError(""); } }}><option value="" disabled>{t("选择预设")}</option>{providerPresets.map(preset => <option key={preset.id} value={preset.id}>{t(preset.name)}</option>)}</Select><span id="provider-preset-hint">{t("选择后填入端点、模型和协议，可继续修改。")}</span></div>}
        <section className="provider-connection-editor" aria-labelledby="provider-edit-connection">
            <div className="provider-editor-section-heading"><h2 id="provider-edit-connection" tabIndex={-1}>{t("连接与凭据")}</h2><div className="provider-editor-enabled"><span>{t("启用供应商")}</span><Toggle aria-label={t("启用供应商")} aria-describedby="provider-enabled-hint" checked={form.enabled} disabled={busy || Boolean(readError)} onCheckedChange={enabled => patch({ enabled })} /></div></div>
            <div className="form-grid">
              <Field label={t("供应商名称")} required><Input ref={nameRef} value={form.name} disabled={busy || Boolean(readError)} placeholder={t("例如：团队 API")} onChange={event => patch({ name: event.target.value })} /></Field>
              <Field label={t("供应商 ID")} required description={t("唯一标识，只能包含字母、数字、下划线和短横线；保存后不可修改。")}><Input className="mono" value={form.id} disabled={busy || Boolean(savedId) || Boolean(readError)} onChange={event => patch({ id: event.target.value })} placeholder="team-provider" /></Field>
              <Field className="span-two" label={t("API 地址")} required description={t(form.advanced.isFullUrl ? "直接使用完整地址，不追加协议路径。" : "按所选上游协议自动追加请求路径。") }><Input className="mono" value={form.endpoint} disabled={busy || Boolean(readError)} onChange={event => patch({ endpoint: event.target.value })} /></Field>
              <div className="span-two provider-url-mode"><span>{t("完整请求 URL")}</span><Toggle aria-label={t("使用完整请求 URL")} checked={form.advanced.isFullUrl ?? false} disabled={busy || Boolean(readError)} onCheckedChange={isFullUrl => patch({ advanced: { ...form.advanced, isFullUrl } })} /></div>
              <Field label={t("默认模型 ID")} required><Input className="mono" value={form.model} disabled={busy || Boolean(readError)} onChange={event => patch({ model: event.target.value })} placeholder={t("填写上游支持的模型 ID")} /></Field>
              <Field label={t("上游协议")}><Select value={form.advanced.upstreamFormat} disabled={busy || Boolean(readError)} onChange={event => patch({ advanced: { ...form.advanced, upstreamFormat: event.target.value as UpstreamFormat } })}><option value="openai_responses">{t("OpenAI Responses（原生）")}</option><option value="openai_chat">OpenAI Chat Completions</option><option value="anthropic">Anthropic Messages</option></Select></Field>
              <div className="span-two provider-model-discovery"><Button variant="secondary" size="sm" icon={<Search size={14} />} disabled={saveBlocked} loading={modelBusy} onClick={() => void discover()}>{t("探索可用模型")}</Button>{models.length > 0 && <Select disabled={saveBlocked} aria-label={t("选择探索模型")} value={models.some(model => model.id === form.model) ? form.model : ""} onChange={event => { if (event.target.value) patch({ model: event.target.value }); }}><option value="">{t("选择模型（")}{models.length} {t("项）")}</option>{models.map(model => <option key={model.id} value={model.id}>{model.name === model.id ? model.id : model.name + " · " + model.id}</option>)}</Select>}</div>
              <Field className="span-two" label="API Key" description={keyBusy ? t("正在读取凭据状态…") : keyError ? t("凭据状态未知，请重试读取后保存。") : keyConfigured ? t("已安全配置；留空保留现有密钥，输入新值替换。") : t("密钥不会写入供应商 JSON、日志或安全预览。")}><Input type="password" autoComplete="new-password" leading={<KeyRound size={14} />} value={key} disabled={busy || Boolean(readError)} onChange={event => setKey(event.target.value)} placeholder={keyConfigured ? t("留空保留已保存密钥") : t("输入 API Key")} /></Field>
              {keyError && <div className="span-two"><InlineError message={keyError} /><Button variant="secondary" size="sm" disabled={busy} onClick={() => setKeyReadVersion(value => value + 1)}>{t("重试读取凭据状态")}</Button></div>}
              {savedId && keyConfigured && <div className="span-two"><Button variant="danger" size="sm" disabled={saveBlocked} onClick={() => setClearOpen(true)}>{t("清除已保存密钥")}</Button></div>}
            </div>
            <p className="provider-editor-enabled-note" id="provider-enabled-hint">{t("启用后才可设为当前；这不是连接健康状态。")}</p>
        </section>
        <div className="provider-task-list" aria-label={t("可选配置")}>{providerSections.map(task => <ProviderSectionDisclosure key={task.id} id={"provider-edit-" + task.id} title={t(task.label)} summary={(task.id === "headers" && headers !== formatJsonObject(form.advanced.headerOverrides)) || (task.id === "body" && body !== formatJsonObject(form.advanced.bodyOverrides)) ? t("有未保存的修改") : providerSectionSummary(form.advanced, task.id)} open={Boolean(openSections[task.id])} onOpenChange={open => setOpenSections(value => ({ ...value, [task.id]: open }))}>
          <ProviderAdvancedEditor section={task.id} {...advancedProps} />
          {task.id === "request" && <dl className="provider-editor-metadata"><div><dt>{t("记录分组")}</dt><dd>{form.group}</dd></div><div><dt>{t("供应商类型")}</dt><dd className="mono">{form.providerType || "custom"}</dd></div></dl>}
        </ProviderSectionDisclosure>)}</div>
        <aside className="provider-editor-impact" aria-label={t("保存影响")}><p>{t("保存启用的当前供应商会同步所有已接管客户端；不会开启新的接管。客户端配置更新后请重启相关客户端。")}</p></aside>
        <footer className="provider-editor-savebar">
          <div className="provider-editor-savebar__feedback" ref={errorRef} tabIndex={-1}><InlineError message={readError || error} /><p role="status">{t(dirty ? "有未保存的修改" : savedId ? "没有未保存的修改" : "配置尚未保存")}</p><span>{t("保存不会自动将新供应商设为当前。")}</span></div>
          <div className="page-actions"><Button variant="secondary" disabled={busy} onClick={close}>{t("取消")}</Button><Button type="submit" loading={saving} disabled={saveBlocked}>{t("保存配置")}</Button></div>
        </footer>
      </form>
    </div>
    {discardOpen && <ConfirmActionDialog title={t("放弃未保存的修改？")} message={t("未保存的配置修改和输入的密钥将清除；已完成的保存或凭据清除不会撤销。")} confirmLabel={t("放弃修改")} busy={busy} onCancel={() => { pendingLeave.current = null; setDiscardOpen(false); }} onConfirm={() => { if (!busy && !lock.current) { const proceed = pendingLeave.current; pendingLeave.current = null; onNavigationGuardChange?.(null); setKey(""); setDiscardOpen(false); proceed?.(); } }} />}
    {clearOpen && <Modal open size="sm" role="alertdialog" hideClose title={t("清除 API Key")} description={t("仅删除此供应商已保存的安全凭据。后续请求可能因缺少密钥失败，供应商配置会保留。")} initialFocusRef={clearCancelRef} busy={busy} onClose={() => { if (!busy) setClearOpen(false); }} footer={<>
      <Button ref={clearCancelRef} variant="secondary" disabled={busy} onClick={() => setClearOpen(false)}>{t("取消")}</Button>
      <Button variant="danger" disabled={saveBlocked} onClick={() => void clearKey()}>{t("确认清除")}</Button>
    </>}><InlineError message={clearError || readError || keyError} /></Modal>}
  </>;
}
