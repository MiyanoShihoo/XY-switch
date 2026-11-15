import { useEffect, useId, useRef, type FormEvent } from "react";
import { Plus, Trash2 } from "lucide-react";
import { t, useLanguage } from "../../i18n/language";
import type { AccountInfo } from "../../types/accounts";
import type { ChannelGroupConfiguration } from "../../types/channel-group";
import type { Provider } from "../../types/provider";
import { Button, Field, InlineError, Input, Select } from "../../ui";
import { CHANNEL_GROUP_STRATEGIES, changeDraftProject, newChannelDraft, type ChannelDraft, type ChannelGroupDraft, type ChannelGroupFormErrors } from "./channel-group-form";
import { channelStrategyDescription, channelStrategyLabel } from "./channel-group-presentation";

interface Props {
  draft: ChannelGroupDraft;
  configuration: ChannelGroupConfiguration;
  providers: Provider[];
  accounts: AccountInfo[];
  accountsError: string;
  errors: ChannelGroupFormErrors;
  busy: boolean;
  blocked: boolean;
  dirty: boolean;
  onChange: (draft: ChannelGroupDraft) => void;
  onSave: () => void;
  onCancel: () => void;
}

export function ChannelGroupEditor({ draft, configuration, providers, accounts, accountsError, errors, busy, blocked, dirty, onChange, onSave, onCancel }: Props) {
  useLanguage();
  const form = useRef<HTMLFormElement>(null);
  const id = useId();
  const sharedGroups = configuration.groups.filter(group => group.id !== draft.id && group.projectId === draft.projectId.trim());
  const patch = (value: Partial<ChannelGroupDraft>) => onChange({ ...draft, ...value });
  const patchChannel = (index: number, value: Partial<ChannelDraft>) => patch({ channels: draft.channels.map((channel, channelIndex) => channelIndex === index ? { ...channel, ...value } : channel) });
  useEffect(() => {
    if (Object.keys(errors).length) (form.current?.querySelector<HTMLElement>('[aria-invalid="true"]') ?? form.current?.querySelector<HTMLElement>("[data-form-error]"))?.focus();
  }, [errors]);
  const submit = (event: FormEvent) => { event.preventDefault(); if (!busy && !blocked) onSave(); };
  return <form ref={form} className="channel-group-editor" onSubmit={submit} noValidate aria-label={t("编辑渠道组")}>
    {(errors.form || errors.channels) && <div data-form-error tabIndex={-1}><InlineError message={errors.form} /><InlineError message={errors.channels} /></div>}
    <fieldset disabled={busy || blocked} className="channel-group-fields">
      <section className="channel-group-section" aria-labelledby={id + "-identity"}>
        <h2 id={id + "-identity"}>{t("组与虚拟模型")}</h2>
        <div className="channel-group-form-grid">
          <Field label={t("渠道组名称")} error={errors.name} required><Input autoFocus value={draft.name} onChange={event => {
            const name = event.target.value;
            patch({ name, ...(!configuration.groups.some(group => group.id === draft.id) && (!draft.modelName || draft.modelName === draft.name.trim()) && /^[A-Za-z0-9._-]*$/.test(name.trim()) ? { modelName: name.trim() } : {}) });
          }} /></Field>
          <Field label={t("模型别名")} error={errors.modelName} required description={t("虚拟模型只供本地试点识别，尚未接入实时路由。")}>
            <Input leading={<code>xy-group/</code>} value={draft.modelName} onChange={event => patch({ modelName: event.target.value })} />
          </Field>
        </div>
        <p className="channel-group-model"><code>{`xy-group/${draft.modelName.trim() || "name"}`}</code></p>
        <fieldset className="channel-group-strategies"><legend>{t("选择策略")}</legend>
          {CHANNEL_GROUP_STRATEGIES.map(strategy => <label key={strategy} className="channel-group-strategy" data-primary={strategy === "roundRobin" || strategy === "quotaAware"}>
            <input type="radio" name={id + "-strategy"} value={strategy} checked={draft.strategy === strategy} onChange={() => patch({ strategy })} />
            <span><strong>{channelStrategyLabel(strategy)}</strong><small>{channelStrategyDescription(strategy)}</small></span>
          </label>)}
        </fieldset>
        <InlineError message={errors.strategy} />
        <label className="channel-group-check"><input type="checkbox" checked={draft.enabled} onChange={event => patch({ enabled: event.target.checked })} />{t("在离线试点中启用此组")}</label>
      </section>

      <section className="channel-group-section" aria-labelledby={id + "-budget"}>
        <h2 id={id + "-budget"}>{t("共享项目预算")}</h2>
        <p>{t("相同项目 ID 的组共享一份日预算；修改策略会影响这些组。金额单位为整数 μUSD，1 USD = 1,000,000 μUSD。")}</p>
        <div className="channel-group-form-grid">
          <Field label={t("共享项目 ID")} description={t("留空表示不关联项目；输入已有 ID 会读取其当前预算策略。")} error={errors.projectId}>
            <Input list={id + "-projects"} value={draft.projectId} onChange={event => onChange(changeDraftProject(draft, event.target.value, configuration))} />
          </Field>
          <Field label={t("项目日预算（μUSD）")} description={t("留空为未设置上限；0 表示零预算。")} error={errors.projectDailyMicrousd}>
            <Input inputMode="numeric" disabled={!draft.projectId.trim()} value={draft.projectDailyMicrousd} onChange={event => patch({ projectDailyMicrousd: event.target.value })} />
          </Field>
        </div>
        <datalist id={id + "-projects"}>{configuration.projects.map(project => <option key={project.id} value={project.id} />)}</datalist>
        {draft.projectId.trim() && <p className="channel-group-shared" role="status">{sharedGroups.length ? t("同时影响 {count} 个其他组：{names}", { count: sharedGroups.length, names: sharedGroups.map(group => group.name).join(", ") }) : t("此项目当前没有其他渠道组。")}</p>}
        <Field label={t("会话亲和有效期（秒）")} error={errors.affinityTtlSeconds}><Input inputMode="numeric" value={draft.affinityTtlSeconds} onChange={event => patch({ affinityTtlSeconds: event.target.value })} /></Field>
      </section>

      <section className="channel-group-section" aria-labelledby={id + "-channels"}>
        <div className="channel-group-section-heading"><h2 id={id + "-channels"}>{t("渠道引用")}</h2><Button variant="secondary" size="sm" icon={<Plus size={14} />} disabled={draft.channels.length >= 32} onClick={() => patch({ channels: [...draft.channels, newChannelDraft(crypto.randomUUID())] })}>{t("添加渠道")}</Button></div>
        <p>{t("仅引用已有供应商、账号和模型。这里只读取账号元数据。")}</p>
        <InlineError message={accountsError} />
        <div className="channel-group-channel-list">{draft.channels.map((channel, index) => {
          const key = `channels.${index}.`;
          const provider = providers.find(item => item.id === channel.providerId);
          const providerAccounts = accounts.filter(account => account.providerId === channel.providerId);
          const account = providerAccounts.find(item => item.id === channel.accountId);
          const models = [...new Set([provider?.model, ...(provider?.advanced.modelMappings.map(mapping => mapping.model) ?? []), ...(account?.availableModels ?? [])].filter((model): model is string => Boolean(model)))];
          const advancedErrors = Object.keys(errors).some(path => path.startsWith(key) && !["providerId", "accountId", "upstreamModel"].includes(path.slice(key.length)));
          const numberField = (field: "priority" | "weight" | "rpm" | "tpm" | "concurrency" | "dailyMicrousd" | "rateLimitSeconds" | "failureSeconds" | "maxBackoffSeconds", label: string) => <Field label={label} error={errors[key + field]}><Input inputMode="numeric" value={channel[field]} onChange={event => patchChannel(index, { [field]: event.target.value })} /></Field>;
          return <fieldset key={channel.id} className="channel-group-channel" data-channel-id={channel.id}>
            <legend>{t("渠道 {index}", { index: index + 1 })}</legend>
            <div className="channel-group-section-heading"><label className="channel-group-check"><input type="checkbox" checked={channel.enabled} onChange={event => patchChannel(index, { enabled: event.target.checked })} />{t("参与离线选择")}</label><Button variant="quiet" size="sm" icon={<Trash2 size={14} />} aria-label={t("移除渠道 {index}", { index: index + 1 })} onClick={() => patch({ channels: draft.channels.filter(item => item.id !== channel.id) })}>{t("移除")}</Button></div>
            <div className="channel-group-form-grid channel-group-reference-grid">
              <Field label={t("供应商引用")} error={errors[key + "providerId"]} required><Select value={channel.providerId} onChange={event => {
                const next = providers.find(item => item.id === event.target.value);
                patchChannel(index, { providerId: event.target.value, accountId: "", upstreamModel: next?.model ?? "" });
              }}><option value="">{t("选择供应商")}</option>{channel.providerId && !provider && <option value={channel.providerId}>{t("引用缺失：{id}", { id: channel.providerId })}</option>}{providers.map(item => <option key={item.id} value={item.id}>{item.name}{item.enabled ? "" : ` · ${t("已禁用")}`}</option>)}</Select></Field>
              <Field label={t("账号引用")} error={errors[key + "accountId"]}><Select value={channel.accountId} disabled={!channel.providerId} onChange={event => patchChannel(index, { accountId: event.target.value })}>
                <option value="">{t("供应商默认引用")}</option>{channel.accountId && !account && <option value={channel.accountId}>{t("未读取的账号引用：{id}", { id: channel.accountId })}</option>}{providerAccounts.map(item => <option key={item.id} value={item.id}>{item.alias} · {item.id}{item.enabled && item.status !== "revoked" ? "" : ` · ${t("不可用")}`}</option>)}
              </Select></Field>
              <Field label={t("上游模型 ID")} error={errors[key + "upstreamModel"]} required><Input value={channel.upstreamModel} list={id + "-models-" + index} onChange={event => patchChannel(index, { upstreamModel: event.target.value })} /></Field>
            </div>
            <datalist id={id + "-models-" + index}>{models.map(model => <option key={model} value={model} />)}</datalist>
            <details className="channel-group-advanced" open={advancedErrors || undefined}>
              <summary>{t("限制、权重、优先级与额度共享")}</summary>
              <p>{t("优先级数字越小越先选；限制留空为未设置，不代表上游剩余额度。")}</p>
              <div className="channel-group-form-grid channel-group-limits-grid">
                {numberField("priority", t("优先级"))}{numberField("weight", t("轮询权重"))}
                {numberField("rpm", t("每分钟请求上限（RPM）"))}{numberField("tpm", t("每分钟 Token 上限（TPM）"))}
                {numberField("concurrency", t("并发上限"))}{numberField("dailyMicrousd", t("渠道日预算（μUSD）"))}
              </div>
              <h3>{t("冷却策略")}</h3>
              <div className="channel-group-form-grid channel-group-limits-grid">{numberField("rateLimitSeconds", t("限流冷却（秒）"))}{numberField("failureSeconds", t("失败冷却（秒）"))}{numberField("maxBackoffSeconds", t("最大退避（秒）"))}</div>
              <h3>{t("额度池关系声明")}</h3>
              <Field label={t("额度共享方式")} error={errors[key + "quotaMode"]}><Select value={channel.quotaMode} onChange={event => patchChannel(index, { quotaMode: event.target.value as ChannelDraft["quotaMode"] })}>
                <option value="provider">{t("保守共享（默认）")}</option><option value="shared">{t("声明共享额度池")}</option><option value="independent">{t("声明独立额度")}</option>
              </Select></Field>
              {channel.quotaMode === "shared" && <Field label={t("共享额度池 ID")} required error={errors[key + "quotaPoolId"]}><Input value={channel.quotaPoolId} onChange={event => patchChannel(index, { quotaPoolId: event.target.value })} /></Field>}
              <p className="channel-group-declaration">{t("额度共享关系由用户声明，未验证独立性；不同账号或供应商名称不构成独立额度证据。")}</p>
            </details>
          </fieldset>;
        })}</div>
      </section>
    </fieldset>
    <footer className="channel-group-savebar"><p role="status">{t(dirty ? "有未保存的修改" : "没有未保存的修改")}</p><div><Button variant="secondary" disabled={busy} onClick={onCancel}>{t("取消")}</Button><Button type="submit" loading={busy} disabled={blocked}>{t("保存离线配置")}</Button></div></footer>
  </form>;
}
