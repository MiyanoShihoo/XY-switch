import { Power } from "lucide-react";
import type { ProxyStatus } from "../types/proxy";
import { Badge, Button } from "../ui";
export function RuntimeBar({ status, busy, onToggle }: { status: ProxyStatus | null; busy: boolean; onToggle: () => void }) {
  const running = status?.running === true;
  return <footer className="xy-shell-runtime" aria-label="路由运行状态"><div className="xy-shell-runtime-state"><span>本地路由</span><Badge tone={status?.lastError ? "danger" : running ? "success" : "neutral"} dot>{!status ? "状态未读取" : status.lastError ? "运行异常" : running ? "运行中" : "已停止"}</Badge>{status && <code>{status.address || status.settings.host + ":" + status.settings.port}</code>}</div><div className="xy-shell-runtime-detail"><span title={status?.lastError ?? undefined}>{status?.lastError || (running ? "目标 " + (status?.settings.routeTargets.join(" / ") || "未配置") : "配置选择不等于连接验证")}</span><Button size="sm" variant="quiet" icon={<Power size={14} />} loading={busy} disabled={!status} onClick={onToggle}>{running ? "停止" : "启动"}</Button></div></footer>;
}
