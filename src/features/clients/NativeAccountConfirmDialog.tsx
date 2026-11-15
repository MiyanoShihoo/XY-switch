import { useRef } from "react";
import { t, useLanguage } from "../../i18n/language";
import { Button, Dialog, InlineError } from "../../ui";

interface NativeAccountConfirmDialogProps {
  title: string; message: string; confirmLabel: string; busy?: boolean; disabled?: boolean; error?: string;
  onCancel: () => void; onConfirm: () => void;
}

export function NativeAccountConfirmDialog({ title, message, confirmLabel, busy = false, disabled = false, error = "", onCancel, onConfirm }: NativeAccountConfirmDialogProps) {
  useLanguage();
  const cancelRef = useRef<HTMLButtonElement>(null);
  const [summary, ...scope] = message.split("\n");
  return <div className="client-dialog client-confirm"><Dialog open size="sm" role="alertdialog" title={title} description={summary} busy={busy} hideClose initialFocusRef={cancelRef}
    onClose={() => { if (!busy) onCancel(); }} footer={<>
      <Button ref={cancelRef} variant="secondary" disabled={busy} onClick={() => { if (!busy) onCancel(); }}>{t("取消")}</Button>
      <Button variant="danger" disabled={busy || disabled} onClick={() => { if (!busy && !disabled) onConfirm(); }}>{busy ? t("处理中…") : confirmLabel}</Button>
    </>}>
      {scope.length > 0 && <div className="client-confirm__scope" aria-label={t("操作涉及的配置与连接")}>{scope.map((line, index) => <p key={index}>{line}</p>)}</div>}
      <InlineError message={error} />
    </Dialog></div>;
}
