import type { ButtonHTMLAttributes } from "react";
import { cx } from "./utils";

interface ToggleProps extends Omit<ButtonHTMLAttributes<HTMLButtonElement>, "onChange" | "aria-checked"> {
  checked: boolean;
  onCheckedChange?: (checked: boolean) => void;
  "aria-label": string;
}

export function Toggle({ checked, onCheckedChange, className, type = "button", onClick, ...props }: ToggleProps) {
  return (
    <button
      {...props}
      type={type}
      role="switch"
      aria-checked={checked}
      className={cx("xy-ui-toggle", className)}
      onClick={(event) => {
        onClick?.(event);
        if (!event.defaultPrevented) onCheckedChange?.(!checked);
      }}
    />
  );
}
