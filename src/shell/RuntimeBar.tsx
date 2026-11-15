import { useEffect, useRef } from "react";
import { ChevronDown, Power } from "lucide-react";
import { t, useLanguage } from "../i18n/language";
import type { ProxyStatus } from "../types/proxy";
import { Badge, Button } from "../ui";

export function RuntimeBar({ status, busy, stale = false, onToggle, currentProviderName }: { status: ProxyStatus | null; busy: boolean; stale?: boolean; onToggle: () => void; currentProviderName?: string }) {
  useLanguage();
  const root = useRef<HTMLDetailsElement>(null);
  const running = status?.running === true;
  const native = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
  const label = stale ? t("运行状态待刷新") : !status ? t("状态未读取") : status.lastError ? t("运行异常") : running ? t("运行中") : t("已停止");
  const tone = stale ? "warning" : status?.lastError ? "danger" : running ? "success" : "neutral";
  useEffect(() => {
    const outside = (event: PointerEvent) => {
      if (root.current?.open && event.target instanceof Node && !root.current.contains(event.target)) root.current.open = false;
    };
    document.addEventListener("pointerdown", outside);
    return () => document.removeEventListener("pointerdown", outside);
  }, []);
  return <details ref={root} className="xy-shell-runtime" data-state={tone} onKeyDown={event => {
    if (event.key === "Escape" && root.current?.open) {
      event.stopPropagation(); root.current.open = false; root.current.querySelector("summary")?.focus();
    }
  }}>
    <summary aria-label={t("路由运行状态") + " · " + label}>
      <span className="xy-shell-runtime-dot" aria-hidden="true" /><span className="xy-shell-runtime-label">{t("本地路由")}</span><span>{label}</span><ChevronDown size={13} aria-hidden="true" />
    </summary>
    <div className="xy-shell-runtime-panel">
      <div className="xy-shell-runtime-heading"><strong>{t("本地路由")}</strong><Badge dot tone={tone}>{label}</Badge></div>
      <dl><div><dt>{t("当前配置")}</dt><dd>{currentProviderName || t("未设置供应商")}</dd></div>
        {status && <div><dt>{t("监听地址")}</dt><dd><code>{status.address || status.settings.host + ":" + status.settings.port}</code></dd></div>}
        {running && <div><dt>{t("目标客户端")}</dt><dd>{status?.settings.routeTargets.join(" / ") || t("未配置")}</dd></div>}
      </dl>
      {status?.lastError ? <p className="xy-shell-runtime-error" role="status">{status.lastError}</p> : <p className="muted">{t("配置选择不等于连接验证")}</p>}
      {!native && <p className="muted">{t("请在桌面应用中控制本地路由。")}</p>}
      <Button variant={running ? "secondary" : "primary"} icon={<Power size={15} />} loading={busy} disabled={stale || !status || !native} onClick={onToggle}>{running ? t("停止") : t("启动")}</Button>
    </div>
  </details>;
}
