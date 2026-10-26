import { useLanguage, t, translateMessage } from "../i18n/language";
import type { ReactNode } from "react";
import { X, CheckCircle2, AlertTriangle, Info, XCircle } from "lucide-react";
import { IconButton } from "./IconButton";
import type { BadgeTone } from "./Badge";
import { cx } from "./utils";

interface ToastProps {
  title: string;
  description?: string;
  tone?: Extract<BadgeTone, "success" | "warning" | "danger" | "info">;
  icon?: ReactNode;
  onDismiss?: () => void;
  className?: string;
}

const defaultIcons = {
  success: <CheckCircle2 size={18} />,
  warning: <AlertTriangle size={18} />,
  danger: <XCircle size={18} />,
  info: <Info size={18} />,
} as const;

export function Toast({ title, description, tone = "info", icon, onDismiss, className }: ToastProps) {
  useLanguage();
  return (
    <div className={cx("xy-ui-toast", className)} data-tone={tone} role="status">
      <span aria-hidden="true">{icon ?? defaultIcons[tone]}</span>
      <div>
        <div className="xy-ui-toast__title">{translateMessage(title)}</div>
        {description && <div className="xy-ui-toast__description">{translateMessage(description)}</div>}
      </div>
      {onDismiss && <IconButton className="xy-ui-toast__close" size="sm" aria-label={t("关闭提示")} onClick={onDismiss}><X size={16} /></IconButton>}
    </div>
  );
}
