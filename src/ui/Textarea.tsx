import { forwardRef, type TextareaHTMLAttributes } from "react";
import { cx } from "./utils";

export interface TextareaProps extends TextareaHTMLAttributes<HTMLTextAreaElement> {
  invalid?: boolean;
}

export const Textarea = forwardRef<HTMLTextAreaElement, TextareaProps>(function Textarea(
  { className, invalid = false, ...props },
  ref,
) {
  return <textarea ref={ref} className={cx("xy-ui-textarea", className)} data-invalid={invalid || undefined} {...props} />;
});
