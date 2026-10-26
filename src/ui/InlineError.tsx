import { useLanguage, t, translateMessage } from "../i18n/language";
import { AlertTriangle } from "lucide-react";
import { Button } from "./Button";
export function InlineError({ message, onRetry, retrying = false }: { message?: string; onRetry?: () => void; retrying?: boolean }) {
  useLanguage();
  if (!message) return null;
  return <div className="xy-ui-inline-error" role="alert"><AlertTriangle size={16} aria-hidden="true" /><span>{translateMessage(message)}</span>{onRetry && <Button size="sm" variant="quiet" loading={retrying} onClick={onRetry}>{t("重试")}</Button>}</div>;
}
