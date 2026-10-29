import { forwardRef, type SelectHTMLAttributes } from "react";
import { cx } from "./utils";

export interface SelectProps extends SelectHTMLAttributes<HTMLSelectElement> {
  invalid?: boolean;
}

export const Select = forwardRef<HTMLSelectElement, SelectProps>(function Select(
  { className, invalid = false, ...props },
  ref,
) {
  return <select ref={ref} className={cx("xy-ui-select", className)} data-invalid={invalid || undefined} {...props} />;
});
