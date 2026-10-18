import { X } from "lucide-react";
import type { ReactNode } from "react";
import { IconButton } from "./IconButton";
export function Inspector({ title, onClose, children, className = "" }: { title: string; onClose?: () => void; children: ReactNode; className?: string }) {
  return <aside className={"xy-ui-inspector " + className} aria-label={title}><header className="xy-ui-inspector__header"><h2>{title}</h2>{onClose && <IconButton aria-label="关闭详情" onClick={onClose}><X size={16} /></IconButton>}</header><div className="xy-ui-inspector__body">{children}</div></aside>;
}
