import { useId, useRef, type ReactNode } from "react";
import { X } from "lucide-react";
import { IconButton } from "./IconButton";
import { useDialogFocus } from "./useDialogFocus";

interface DrawerProps { open: boolean; title: string; description?: string; side?: "left" | "right"; closeLabel?: string; onClose: () => void; children: ReactNode; footer?: ReactNode; busy?: boolean; }
export function Drawer({ open, title, description, side = "right", closeLabel = "关闭面板", onClose, children, footer, busy = false }: DrawerProps) {
  const id = useId(), panelRef = useRef<HTMLDivElement>(null);
  useDialogFocus(open, panelRef, onClose, busy);
  if (!open) return null;
  return <div className="xy-ui-drawer-shell" data-side={side} role="presentation" onMouseDown={event => { if (!busy && event.target === event.currentTarget) onClose(); }}>
    <div ref={panelRef} className="xy-ui-drawer" role="dialog" aria-modal="true" aria-labelledby={id + "-title"} aria-describedby={description ? id + "-description" : undefined} aria-busy={busy} tabIndex={-1}>
      <header className="xy-ui-drawer__header"><div><h2 id={id + "-title"}>{title}</h2>{description && <p id={id + "-description"} className="xy-ui-drawer__description">{description}</p>}</div><IconButton aria-label={closeLabel} disabled={busy} onClick={onClose}><X size={16} /></IconButton></header>
      <div className="xy-ui-drawer__body">{children}</div>{footer && <footer className="xy-ui-drawer__footer">{footer}</footer>}
    </div>
  </div>;
}
