import { forwardRef, type InputHTMLAttributes, type ReactNode } from "react";
import { cx } from "./utils";

export interface InputProps extends InputHTMLAttributes<HTMLInputElement> {
  invalid?: boolean;
  leading?: ReactNode;
  trailing?: ReactNode;
}

export const Input = forwardRef<HTMLInputElement, InputProps>(function Input(
  { className, invalid = false, leading, trailing, ...props },
  ref,
) {
  if (!leading && !trailing) {
    return <input ref={ref} className={cx("xy-ui-input", className)} data-invalid={invalid || undefined} {...props} />;
  }

  return (
    <span className="xy-ui-input-shell" data-leading={Boolean(leading)} data-trailing={Boolean(trailing)}>
      {leading && <span className="xy-ui-input-affix" data-position="leading" aria-hidden="true">{leading}</span>}
      <input ref={ref} className={cx("xy-ui-input", className)} data-invalid={invalid || undefined} {...props} />
      {trailing && <span className="xy-ui-input-affix" data-position="trailing" aria-hidden="true">{trailing}</span>}
    </span>
  );
});
