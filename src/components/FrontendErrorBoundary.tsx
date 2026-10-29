import { t, useLanguage } from "../i18n/language";
import { Component, type ErrorInfo, type ReactNode } from "react";
import { AlertTriangle, RefreshCw } from "lucide-react";
import { Button, InlineError } from "../ui";

export class FrontendErrorBoundary extends Component<{ children?: ReactNode }, { error: Error | null }> {
  state = { error: null as Error | null };
  static getDerivedStateFromError(error: Error) { return { error }; }
  componentDidCatch(error: Error, info: ErrorInfo) { console.error("Uncaught frontend error:", error, info); }
  render() {
    if (!this.state.error) return this.props.children;
    return <FatalFrontendError error={this.state.error} />;
  }
}

function FatalFrontendError({ error }: { error: Error }) {
  useLanguage();
  return <main className="fatal-error"><AlertTriangle size={32} aria-hidden="true" /><h1>{t("界面暂时无法显示")}</h1><InlineError message={error.message || t("发生未知错误")} /><p>{t("重新加载界面不会主动重置配置。")}</p><Button icon={<RefreshCw size={16} />} onClick={() => window.location.reload()}>{t("重新加载")}</Button></main>;
}
