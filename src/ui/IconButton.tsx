import { forwardRef, type ButtonHTMLAttributes } from "react";
import { cx } from "./utils";
import type { ButtonSize } from "./Button";

export interface IconButtonProps extends Omit<ButtonHTMLAttributes<HTMLButtonElement>, "aria-label"> {
  "aria-label": string;
  size?: ButtonSize;
}

export const IconButton = forwardRef<HTMLButtonElement, IconButtonProps>(function IconButton(
  { size = "md", className, type = "button", ...props },
  ref,
) {
  return <button ref={ref} type={type} className={cx("xy-ui-icon-button", className)} data-size={size} {...props} />;
});
