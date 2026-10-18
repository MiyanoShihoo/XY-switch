import type { ReactNode } from "react";
import { Inbox } from "lucide-react";
import { cx } from "./utils";

interface EmptyStateProps {
  title: string;
  description?: string;
  action?: ReactNode;
  icon?: ReactNode;
  className?: string;
}

export function EmptyState({ title, description, action, icon = <Inbox size={28} />, className }: EmptyStateProps) {
  return (
    <div className={cx("xy-ui-empty", className)}>
      <span aria-hidden="true">{icon}</span>
      <strong className="xy-ui-empty__title">{title}</strong>
      {description && <p className="xy-ui-empty__description">{description}</p>}
      {action}
    </div>
  );
}
