import { Component, type ErrorInfo, type ReactNode } from "react";
import { AlertTriangle, RefreshCw } from "lucide-react";
import { Button, InlineError } from "../ui";

export class FrontendErrorBoundary extends Component<{ children?: ReactNode }, { error: Error | null }> {
  state = { error: null as Error | null };
  static getDerivedStateFromError(error: Error) { return { error }; }
  componentDidCatch(error: Error, info: ErrorInfo) { console.error("Uncaught frontend error:", error, info); }
  render() {
    if (!this.state.error) return this.props.children;
    return <main className="fatal-error"><AlertTriangle size={32} aria-hidden="true" /><h1>界面暂时无法显示</h1><InlineError message={this.state.error.message || "发生未知错误"} /><p>重新加载界面不会主动重置配置。</p><Button icon={<RefreshCw size={16} />} onClick={() => window.location.reload()}>重新加载</Button></main>;
  }
}
