import { useId, useState, type ReactNode } from "react";
import { t } from "../../i18n/language";
import type { SyncChange } from "../../types/encrypted-sync";
import { Badge, Field, Select } from "../../ui";
import { DiffPreview } from "../governance/shared";

function effect(value: string) {
  switch (value) {
    case "new_disabled_provider": return t("创建新的禁用供应商；需要在本机重新配置凭据。");
    case "keep_old_target_create_disabled_copy": return t("目标地址或协议已变化：保留旧供应商及凭据，另建禁用副本。");
    case "update_allowed_fields_only": return t("仅修改白名单字段；保留本机凭据和高级配置。");
    case "custom_price_only_history_unchanged": return t("只更新自定义价格，不重算历史费用。");
    default: return t("只合并收藏标识，不包含会话内容，也不传播删除。");
  }
}
const kindLabel = (change: SyncChange) => change.kind === "provider" ? t("供应商") : change.kind === "favorite" ? t("收藏") : t("自定义定价");
const statusLabel = (change: SyncChange) => change.status === "add" ? t("新增") : change.status === "conflict" ? t("冲突") : t("内容没有变化");
const choiceLabel = (change: SyncChange, value: boolean | undefined) => change.status === "unchanged" ? t("内容没有变化") : value === undefined ? t("尚未选择") : value ? t("接受传入项") : t("保留本地 / 跳过");

export function SyncImportReview({ changes, choices, disabled, onChange, pagination }: {
  changes: SyncChange[];
  choices: Record<string, boolean | undefined>;
  disabled: boolean;
  onChange: (key: string, value: boolean | undefined) => void;
  pagination?: ReactNode;
}) {
  const [selectedKey, setSelectedKey] = useState("");
  const detailId = useId();
  const selected = changes.find(change => change.key === selectedKey) ?? changes[0];
  if (!selected) return null;
  const value = choices[selected.key];
  return <div className="sync-review">
    <nav className="sync-review__index" aria-label={t("对象清单")}>
      <h3 className="sync-review__heading"><span>01</span>{t("对象清单")}</h3>
      <Field className="sync-review__mobile-select" label={t("所选对象")}><Select value={selected.key} onChange={event => setSelectedKey(event.target.value)} disabled={disabled}>
        {changes.map(change => <option key={change.key} value={change.key}>{kindLabel(change)} · {change.key} · {choiceLabel(change, choices[change.key])}</option>)}
      </Select></Field>
      <ol className="sync-review__objects">{changes.map(change => <li key={change.key}>
        <button type="button" aria-pressed={selected.key === change.key} aria-controls={detailId} disabled={disabled} onClick={() => setSelectedKey(change.key)}>
          <span className="sync-review__object-meta"><span>{kindLabel(change)}</span><span data-status={change.status}>{statusLabel(change)}</span></span>
          <code title={change.key}>{change.key}</code>
          <span className="sync-review__decision" data-decided={change.status === "unchanged" || choices[change.key] !== undefined}>{choiceLabel(change, choices[change.key])}</span>
        </button>
      </li>)}</ol>
      {pagination}
    </nav>
    <section id={detailId} className="sync-review__detail" aria-label={t("所选对象")}>
      <header className="sync-review__detail-heading"><h3 className="sync-review__heading"><span>02</span>{t("所选对象")}</h3><Badge tone={selected.status === "conflict" ? "warning" : "neutral"}>{statusLabel(selected)}</Badge></header>
      {selected.status === "unchanged" ? <p className="sync-change__effect">{t("此对象与本地一致，不会写入。")}</p> : <p className={selected.effect === "keep_old_target_create_disabled_copy" ? "gov-warning sync-change__effect" : "sync-change__effect"}><strong>{t("接受传入项后的影响")}</strong>{effect(selected.effect)}</p>}
      {selected.status !== "unchanged" && <Field className="sync-choice" label={t("处理方式")}><Select aria-label={t("处理方式") + " · " + selected.key} value={value === undefined ? "" : value ? "incoming" : "local"} disabled={disabled} onChange={event => onChange(selected.key, event.target.value === "" ? undefined : event.target.value === "incoming")}><option value="">{t("请选择")}</option><option value="local">{t("保留本地 / 跳过")}</option><option value="incoming">{t("接受传入项")}</option></Select></Field>}
      {selected.status !== "unchanged" && <p className="sync-change__decision" role="status">{value === false ? t("已选择保留本地 / 跳过，此对象不会写入。") : value === true ? t("已选择接受传入项，最终确认后才会合并。") : t("请先选择此对象的处理方式。")}</p>}
      <DiffPreview identity={selected.key} beforeLabel={t("本地内容")} afterLabel={t("传入内容")} before={JSON.stringify(selected.local, null, 2)} after={JSON.stringify(selected.incoming, null, 2)} />
    </section>
  </div>;
}
