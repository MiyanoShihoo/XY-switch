import { useLanguage, t } from "../i18n/language";
interface LoadingStateProps {
  label?: string;
  className?: string;
}

export function LoadingState({ label = t("正在加载…"), className }: LoadingStateProps) {
  useLanguage();
  return (
    <div className={`xy-ui-loading${className ? ` ${className}` : ""}`} role="status" aria-live="polite">
      <span className="xy-ui-loading__spinner xy-ui-button__spinner" aria-hidden="true" />
      <p className="xy-ui-loading__label">{label}</p>
    </div>
  );
}
