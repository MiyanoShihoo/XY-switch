import { useRef } from "react";
import { Button, Dialog, InlineError } from "../ui";
interface ConfirmActionDialogProps { title: string; message: string; confirmLabel?: string; busy?: boolean; error?: string; onCancel: () => void; onConfirm: () => void; }
export function ConfirmActionDialog({ title, message, confirmLabel = "确认删除", busy = false, error = "", onCancel, onConfirm }: ConfirmActionDialogProps) {
  const cancelRef = useRef<HTMLButtonElement>(null);
  return <Dialog open size="sm" role="alertdialog" title={title} description={message} busy={busy} hideClose initialFocusRef={cancelRef} onClose={() => { if (!busy) onCancel(); }} footer={<>
    <Button ref={cancelRef} variant="secondary" disabled={busy} onClick={() => { if (!busy) onCancel(); }}>取消</Button>
    <Button variant="danger" disabled={busy} onClick={() => { if (!busy) onConfirm(); }}>{busy ? "处理中…" : confirmLabel}</Button>
  </>}><InlineError message={error} /></Dialog>;
}
