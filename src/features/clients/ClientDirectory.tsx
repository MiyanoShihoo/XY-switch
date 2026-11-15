import { ChevronRight, Monitor, Search, Terminal } from "lucide-react";
import type { ReactNode } from "react";
import { t, translateMessage, useLanguage } from "../../i18n/language";
import { Button, Input } from "../../ui";
import type { ClientConnection, ClientId, NativeLoginProgress } from "../../types/clients";
import type { AppTakeoverStatus } from "../../types/takeover";
import { clientConfigurationLabel, groupClientConfigurations } from "./client-configuration";
import { nativeLoginPending } from "./native-account-actions";
import { nativeLoginEndLabels, type ClientAction } from "./ClientDetailsPanel";

interface ClientDirectoryProps {
  rows: ClientConnection[];
  takeovers: AppTakeoverStatus[] | undefined;
  login: NativeLoginProgress | null;
  filter: string;
  busy: boolean;
  disabled: boolean;
  configurationControl?: ReactNode;
  onFilter: (filter: string) => void;
  onSelect: (id: ClientId) => void;
  onAction: (action: ClientAction) => void;
}

export function ClientDirectory({ rows, takeovers, login, filter, busy, disabled, configurationControl, onFilter, onSelect, onAction }: ClientDirectoryProps) {
  useLanguage();
  const groups = groupClientConfigurations(rows);
  const search = filter.trim().toLocaleLowerCase();
  const visibleGroups = groups.map(group => ({ ...group, visibleRows: group.rows.filter(row =>
    [row.label, row.id, group.owner ?? "", clientConfigurationLabel(group)].some(value => value.toLocaleLowerCase().includes(search)))
  })).filter(group => group.visibleRows.length);
  const visibleCount = visibleGroups.reduce((count, group) => count + group.visibleRows.length, 0);

  return <section className="client-directory" aria-label={t("客户端目录")}>
    <div className="client-directory-toolbar">
      <Input id="client-filter" type="search" leading={<Search size={16} />} value={filter} disabled={busy}
        aria-label={t("搜索客户端或配置归属")} placeholder={t("搜索客户端或配置归属")} onChange={event => onFilter(event.currentTarget.value)} />
      {configurationControl != null && <div className="client-configuration-control">{configurationControl}</div>}
    </div>
    <div className="client-directory-summary"><span className="client-directory-count" role="status">{t("{count} 个客户端", { count: visibleCount })} · {t("共 {count} 份配置", { count: visibleGroups.length })}</span><p className="client-directory-note">{t("按配置归属分组；配置状态不代表客户端在线。")}</p></div>
    {visibleCount > 0 ? <div className="client-directory-groups">{visibleGroups.map(group => <section className="client-directory-group" key={group.id} data-shared={group.rows.length > 1} aria-label={clientConfigurationLabel(group)}>
      {group.rows.length > 1 && <header className="client-group-heading">
        <h2 id={"client-group-" + group.rows[0].id}>{clientConfigurationLabel(group)}</h2>
        <span>{t("{count} 个客户端共享配置", { count: group.rows.length })}</span>
      </header>}
      <ul className="client-directory-list">{group.visibleRows.map(row => {
        const takeover = takeovers?.find(item => item.app === row.takeoverTarget);
        const pending = nativeLoginPending(row.id, rows, login);
        const configurationPending = row.configurationPending || group.rows.some(member => member.configurationPending);
        const endedLogin = nativeLoginEndLabels[(login?.clientId === row.id ? login.status : row.loginStatus) ?? ""];
        const errors = [row.setupError, takeover?.lastError].filter(Boolean).map(message => translateMessage(message!));
        const bound = row.account?.poolCandidates.some(candidate => candidate.boundAccountId !== null);
        const configuration = row.nativeVerification === "preview" ? t("浏览器预览")
          : takeover?.active ? t("配置已接管，连接待验证")
          : row.configDetected ? t("已发现配置") : t("未发现配置");
        const attention = [configurationPending ? t("待重启") : "", pending ? t("正在登录") : "", !pending && endedLogin ? t(endedLogin) : "", ...errors].filter(Boolean);
        const account = bound ? t("已绑定") : row.account ? t("原生账号状态已读取") : "";
        const ClientIcon = row.id.includes("desktop") ? Monitor : Terminal;
        return <li className="client-list-row" key={row.id}>
          <button id={"client-row-" + row.id} className="client-row-select" type="button" disabled={busy}
            aria-label={t("查看客户端 {client}", { client: row.label })} onClick={() => onSelect(row.id)}>
            <ClientIcon size={19} aria-hidden="true" />
            <span className="client-row-identity"><strong>{row.label}</strong><span>{t(row.id.includes("desktop") ? "桌面应用" : "命令行工具")}{group.rows.length === 1 && " · " + t(group.owner ? "独立配置" : "未提供配置归属")}{account && " · " + account}</span></span>
            <span className="client-row-state">{configuration}</span>
            <span className="client-row-open">{t("详情")}<ChevronRight size={14} aria-hidden="true" /></span>
          </button>
          <div className="client-row-action">{row.takeoverTarget ? <Button variant="secondary" size="sm"
            aria-label={t(takeover?.active ? "恢复 {client} 的原配置" : "将 {client} 连接本地网关", { client: row.label })}
            disabled={disabled || busy || configurationPending || pending || Boolean(row.setupError) || !takeover}
            onClick={() => onAction({ kind: "gateway", row, enabled: !takeover?.active })}>{t(takeover?.active ? "恢复原配置" : "连接本地网关")}</Button>
            : <span className="client-native-label">{t("原生订阅")}</span>}</div>
          {attention.length > 0 && <p className="client-row-attention" role="status">{attention.join(" · ")}</p>}
        </li>;
      })}</ul>
    </section>)}</div> : <div className="client-directory-empty" role="status">
      <p>{t(search ? "没有匹配的客户端。" : "暂无客户端入口。")}</p>
      {search && <Button variant="secondary" size="sm" onClick={() => onFilter("")}>{t("清除筛选")}</Button>}
    </div>}
  </section>;
}
