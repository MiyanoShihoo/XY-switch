import { forwardRef, type ButtonHTMLAttributes, type ReactNode } from "react";
import { cx } from "./utils";

export type ButtonVariant = "primary" | "secondary" | "quiet" | "danger";
export type ButtonSize = "sm" | "md" | "lg";

export interface ButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: ButtonVariant;
  size?: ButtonSize;
  loading?: boolean;
  fullWidth?: boolean;
  icon?: ReactNode;
  iconPosition?: "start" | "end";
}

export const Button = forwardRef<HTMLButtonElement, ButtonProps>(function Button(
  {
    variant = "primary",
    size = "md",
    loading = false,
    fullWidth = false,
    icon,
    iconPosition = "start",
    className,
    children,
    disabled,
    type = "button",
    ...props
  },
  ref,
) {
  const iconBefore = iconPosition === "start";
  return (
    <button
      ref={ref}
      type={type}
      className={cx("xy-ui-button", className)}
      data-variant={variant}
      data-size={size}
      data-width={fullWidth ? "full" : undefined}
      aria-busy={loading || undefined}
      disabled={disabled || loading}
      {...props}
    >
      {loading && <span className="xy-ui-button__spinner" aria-hidden="true" />}
      {!loading && icon && iconBefore && <span className="xy-ui-button__icon" aria-hidden="true">{icon}</span>}
      {children}
      {!loading && icon && !iconBefore && <span className="xy-ui-button__icon" aria-hidden="true">{icon}</span>}
    </button>
  );
});
