import { useLanguage } from "../i18n/language";
import type { ReactNode } from "react";
import { cx } from "./utils";

interface StatProps {
  label: string;
  value: ReactNode;
  detail?: ReactNode;
  className?: string;
}

export function Stat({ label, value, detail, className }: StatProps) {
  useLanguage();
  return (
    <div className={cx("xy-ui-stat", className)}>
      <span className="xy-ui-stat__label">{label}</span>
      <strong className="xy-ui-stat__value">{value}</strong>
      {detail && <span className="xy-ui-stat__detail">{detail}</span>}
    </div>
  );
}
