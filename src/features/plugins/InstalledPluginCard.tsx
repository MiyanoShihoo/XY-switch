import { useLanguage, t, translateMessage } from "../../i18n/language";
import { pluginStatusText } from "../../lib/plugins-service";
import type { InstalledPlugin, PluginClient } from "../../types/plugins";
import { Badge, Button, Card } from "../../ui";
import { pluginClientLabels } from "./clients";
import { PermissionsSummary } from "./PermissionsSummary";

export function InstalledPluginCard({ plugin, busy, onReviewClients, onDisable, onUninstall, onRollback }: {
  plugin: InstalledPlugin; busy: boolean; onReviewClients: (client?: PluginClient) => void;
  onDisable: (client: PluginClient | null) => void; onUninstall: () => void; onRollback: (version: string) => void;
}) {
  useLanguage();
  const manifest = plugin.manifest;
  const verified = plugin.verification === "verified";
  return <Card className="plugins-installed-card">
    <header className="plugins-section-header"><div><h3>{manifest?.name ?? plugin.id}</h3>
      <p>{plugin.id} · {plugin.activeVersion ?? t("无活跃版本")}</p></div>
      <Badge tone={verified ? "success" : "warning"}>{translateMessage(pluginStatusText(plugin.verification))}</Badge></header>
    {manifest && <><p>{manifest.description}</p><PermissionsSummary permissions={manifest.permissions} /></>}
    {plugin.digest && <code className="plugins-digest">{plugin.digest}</code>}
    {plugin.warnings.map(code => <p className="plugins-notice" key={code}>{translateMessage(pluginStatusText(code))}</p>)}
    {manifest && plugin.installed && <div className="plugins-client-list" aria-label={t("客户端授权状态")}>
      {manifest.clients.map(client => <div className="plugins-client-row" key={client}><span>{pluginClientLabels[client]}</span>
        <Badge tone={plugin.enabledClients.includes(client) ? "success" : "neutral"}>{plugin.enabledClients.includes(client) ? t("已启用") : t("已停用")}</Badge>
        {plugin.enabledClients.includes(client)
          ? <Button variant="secondary" size="sm" disabled={busy} onClick={() => onDisable(client)}>{t("停用")}</Button>
          : <Button variant="secondary" size="sm" disabled={busy || !verified} onClick={() => onReviewClients(client)}>{t("审核授权")}</Button>}
      </div>)}
    </div>}
    <div className="plugins-actions"><Button variant="secondary" disabled={busy || !plugin.installed || !verified}
      onClick={() => onReviewClients()}>{t("审核全部客户端")}</Button>
      <Button variant="secondary" disabled={busy || !plugin.installed || !plugin.enabledClients.length}
        onClick={() => onDisable(null)}>{t("停用全部客户端")}</Button>
      <Button variant="danger" disabled={busy || !plugin.installed} onClick={onUninstall}>{t("卸载插件")}</Button></div>
    <details className="plugins-versions"><summary>{t("版本历史（{count}）", { count: plugin.versions.length })}</summary>
      <p className="plugins-notice">{t("卸载保留不可变历史缓存。回滚会重新审核完整权限与客户端授权。")}</p>
      {plugin.versions.map(version => <div className="plugins-version" key={version.version}>
        <div className="plugins-section-header"><strong>{version.version}</strong>
          <Badge tone={version.withdrawn ? "danger" : version.verification === "verified" ? "success" : "warning"}>
            {version.withdrawn ? t("已撤回") : translateMessage(pluginStatusText(version.verification))}</Badge>
          <Button variant="secondary" size="sm" disabled={busy || version.withdrawn || version.verification !== "verified"
            || plugin.installed && version.version === plugin.activeVersion} onClick={() => onRollback(version.version)}>{t("审核回滚")}</Button></div>
        <code className="plugins-digest">{version.digest}</code><p>{t("签名密钥")}: {version.signingKeyId}</p>
      </div>)}
    </details>
  </Card>;
}
