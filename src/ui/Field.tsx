import { Children, cloneElement, isValidElement, useId, type ReactElement, type ReactNode } from "react";
import { cx } from "./utils";
interface FieldProps { label: string; htmlFor?: string; description?: string; error?: string; required?: boolean; children: ReactNode; className?: string; }
export function Field({ label, htmlFor, description, error, required = false, children, className }: FieldProps) {
  const generatedId = useId();
  const child = Children.toArray(children)[0];
  const childProps = isValidElement(child) ? child.props as Record<string, unknown> : {};
  const id = htmlFor ?? (typeof childProps.id === "string" ? childProps.id : generatedId);
  const describedBy = [childProps["aria-describedby"], description && id + "-description", error && id + "-error"].filter(Boolean).join(" ") || undefined;
  const control = isValidElement(child) ? cloneElement(child as ReactElement<Record<string, unknown>>, { id, "aria-describedby": describedBy, "aria-invalid": error ? true : childProps["aria-invalid"], required: required || childProps.required }) : children;
  return <div className={cx("xy-ui-field", className)}>
    <label className="xy-ui-field__label" htmlFor={id}><span>{label}{required && <span className="xy-ui-field__required" aria-hidden="true"> *</span>}</span></label>
    {description && <p id={id + "-description"} className="xy-ui-field__description">{description}</p>}
    {control}{error && <p id={id + "-error"} className="xy-ui-field__error" role="alert">{error}</p>}
  </div>;
}
