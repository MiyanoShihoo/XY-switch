import type { HTMLAttributes, ReactNode } from "react";
import { cx } from "./utils";

interface CardProps extends HTMLAttributes<HTMLDivElement> {
  padding?: "sm" | "md" | "lg" | "none";
  interactive?: boolean;
  children: ReactNode;
}

export function Card({ padding = "md", interactive = false, className, children, ...props }: CardProps) {
  return (
    <div className={cx("xy-ui-card", className)} data-padding={padding === "none" ? undefined : padding} data-interactive={interactive || undefined} {...props}>
      {children}
    </div>
  );
}
