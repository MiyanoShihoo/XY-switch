import { useLanguage, t } from "../../i18n/language";
import { useState } from "react";
import { ArrowDown, ArrowUp, Download, Plus, Search, Trash2 } from "lucide-react";
import { Button, Field, IconButton, Input, Select, Textarea, Toggle } from "../../ui";
import type { DiscoveredModel } from "../../lib/model-service";
import { addDiscoveredModelMapping, mergeDiscoveredModelMappings, reorderModelMappings } from "../../lib/model-selection";
import type { ProviderAdvancedConfig, ProviderModelMapping, UpstreamFormat } from "../../types/provider";
interface Props { value: ProviderAdvancedConfig; onChange: (value: ProviderAdvancedConfig) => void; headerText: string; bodyText: string; onHeader: (text: string) => void; onBody: (text: string) => void; models: DiscoveredModel[]; onDiscover: () => void; modelBusy: boolean; disabled: boolean; apiKeyConfigured: boolean; }
const agentPresets = ["claude-cli/2.1.161 (external, cli)", "claude-code/1.0.0", "Kilo-Code/1.0"];
export function ProviderAdvancedEditor({ value, onChange, headerText, bodyText, onHeader, onBody, models, onDiscover, modelBusy, disabled, apiKeyConfigured }: Props) {
  useLanguage();
  const [dragged, setDragged] = useState<number | null>(null);
  const patch = (index: number, change: Partial<ProviderModelMapping>) => onChange({ ...value, modelMappings: value.modelMappings.map((mapping, i) => i === index ? { ...mapping, ...change } : mapping) });
  const move = (from: number, to: number) => onChange({ ...value, modelMappings: reorderModelMappings(value.modelMappings, from, to) });
  const setCodex = (change: Partial<ProviderAdvancedConfig["codex"]>) => onChange({ ...value, codex: { ...value.codex, ...change } });
  return <>
    <details className="details-group" open><summary>{t("协议与请求覆盖")}</summary><div className="details-group__body">
      <Field label={t("上游协议")}><Select value={value.upstreamFormat} disabled={disabled} onChange={event => onChange({ ...value, upstreamFormat: event.target.value as UpstreamFormat })}><option value="openai_responses">{t("OpenAI Responses（原生）")}</option><option value="openai_chat">OpenAI Chat Completions</option><option value="anthropic">Anthropic Messages</option></Select></Field>
      <div className="form-switch"><div className="form-switch__copy"><strong>{t("完整请求 URL")}</strong><small>{t("直接使用端点地址，不追加协议路径。")}</small></div><Toggle aria-label={t("使用完整请求 URL")} checked={value.isFullUrl ?? false} disabled={disabled} onCheckedChange={checked => onChange({ ...value, isFullUrl: checked })} /></div>
      <Field label="User-Agent" description={t("留空时由代理管理；不允许控制字符。")}><Input value={value.customUserAgent} disabled={disabled} list="provider-user-agents" onChange={event => onChange({ ...value, customUserAgent: event.target.value })} /></Field><datalist id="provider-user-agents">{agentPresets.map(agent => <option value={agent} key={agent} />)}</datalist>
      <Field label={t("Header 覆盖（JSON）")} description={t("鉴权及连接控制 Header 由代理安全管理。")}><Textarea className="mono" value={headerText} disabled={disabled} placeholder={'{ "X-Provider": "xy-switch" }'} onChange={event => onHeader(event.target.value)} /></Field>
      <Field label={t("Body 覆盖（JSON）")} description={t("不能覆盖 model、stream 或写入敏感字段。")}><Textarea className="mono" value={bodyText} disabled={disabled} placeholder={'{ "temperature": 0.2 }'} onChange={event => onBody(event.target.value)} /></Field>
    </div></details>
    <details className="details-group"><summary>{t("模型映射")} <span className="muted">{value.modelMappings.length} {t("项")}</span></summary><div className="details-group__body">
      <p className="page-description">{t("第一行为默认映射。可拖动行或使用上移、下移按钮调整顺序。")}</p>
      <div className="model-mapping-list">{value.modelMappings.map((mapping, index) => <div className="model-mapping-row" key={index} draggable={!disabled} onDragStart={() => setDragged(index)} onDragEnd={() => setDragged(null)} onDragOver={event => { if (!disabled) event.preventDefault(); }} onDrop={event => { event.preventDefault(); if (!disabled && dragged !== null) move(dragged, index); setDragged(null); }}>
        <span className="model-mapping-index mono">{index + 1}</span>
        <Field label={t("模型 ID")}><Input className="mono" value={mapping.model} disabled={disabled} onChange={event => patch(index, { model: event.target.value })} /></Field>
        <Field label={t("显示名称")}><Input value={mapping.displayName} disabled={disabled} onChange={event => patch(index, { displayName: event.target.value })} /></Field>
        <Field label={t("上下文窗口")}><Input type="number" min={1} value={mapping.contextWindow ?? ""} placeholder={t("自动")} disabled={disabled} onChange={event => patch(index, { contextWindow: event.target.value ? Number(event.target.value) : null })} /></Field>
        <div className="model-mapping-actions"><IconButton aria-label={t("上移模型 ") + (index + 1)} disabled={disabled || index === 0} onClick={() => move(index, index - 1)}><ArrowUp size={14} /></IconButton><IconButton aria-label={t("下移模型 ") + (index + 1)} disabled={disabled || index === value.modelMappings.length - 1} onClick={() => move(index, index + 1)}><ArrowDown size={14} /></IconButton><IconButton aria-label={t("移除模型 ") + (index + 1)} disabled={disabled} onClick={() => onChange({ ...value, modelMappings: value.modelMappings.filter((_, i) => i !== index) })}><Trash2 size={14} /></IconButton></div>
      </div>)}</div>
      <div className="inline-actions"><Button variant="secondary" size="sm" icon={<Plus size={14} />} disabled={disabled} onClick={() => onChange({ ...value, modelMappings: [...value.modelMappings, { model: "", displayName: "", contextWindow: null }] })}>{t("添加映射")}</Button><Button variant="quiet" size="sm" icon={<Search size={14} />} disabled={disabled || !apiKeyConfigured} loading={modelBusy} onClick={onDiscover}>{t("探索模型")}</Button>{models.length > 0 && <Button variant="secondary" size="sm" icon={<Download size={14} />} disabled={disabled} onClick={() => onChange({ ...value, modelMappings: mergeDiscoveredModelMappings(value.modelMappings, models) })}>{t("导入全部")} {models.length} {t("项")}</Button>}</div>
      {models.length > 0 && <Field label={t("从探索结果添加映射")}><Select value="" disabled={disabled} onChange={event => { const model = models.find(m => m.id === event.target.value); if (model) onChange({ ...value, modelMappings: addDiscoveredModelMapping(value.modelMappings, model) }); }}><option value="">{t("选择模型")}</option>{models.map(model => <option key={model.id} value={model.id}>{model.name === model.id ? model.id : model.name + " · " + model.id}</option>)}</Select></Field>}
    </div></details>
    <details className="details-group"><summary>{t("Claude 配置")}</summary><div className="details-group__body"><div className="form-switch"><div className="form-switch__copy"><strong>{t("禁用 Artifact 工具")}</strong><small>{t("转发前从 tools 数组中移除 Artifact，适用于拒收其 Schema 的网关。")}</small></div><Toggle aria-label={t("禁用 Artifact 工具")} checked={value.claude.disableArtifactTool} disabled={disabled} onCheckedChange={checked => onChange({ ...value, claude: { ...value.claude, disableArtifactTool: checked } })} /></div></div></details>
    <details className="details-group"><summary>{t("Codex 配置与安全预览")}</summary><div className="details-group__body">
      <p className="notice">{t("仅在开启 Codex 接管时同步这些配置；切换供应商不会启用未开启的客户端接管。客户端配置更新后请重启 Codex。")}</p>
      <Field label={t("auth.json（脱敏预览）")}><Textarea className="mono" readOnly value={JSON.stringify({ OPENAI_API_KEY: apiKeyConfigured ? t("<已安全配置>") : t("<未配置>") }, null, 2)} /></Field>
      <Field label="config.toml" description={t("不得包含 API Key、Token 或密码。")}><Textarea className="mono" value={value.codex.configToml} disabled={disabled} onChange={event => setCodex({ configToml: event.target.value })} /></Field>
      {[{ key: "remoteCompression", label: t("启用远程压缩") }, { key: "applyCommonConfig", label: t("应用通用配置") }, { key: "context1m", label: t("1M 上下文窗口") }].map(item => <div className="form-switch" key={item.key}><strong>{item.label}</strong><Toggle aria-label={item.label} checked={Boolean(value.codex[item.key as "remoteCompression" | "applyCommonConfig" | "context1m"])} disabled={disabled} onCheckedChange={checked => setCodex({ [item.key]: checked })} /></div>)}
      <Field label={t("压缩阈值")}><Input type="number" min={10000} max={10000000} disabled={disabled || !value.codex.context1m} value={value.codex.compactionThreshold} onChange={event => setCodex({ compactionThreshold: Number(event.target.value) })} /></Field>
    </div></details>
  </>;
}
