import { useLanguage, t, translateMessage } from "../../i18n/language";
import { useRef, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { Copy, Download, Upload } from "lucide-react";
import { Badge, Button, InlineError, LoadingState, Toast } from "../../ui";
import { ConfirmActionDialog } from "../../components/ConfirmActionDialog";
import { downloadConfig, exportConfig, importConfig } from "../../lib/config-service";
import { loadAppPaths, loadAppVersion } from "../../lib/app-info-service";
import { errorText } from "../../lib/error";
import { usePageOperation } from "../shared/usePageOperation";
import { ProjectRepository } from "./ProjectRepository";

export function DataSettings({ desktop, onImported }: { desktop: boolean; onImported: () => Promise<void> }) {
  useLanguage();
  const fileInput = useRef<HTMLInputElement>(null);
  const [pendingFile, setPendingFile] = useState<File | null>(null);
  const operation = usePageOperation();
  const confirmImport = () => operation.run(async () => {
    if (!pendingFile) return;
    let result;
    try { result = await importConfig(await pendingFile.text()); }
    catch (cause) {
      // Disk or client-sync failure can follow a committed section. Refresh even
      // on failure so the workbench never hides changes that actually happened.
      try { await onImported(); }
      catch (refreshCause) { throw new Error(errorText(cause) + t("；读取当前配置失败：") + errorText(refreshCause)); }
      throw cause;
    }
    // The write succeeded. Close the confirmation before refreshing, so a refresh failure cannot invite a duplicate import.
    setPendingFile(null);
    operation.setMessage(t("配置已导入：供应商 {providers} 个、提示词 {prompts} 个、自定义定价 {pricing} 条、MCP {mcp} 个{routing}", { providers: result.providers, prompts: result.prompts, pricing: result.modelPricing, mcp: result.mcpServers, routing: result.proxyImported ? t("，包含路由设置。") : t("。") }));
    try { await onImported(); }
    catch (cause) { throw new Error(t("导入已完成，但界面刷新失败：") + errorText(cause)); }
  });
  return <div className="settings-sections"><section className="settings-section"><div className="settings-section__heading"><h2>{t("配置备份")}</h2><p>{t("导入前确认文件；导入后刷新当前工作台数据。")}</p></div><div className="form-stack">
    <p>{t("导出的 v2 JSON 包含供应商、提示词、自定义定价、MCP 与路由设置，仍可导入旧版 v1 备份。供应商 API Key 单独保存在凭据管理器；参考价目录和历史用量不随配置备份迁移。")}</p>
    <p>{t("导入前会先校验整包配置；磁盘写入或客户端同步失败可能只完成部分导入，错误信息会注明进度。")}</p>
    <div className="inline-actions"><Button icon={<Download size={15} />} variant="secondary" disabled={!desktop || operation.busy} onClick={() => void operation.run(async () => downloadConfig(await exportConfig()), t("已生成配置导出文件"))}>{t("导出配置")}</Button><Button icon={<Upload size={15} />} disabled={!desktop || operation.busy} onClick={() => fileInput.current?.click()}>{t("导入配置")}</Button></div>
    <input hidden ref={fileInput} type="file" accept=".json,application/json" aria-label={t("选择配置备份")} disabled={!desktop || operation.busy} onChange={event => { const file = event.target.files?.[0]; event.currentTarget.value = ""; if (file) { operation.setError(""); operation.setMessage(""); setPendingFile(file); } }} />
    <InlineError message={operation.error} />{operation.message && <Toast title={operation.message} tone={operation.error ? "warning" : "success"} onDismiss={() => operation.setMessage("")} />}
    {translateMessage(operation.error).startsWith(t("导入已完成")) && <Button variant="secondary" loading={operation.busy} onClick={() => void operation.run(onImported, t("工作台数据已刷新"))}>{t("重新刷新工作台")}</Button>}
  </div></section><AppInformation pathsOnly />
    {pendingFile && <ConfirmActionDialog title={t("导入配置备份？")} message={t("文件：") + pendingFile.name + t("。同 ID 的配置和自定义定价可能被更新，MCP 选择可能同步到客户端，启用的路由配置可能启动监听。凭据不会从此文件恢复，历史费用不会重算。")} confirmLabel={t("确认导入")} busy={operation.busy} error={operation.error} onCancel={() => setPendingFile(null)} onConfirm={() => void confirmImport()} />}
  </div>;
}

export function AppInformation({ pathsOnly = false }: { pathsOnly?: boolean }) {
  useLanguage();
  return pathsOnly ? <DataLocations /> : <AboutApplication />;
}

function AboutApplication() {
  useLanguage();
  const version = useQuery({ queryKey: ["app-version"], queryFn: loadAppVersion, retry: false, refetchOnWindowFocus: false });
  return <section className="settings-section"><div className="settings-section__heading"><h2>{t("关于 XY Switch")}</h2><p>{t("让不同 AI 客户端共用一套清晰、可控的模型连接配置。")}</p></div><div className="form-stack">
    <div className="inline-actions"><strong>XY Switch</strong><Badge>{version.data ? "v" + version.data : t("本地工作台")}</Badge><Badge>MIT</Badge></div>
    <p>{t("XY Switch 是本地 AI 配置管理与协议路由工具，提供 Windows / macOS / Linux 桌面构建和 Android / iOS 移动端构建入口。集中管理供应商、API 端点和模型映射，减少在多个客户端之间反复修改配置的工作。")}</p>
    <ul><li>{t("供应商切换与模型探索：统一选择当前上游，按需配置模型、请求头和请求体覆盖。")}</li><li>{t("协议路由：连接 Responses、Chat Completions 与 Messages，并为已授权接管的客户端同步配置。")}</li><li>{t("工作资源：管理 MCP 与本地提示词，按平台授权读取本地 Skills；桌面端支持 Codex、Claude Code 和 Gemini CLI 会话日志。")}</li><li>{t("用量审计：查看请求、token、缓存、成本记录与模型字段；支持 Codex 和 DeepSeek Harness 本地用量读取。")}</li></ul>
    <ProjectRepository />
    {version.error && <InlineError message={t("版本读取失败：") + errorText(version.error)} onRetry={() => void version.refetch()} />}
  </div></section>;
}

function DataLocations() {
  useLanguage();
  const pathsOnly = true;
  const paths = useQuery({ queryKey: ["app-paths"], queryFn: loadAppPaths, retry: false, refetchOnWindowFocus: false });
  const operation = usePageOperation();
  return <section className="settings-section"><div className="settings-section__heading"><h2>{pathsOnly ? t("数据位置") : t("关于 XY Switch")}</h2><p>{pathsOnly ? t("以下路径由当前进程返回，不使用机器路径占位。") : t("本地 AI 客户端配置与协议路由工作台。")}</p></div><div className="form-stack">
    {paths.isPending && <LoadingState label={t("正在读取数据路径…")} />}{paths.error && <InlineError message={t("路径读取失败：") + errorText(paths.error)} onRetry={() => void paths.refetch()} />}
    {paths.data ? <dl className="settings-path-list">{([[t("数据目录"), paths.data.dataDir], [t("数据库"), paths.data.databasePath], [t("用户目录"), paths.data.homeDir], [t("Codex 配置目录"), paths.data.codexHome]] as const).map(([label, value]) => <div key={label}><dt>{label}</dt><dd><code>{value}</code><Button variant="quiet" size="sm" icon={<Copy size={14} />} aria-label={t("复制") + label} onClick={() => void operation.run(() => navigator.clipboard.writeText(value), label + t("已复制"))}>{t("复制")}</Button></dd></div>)}</dl> : !paths.isPending && !paths.error && <p className="muted">{t("浏览器预览不提供本机数据路径。")}</p>}
    <InlineError message={operation.error} />{operation.message && <Toast title={operation.message} tone="success" onDismiss={() => operation.setMessage("")} />}
  </div></section>;
}
