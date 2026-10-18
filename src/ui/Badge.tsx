import type { ReactNode } from "react";
import { cx } from "./utils";

export type BadgeTone = "neutral" | "accent" | "success" | "warning" | "danger" | "info";

interface BadgeProps {
  children: ReactNode;
  tone?: BadgeTone;
  dot?: boolean;
  className?: string;
}

export function Badge({ children, tone = "neutral", dot = false, className }: BadgeProps) {
  return (
    <span className={cx("xy-ui-badge", className)} data-tone={tone}>
      {dot && <span className="xy-ui-status-dot" aria-hidden="true" />}
      {children}
    </span>
  );
}
