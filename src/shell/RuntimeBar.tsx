import { useLanguage, t } from "../i18n/language";
import { Power } from "lucide-react";
import type { ProxyStatus } from "../types/proxy";
import { Badge, Button } from "../ui";
export function RuntimeBar({ status, busy, onToggle }: { status: ProxyStatus | null; busy: boolean; onToggle: () => void }) {
  useLanguage();
  const running = status?.running === true;
  return <footer className="xy-shell-runtime" aria-label={t("路由运行状态")}><div className="xy-shell-runtime-state"><span>{t("本地路由")}</span><Badge tone={status?.lastError ? "danger" : running ? "success" : "neutral"} dot>{!status ? t("状态未读取") : status.lastError ? t("运行异常") : running ? t("运行中") : t("已停止")}</Badge>{status && <code>{status.address || status.settings.host + ":" + status.settings.port}</code>}</div><div className="xy-shell-runtime-detail"><span title={status?.lastError ?? undefined}>{status?.lastError || (running ? t("目标 ") + (status?.settings.routeTargets.join(" / ") || t("未配置")) : t("配置选择不等于连接验证"))}</span><Button size="sm" variant="quiet" icon={<Power size={14} />} loading={busy} disabled={!status} onClick={onToggle}>{running ? t("停止") : t("启动")}</Button></div></footer>;
}
