import { useLanguage, t } from "../i18n/language";
import { useId, useRef, type ReactNode, type RefObject } from "react";
import { X } from "lucide-react";
import { IconButton } from "./IconButton";
import { useDialogFocus } from "./useDialogFocus";

export interface ModalProps {
  open: boolean; title: string; description?: string; size?: "sm" | "md" | "lg";
  closeLabel?: string; onClose: () => void; children: ReactNode; footer?: ReactNode;
  busy?: boolean; role?: "dialog" | "alertdialog"; hideClose?: boolean;
  initialFocusRef?: RefObject<HTMLElement | null>;
}

export function Modal({ open, title, description, size = "md", closeLabel = t("关闭弹窗"), onClose, children, footer, busy = false, role = "dialog", hideClose = false, initialFocusRef }: ModalProps) {
  useLanguage();
  const id = useId();
  const panelRef = useRef<HTMLDivElement>(null);
  useDialogFocus(open, panelRef, onClose, busy, initialFocusRef);
  if (!open) return null;
  return <div className="xy-ui-overlay" role="presentation" onMouseDown={event => { if (!busy && event.target === event.currentTarget) onClose(); }}>
    <div ref={panelRef} className="xy-ui-modal" data-size={size} role={role} aria-modal="true" aria-labelledby={id + "-title"} aria-describedby={description ? id + "-description" : undefined} aria-busy={busy} tabIndex={-1}>
      <header className="xy-ui-modal__header"><div><h2 className="xy-ui-modal__title" id={id + "-title"}>{title}</h2>{description && <p id={id + "-description"} className="xy-ui-modal__description">{description}</p>}</div>
        {!hideClose && <IconButton aria-label={closeLabel} onClick={onClose} disabled={busy}><X size={16} /></IconButton>}
      </header>
      <div className="xy-ui-modal__body">{children}</div>
      {footer && <footer className="xy-ui-modal__footer">{footer}</footer>}
    </div>
  </div>;
}

export const Dialog = Modal;
