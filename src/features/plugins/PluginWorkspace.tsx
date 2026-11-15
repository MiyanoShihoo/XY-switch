import { useEffect, useRef, type ReactNode } from "react";
import { ArrowLeft } from "lucide-react";
import { t, translateMessage, useLanguage } from "../../i18n/language";
import { pluginStatusText } from "../../lib/plugins-service";
import type { InstalledPlugin, PluginClient, ToolExecutionResult, VerifiedTool } from "../../types/plugins";
import { Badge, Button, EmptyState, Tabs } from "../../ui";
import { pluginClientLabels } from "./clients";

export type PluginTask = "clients" | "permissions" | "versions" | "tools" | "evidence";

export function toolsForPackage(plugin: InstalledPlugin | undefined, client: PluginClient | "", tools: VerifiedTool[] = []): VerifiedTool[] {
  if (!plugin?.installed || plugin.verification !== "verified" || !client || !plugin.enabledClients.includes(client)
    || !plugin.manifest?.clients.includes(client)) return [];
  return tools.filter(tool => tool.pluginId === plugin.id && tool.version === plugin.activeVersion
    && tool.digest === plugin.digest && tool.client === client);
}

export function PluginWorkspace({ plugin, task, busy, onTask, onBack, children }: {
  plugin: InstalledPlugin; task: PluginTask; busy: boolean; onTask: (task: PluginTask) => void; onBack: () => void; children: ReactNode;
}) {
  useLanguage();
  const heading = useRef<HTMLHeadingElement>(null);
  useEffect(() => { heading.current?.focus(); }, [plugin.id]);
  return <section className="plugins-lifecycle" aria-label={t("插件生命周期工作区")}>
    <Button className="plugins-directory-back" variant="quiet" size="sm" icon={<ArrowLeft size={16} />} disabled={busy} onClick={onBack}>{t("返回包目录")}</Button>
    <header className="plugins-lifecycle-heading"><div className="plugins-section-header">
      <h2 ref={heading} tabIndex={-1}>{plugin.manifest?.name ?? plugin.id}</h2>
      <Badge tone={!plugin.installed ? "neutral" : plugin.verification === "verified" ? "success" : "warning"}>
        {plugin.installed ? translateMessage(pluginStatusText(plugin.verification)) : t("仅历史记录")}</Badge></div>
      <p className="plugins-lifecycle-purpose">{plugin.manifest?.description}</p>
      <p className="plugins-lifecycle-identity"><code>{plugin.id}</code><span>{plugin.activeVersion ?? t("无活跃版本")}</span>
        {plugin.manifest && <span>{plugin.manifest.author}</span>}</p>
      {plugin.warnings.map(code => <p key={code} className="plugins-notice">{translateMessage(pluginStatusText(code))}</p>)}
    </header>
    <Tabs id="plugin-lifecycle-tasks" label={t("包生命周期任务")} value={task} onChange={value => onTask(value as PluginTask)} items={[
      { value: "clients", label: t("客户端授权"), disabled: busy },
      { value: "permissions", label: t("权限与签名"), disabled: busy },
      { value: "versions", label: t("版本历史"), count: plugin.versions.length, disabled: busy },
      { value: "tools", label: t("可调用工具"), disabled: busy },
      { value: "evidence", label: t("调用证据"), disabled: busy },
    ]} />
    <div className="plugins-lifecycle-task" id={"plugin-lifecycle-tasks-panel-" + task} role="tabpanel" aria-labelledby={"plugin-lifecycle-tasks-tab-" + task}>{children}</div>
  </section>;
}

export function PluginCallEvidence({ plugin, result, onTools }: { plugin: InstalledPlugin; result: ToolExecutionResult | null; onTools: () => void }) {
  useLanguage();
  const current = result?.pluginId === plugin.id && result.version === plugin.activeVersion && result.digest === plugin.digest ? result : null;
  return <section className="plugins-panel-stack" aria-label={t("调用证据")}>
    <p className="plugins-notice">{t("仅显示此工作区中最近一次调用的返回结果；未读取持久调用历史。")}</p>
    {!current ? <EmptyState title={t("尚无本次调用证据")} description={t("选择此包的已授权客户端与工具，完成独立预览和确认后查看返回结果。")}
      action={<Button variant="secondary" onClick={onTools}>{t("查看可调用工具")}</Button>} />
      : <section className="plugins-review" aria-label={t("工具返回结果")}><h3>{t("工具返回结果")}</h3>
        <dl className="key-values"><dt>{t("插件标识")}</dt><dd><code>{current.pluginId}</code></dd>
          <dt>{t("版本")}</dt><dd><code>{current.version}</code></dd><dt>SHA-256</dt><dd><code className="plugins-digest">{current.digest}</code></dd>
          <dt>{t("调用客户端")}</dt><dd>{pluginClientLabels[current.client]}</dd><dt>{t("已验证工具")}</dt><dd>{current.tool}</dd></dl>
        <p className="plugins-notice">{current.redacted ? t("输出已检查并脱敏。") : t("输出已通过宿主隐私检查。")}</p>
        <pre className="plugins-text-preview">{typeof current.value === "string" ? current.value : JSON.stringify(current.value, null, 2)}</pre>
        <Button variant="secondary" onClick={onTools}>{t("返回可调用工具")}</Button>
      </section>}
  </section>;
}
