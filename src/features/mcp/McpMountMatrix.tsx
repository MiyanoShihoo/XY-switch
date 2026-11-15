import { t, useLanguage } from "../../i18n/language";
import { isSharedMcpServerId } from "../../lib/mcp-validation";
import type { McpApp, McpAppStatus, McpServer } from "../../types/mcp";
import { Badge, Toggle } from "../../ui";

export const mcpApplications: Array<{ id: McpApp; label: string }> = [
  { id: "claude", label: "Claude Code" },
  { id: "codex", label: "Codex" },
  { id: "gemini", label: "Gemini CLI" },
  { id: "opencode", label: "OpenCode" },
  { id: "claude-desktop", label: "Claude Desktop" },
];

export function McpMountMatrix({ servers, apps, appsPending, appsFailed, busy, onInspect, onToggle }: {
  servers: McpServer[]; apps: McpAppStatus[]; appsPending: boolean; appsFailed: boolean; busy: boolean;
  onInspect: (server: McpServer) => void;
  onToggle: (server: McpServer, app: McpApp, enabled: boolean) => void;
}) {
  useLanguage();
  const detection = (app: McpApp) => t(appsPending ? "正在加载…" : appsFailed ? "读取失败" : apps.find(item => item.app === app)?.installed ? "已检测" : "未检测到");
  return <table className="mcp-matrix" role="table" aria-describedby="mcp-mount-note">
    <caption className="sr-only">{t("服务与客户端挂载矩阵")}</caption>
    <thead role="rowgroup"><tr role="row"><th scope="col" role="columnheader">{t("服务配置")}</th>{mcpApplications.map(app => <th key={app.id} scope="col" role="columnheader"><span>{app.label}</span><small>{detection(app.id)}</small></th>)}</tr></thead>
    <tbody role="rowgroup">{servers.map(server => {
      const managed = isSharedMcpServerId(server.id);
      return <tr className="mcp-record" key={server.id} role="row">
        <th scope="row" role="rowheader">
          <button className="mcp-service-select" type="button" aria-label={t(managed ? "管理共享连接" : "连接详情") + " · " + server.name} disabled={busy} onClick={() => onInspect(server)}>
            <span className="mcp-service-title"><strong>{server.name}</strong><span className="mcp-transport">{server.transport === "stdio" ? "stdio" : "HTTP / SSE"}</span>{server.builtIn && <Badge>{t("内置")}</Badge>}</span>
            {server.description && <span className="mcp-service-description">{server.description}</span>}
            <code className="mcp-record-endpoint">{server.transport === "stdio" ? server.command : server.url}</code>
            <span className="mcp-mount-summary">{t(managed ? "共享插件连接请在共享插件服务中管理。" : "查看配置与目标路径")}</span>
          </button>
        </th>
        {mcpApplications.map(app => {
          const mounted = server.apps[app.id];
          const installed = apps.find(item => item.app === app.id)?.installed;
          const disabled = busy || appsPending || appsFailed || !installed;
          return <td key={app.id} role="cell"><span className="mcp-mount-client">{app.label}<small>{detection(app.id)}</small></span><div className="mcp-mount-cell">
            {!managed && <Toggle aria-label={server.name + " · " + app.label + t("挂载")} title={appsPending || appsFailed || !installed ? detection(app.id) : undefined} checked={mounted} disabled={disabled} onCheckedChange={enabled => { if (!disabled) onToggle(server, app.id, enabled); }} />}
            <span className="mcp-mount-state" data-mounted={mounted}>{t(mounted ? "已挂载" : "未挂载")}</span>
            {managed && <small>{t("共享管理")}</small>}
          </div></td>;
        })}
      </tr>;
    })}</tbody>
  </table>;
}
