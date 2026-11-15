import { useLanguage, t, translateMessage } from "../../i18n/language";
import { pluginStatusText } from "../../lib/plugins-service";
import type { InstalledPlugin, PluginClient } from "../../types/plugins";
import { Badge, Button } from "../../ui";
import { pluginClientLabels } from "./clients";
import { PermissionsSummary } from "./PermissionsSummary";

interface PluginActions {
  task?: "clients" | "permissions" | "versions"; onOpenTools?: (client: PluginClient) => void;
  plugin: InstalledPlugin; busy: boolean; onReviewClients: (client?: PluginClient) => void;
  onDisable: (client: PluginClient | null) => void; onUninstall: () => void; onRollback: (version: string) => void;
}

export function InstalledPluginCard({ plugin, busy, selected = false, onOpen }: {
  plugin: InstalledPlugin; busy: boolean; selected?: boolean; onOpen: (trigger: HTMLButtonElement) => void;
}) {
  useLanguage();
  const name = plugin.manifest?.name ?? plugin.id;
  const verified = plugin.verification === "verified";
  return <article className="plugins-directory-row" role="listitem" data-selected={selected}>
    <button className="plugins-package-select" type="button" disabled={busy} aria-pressed={selected}
      aria-label={t("审查插件 {name}", { name })} onClick={event => onOpen(event.currentTarget)}>
    <span className="plugins-directory-identity"><strong title={name}>{name}</strong>
      <span className="plugins-package-version">{plugin.activeVersion ?? t("无活跃版本")}</span></span>
    {plugin.manifest && <span className="plugins-directory-description">{plugin.manifest.description}</span>}
    <span className="plugins-directory-context"><code>{plugin.id}</code>{plugin.manifest && <span>{plugin.manifest.clients.map(client => pluginClientLabels[client]).join(" · ")}</span>}</span>
    <span className="plugins-directory-status"><Badge tone={!plugin.installed ? "neutral" : verified ? "success" : "warning"}>
      {plugin.installed ? translateMessage(pluginStatusText(plugin.verification)) : t("仅历史记录")}</Badge>
      {plugin.installed && <span className="plugins-notice">{t("{count} 个客户端已启用", { count: plugin.enabledClients.length })}</span>}
    </span></button>
  </article>;
}

export function InstalledPluginDetails({ plugin, busy, onReviewClients, onDisable, onUninstall, onRollback, task, onOpenTools }: PluginActions) {
  useLanguage();
  const manifest = plugin.manifest;
  const verified = plugin.verification === "verified";
  return <div className="plugins-installed-body">
    {!task && <><div className="plugins-section-header"><code className="plugins-directory-meta">{plugin.id}</code>
      <Badge tone={verified ? "success" : "warning"}>{translateMessage(pluginStatusText(plugin.verification))}</Badge></div>
    {manifest && <p>{manifest.description}</p>}
    <dl className="key-values"><dt>{t("版本")}</dt><dd><code>{plugin.activeVersion ?? t("无活跃版本")}</code></dd>
      {manifest && <><dt>{t("发布者")}</dt><dd>{manifest.author}</dd></>}</dl>
    {plugin.warnings.map(code => <p className="plugins-notice" key={code}>{translateMessage(pluginStatusText(code))}</p>)}</>}
    {(!task || task === "clients") && manifest && plugin.installed && <section className="plugins-detail-section" aria-label={t("客户端授权状态")}>
      <div className="plugins-section-header"><h3>{t("客户端授权状态")}</h3>
        <Button variant="secondary" size="sm" disabled={busy || !verified} onClick={() => onReviewClients()}>{t("审核全部客户端")}</Button></div>
      <div className="plugins-client-list">{manifest.clients.map(client => <div className="plugins-client-row" key={client}><span>{pluginClientLabels[client]}</span>
        <Badge tone={plugin.enabledClients.includes(client) ? "success" : "neutral"}>{plugin.enabledClients.includes(client) ? t("已启用") : t("已停用")}</Badge>
        {plugin.enabledClients.includes(client)
          ? <Button variant="secondary" size="sm" disabled={busy} onClick={() => onDisable(client)}>{t("停用")}</Button>
          : <Button variant="secondary" size="sm" disabled={busy || !verified} onClick={() => onReviewClients(client)}>{t("审核授权")}</Button>}
        {onOpenTools && verified && plugin.enabledClients.includes(client) && <Button variant="quiet" size="sm" disabled={busy} onClick={() => onOpenTools(client)}>{t("查看可调用工具")}</Button>}
      </div>)}</div>
    </section>}
    {(!task || task === "permissions") && <details className="workspace-disclosure" open><summary>{t("权限与签名")}</summary><div className="workspace-disclosure__body">
      {manifest && <PermissionsSummary permissions={manifest.permissions} />}
      {plugin.digest && <dl className="key-values"><dt>SHA-256</dt><dd><code className="plugins-digest">{plugin.digest}</code></dd></dl>}
    </div></details>}
    {(!task || task === "versions") && <details className="workspace-disclosure plugins-versions" open={task === "versions"}><summary>{t("版本历史（{count}）", { count: plugin.versions.length })}</summary><div className="workspace-disclosure__body">
      <p className="plugins-notice">{t("卸载保留不可变历史缓存。回滚会重新审核完整权限与客户端授权。")}</p>
      {plugin.versions.map(version => <div className="plugins-version" key={version.version}>
        <div className="plugins-section-header"><strong className="plugins-package-version">{version.version}</strong>
          <Badge tone={version.withdrawn ? "danger" : version.verification === "verified" ? "success" : "warning"}>
            {version.withdrawn ? t("已撤回") : translateMessage(pluginStatusText(version.verification))}</Badge>
          <Button variant="secondary" size="sm" disabled={busy || version.withdrawn || version.verification !== "verified"
            || plugin.installed && version.version === plugin.activeVersion} onClick={() => onRollback(version.version)}>{t("审核回滚")}</Button></div>
        <code className="plugins-digest">{version.digest}</code><p>{t("签名密钥")}: <code>{version.signingKeyId}</code></p>
      </div>)}
    </div></details>}
    {task === "clients" && (!manifest || !plugin.installed) && <p className="plugins-notice">{t("此包当前没有活跃安装，可在版本历史中重新审核安装。")}</p>}
    {(!task || task === "clients") && <div className="plugins-actions plugins-maintenance"><Button variant="secondary" disabled={busy || !plugin.installed || !plugin.enabledClients.length}
      onClick={() => onDisable(null)}>{t("停用全部客户端")}</Button>
      <Button variant="danger" disabled={busy || !plugin.installed} onClick={onUninstall}>{t("卸载插件")}</Button></div>}
  </div>;
}
