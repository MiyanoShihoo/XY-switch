import type { Ref } from "react";
import { Copy, Pencil, Trash2 } from "lucide-react";
import { localeTag, t, useLanguage } from "../../i18n/language";
import type { Prompt } from "../../types/prompt";
import { Badge, Button, Toggle } from "../../ui";

export function PromptReader({ prompt, busy, editable, headingRef, editRef, onCopy, onEdit, onToggle, onDuplicate, onDelete }: {
  prompt: Prompt; busy: boolean; editable: boolean; headingRef?: Ref<HTMLHeadingElement>; editRef?: Ref<HTMLButtonElement>;
  onCopy: () => void; onEdit: () => void; onToggle: (enabled: boolean) => void; onDuplicate: () => void; onDelete: () => void;
}) {
  useLanguage();
  return <section className="prompt-document" aria-label={t("提示词内容")}>
    <header className="prompt-document-heading"><h2 ref={headingRef} tabIndex={-1}>{prompt.name}</h2><div className="prompt-document-actions">
      <Button variant="secondary" size="sm" icon={<Copy size={14} />} disabled={busy} onClick={onCopy}>{t("复制内容")}</Button>
      <Button ref={editRef} size="sm" icon={<Pencil size={14} />} disabled={busy || !editable} onClick={onEdit}>{t("编辑")}</Button>
    </div></header>
    <div className="prompt-detail">
      <div className="prompt-detail__meta"><Badge tone={prompt.enabled ? "accent" : "neutral"}>{prompt.enabled ? t("已启用") : t("已停用")}</Badge><span>{prompt.content.length.toLocaleString(localeTag())} {t("字符 · 内容按纯文本呈现")}</span></div>
      {prompt.description && <p className="prompt-detail__description">{prompt.description}</p>}
      <pre className="prompt-preview">{prompt.content}</pre>
      <footer className="prompt-management">
        <div className="form-switch"><strong>{t("启用此提示词")}</strong><Toggle aria-label={t("启用 ") + prompt.name} checked={prompt.enabled} disabled={busy || !editable} onCheckedChange={onToggle} /></div>
        <p className="page-description">{t("启用状态仅在提示词库中保存，不表示已注入客户端。")}</p>
        <div className="inline-actions">
          <Button variant="secondary" size="sm" icon={<Copy size={14} />} disabled={busy || !editable} onClick={onDuplicate}>{t("复制为新提示词")}</Button>
          <Button variant="danger" size="sm" icon={<Trash2 size={14} />} disabled={busy || !editable} onClick={onDelete}>{t("删除提示词")}</Button>
        </div>
      </footer>
    </div>
  </section>;
}
