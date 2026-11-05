import { useCallback, useEffect, useRef, useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useLanguage, t, translateMessage } from "../../i18n/language";
import { ConfirmActionDialog } from "../../components/ConfirmActionDialog";
import { Badge, Button, Card, Field, InlineError, Select, Textarea } from "../../ui";
import {
  applyPluginWithdrawal, configureTrustedPluginKey, confirmPluginInstall, disablePlugin, executePluginTool,
  isPluginDesktop, listInstalledPlugins, listTrustedPluginKeys, maxPluginEnvelopeBytes, PluginServiceError,
  pluginErrorText, pluginQueryKeys, pluginToolsForClient, previewInstalledPlugin, previewPluginInstall,
  previewPluginRollback, previewPluginTool, previewTrustedPluginKey, readPluginDocument, revokeTrustedPluginKey,
  uninstallPlugin,
} from "../../lib/plugins-service";
import {
  pluginClients, type InstalledPlugin, type InstallPreview, type PluginClient, type ToolExecutionResult,
  type TrustedKeyInput, type TrustedSigningKey, type VerifiedTool,
} from "../../types/plugins";
import { usePageOperation } from "../shared/usePageOperation";
import { pluginClientLabels } from "./clients";
import { InstalledPluginCard } from "./InstalledPluginCard";
import { InstallReviewDialog, ToolReviewDialog, TrustedKeyDialog, WithdrawalReviewDialog } from "./PluginReviewDialogs";
import {
  confirmInstallReview, confirmKeyReview, confirmToolReview, confirmWithdrawalReview, readWithdrawalReview,
  type InstallChoices, type ToolDraft, type ToolReview, type TrustedKeyReview, type WithdrawalReview,
} from "./review";
import "./plugins.css";

interface InstallationReview { preview: InstallPreview; initialClients: PluginClient[]; source: string | null }
type PendingAction = { kind: "revoke"; key: TrustedSigningKey }
  | { kind: "uninstall"; plugin: InstalledPlugin }
  | { kind: "disable"; plugin: InstalledPlugin; client: PluginClient | null };

function toolKey(tool: VerifiedTool): string {
  return JSON.stringify([tool.client, tool.pluginId, tool.version, tool.digest, tool.tool.name]);
}

function draftFor(tool: VerifiedTool, argumentsJson: string): ToolDraft {
  return { pluginId: tool.pluginId, version: tool.version, pluginDigest: tool.digest, client: tool.client,
    tool: tool.tool.name, argumentsJson };
}

export function PluginsPage() {
  useLanguage();
  const desktop = isPluginDesktop();
  const queryClient = useQueryClient();
  const op = usePageOperation();
  const generation = useRef(0);
  const mounted = useRef(true);
  const [publishedJson, setPublishedJson] = useState("");
  const [withdrawalJson, setWithdrawalJson] = useState("");
  const [keyEditor, setKeyEditor] = useState(false);
  const [keyInput, setKeyInput] = useState<TrustedKeyInput>({ id: "", label: "", publicKey: "" });
  const [keyReview, setKeyReview] = useState<TrustedKeyReview | null>(null);
  const [installationReview, setInstallationReview] = useState<InstallationReview | null>(null);
  const [withdrawalReview, setWithdrawalReview] = useState<WithdrawalReview | null>(null);
  const [toolReview, setToolReview] = useState<ToolReview | null>(null);
  const [pendingAction, setPendingAction] = useState<PendingAction | null>(null);
  const [toolClient, setToolClient] = useState<PluginClient | "">("");
  const [selectedToolKey, setSelectedToolKey] = useState("");
  const [argumentsJson, setArgumentsJson] = useState("{}");
  const [toolResult, setToolResult] = useState<ToolExecutionResult | null>(null);
  const installed = useQuery({ queryKey: pluginQueryKeys.installed, queryFn: listInstalledPlugins,
    enabled: desktop, retry: false, refetchOnWindowFocus: false });
  const keys = useQuery({ queryKey: pluginQueryKeys.keys, queryFn: listTrustedPluginKeys,
    enabled: desktop, retry: false, refetchOnWindowFocus: false });
  const tools = useQuery({ queryKey: pluginQueryKeys.tools(toolClient),
    queryFn: () => toolClient ? pluginToolsForClient(toolClient) : Promise.resolve([]),
    enabled: desktop && toolClient !== "", retry: false, refetchOnWindowFocus: false });
  const selectedTool = tools.data?.find(tool => tool.client === toolClient && toolKey(tool) === selectedToolKey);
  const busy = op.busy || !desktop;
  const hasDialog = keyEditor || installationReview !== null || withdrawalReview !== null || toolReview !== null || pendingAction !== null;

  const clearReviews = useCallback(() => {
    generation.current += 1;
    setKeyReview(null); setInstallationReview(null); setWithdrawalReview(null); setToolReview(null);
    return generation.current;
  }, []);
  useEffect(() => {
    mounted.current = true;
    return () => { mounted.current = false; generation.current += 1; };
  }, []);
  useEffect(() => {
    clearReviews(); setToolResult(null);
  }, [clearReviews, installed.dataUpdatedAt, installed.errorUpdatedAt, keys.dataUpdatedAt, keys.errorUpdatedAt,
    tools.dataUpdatedAt, tools.errorUpdatedAt]);

  const run = (work: () => Promise<void>, success = "") => op.run(async () => {
    try { await work(); } catch (cause) { throw new Error(pluginErrorText(cause)); }
  }, success);
  const refresh = async () => { await queryClient.invalidateQueries({ queryKey: pluginQueryKeys.all }); };

  const requestInstallationReview = (load: () => Promise<InstallPreview>, initialClients: PluginClient[] = [], source: string | null = null) => {
    void run(async () => {
      const revision = clearReviews();
      const preview = await load();
      if (mounted.current && generation.current === revision) setInstallationReview({ preview, initialClients, source });
    });
  };
  const confirmInstallation = (choices: InstallChoices) => {
    const review = installationReview;
    if (!review) return;
    void run(async () => {
      if (review.source !== null && review.source !== publishedJson) throw new PluginServiceError("plugin_confirmation");
      const confirmation = confirmInstallReview(review.preview, choices);
      clearReviews();
      await confirmPluginInstall(confirmation);
      if (review.source !== null) setPublishedJson("");
      await refresh();
    }, t("插件安装状态已更新。"));
  };

  const importDocument = (file: File) => {
    void run(async () => {
      const revision = clearReviews();
      const text = await readPluginDocument(file);
      if (mounted.current && generation.current === revision) setPublishedJson(text);
    });
  };
  const previewKey = () => {
    const input = { ...keyInput };
    void run(async () => {
      const revision = clearReviews();
      const preview = await previewTrustedPluginKey(input);
      if (mounted.current && generation.current === revision) setKeyReview({ input, preview });
    });
  };
  const confirmKey = (fingerprint: string, independentSource: boolean) => {
    const review = keyReview;
    if (!review) return;
    void run(async () => {
      const input = confirmKeyReview(review, keyInput, fingerprint, independentSource);
      clearReviews();
      await configureTrustedPluginKey(input, fingerprint, true);
      setKeyEditor(false); setKeyInput({ id: "", label: "", publicKey: "" });
      await refresh();
    }, t("发布者信任密钥已保存。"));
  };

  const requestAction = (action: PendingAction) => {
    clearReviews(); op.setError(""); op.setMessage(""); setPendingAction(action);
  };
  const confirmAction = () => {
    const action = pendingAction;
    if (!action) return;
    void (async () => {
      const succeeded = await run(async () => {
        clearReviews();
        if (action.kind === "revoke") await revokeTrustedPluginKey(action.key.id);
        else if (action.kind === "uninstall") await uninstallPlugin(action.plugin.id, true);
        else await disablePlugin(action.plugin.id, action.client);
        await refresh();
      }, t("插件管理操作已完成。"));
      if (succeeded && mounted.current) setPendingAction(null);
    })();
  };

  const requestWithdrawalReview = () => {
    void run(async () => { clearReviews(); setWithdrawalReview(readWithdrawalReview(withdrawalJson)); });
  };
  const confirmWithdrawal = (accepted: boolean) => {
    const review = withdrawalReview;
    if (!review) return;
    void run(async () => {
      const raw = confirmWithdrawalReview(review, withdrawalJson, accepted);
      clearReviews();
      await applyPluginWithdrawal(raw);
      setWithdrawalJson("");
      await refresh();
    }, t("撤回通知已验证并应用。"));
  };

  const previewTool = () => {
    if (!selectedTool) return;
    const draft = draftFor(selectedTool, argumentsJson);
    void run(async () => {
      const revision = clearReviews(); setToolResult(null);
      const preview = await previewPluginTool(draft.pluginId, draft.client, draft.tool, draft.argumentsJson);
      if (mounted.current && generation.current === revision) setToolReview({ draft, preview });
    });
  };
  const confirmTool = (argumentsDigest: string, accepted: boolean) => {
    const review = toolReview;
    if (!review || !selectedTool) return;
    const current = draftFor(selectedTool, argumentsJson);
    void run(async () => {
      const confirmation = confirmToolReview(review, current, argumentsDigest, accepted);
      const revision = clearReviews(); setArgumentsJson("{}"); setToolResult(null);
      const result = await executePluginTool(current.client, confirmation);
      if (mounted.current && generation.current === revision) setToolResult(result);
    });
  };

  return <div className="plugins-page">
    <header className="plugins-section-header"><div><h2>{t("本地插件")}</h2>
      <p>{t("导入已签名的文本插件，按客户端授权，并逐次确认工具调用。")}</p></div>
      <Button variant="secondary" disabled={busy} onClick={() => { void run(async () => { clearReviews(); await refresh(); }); }}>{t("刷新")}</Button></header>
    {!desktop && <p className="plugins-notice" role="status">{t("本地插件安装、信任配置与执行需要桌面应用。")}</p>}
    {op.message && <p className="plugins-notice" role="status">{translateMessage(op.message)}</p>}
    <InlineError message={hasDialog ? "" : translateMessage(op.error)} />
    <div className="plugins-import-grid">
      <Card><header className="plugins-section-header"><h3>{t("发布者信任")}</h3>
        <Button variant="secondary" disabled={busy} onClick={() => { clearReviews(); op.setError(""); setKeyEditor(true); }}>{t("添加信任密钥")}</Button></header>
        <p className="plugins-notice">{t("信任只来自本地配置的公钥；请独立核对指纹。")}</p>
        {desktop && keys.isPending && <p role="status">{t("正在读取信任密钥…")}</p>}
        {keys.error && <InlineError message={translateMessage(pluginErrorText(keys.error))} />}
        {keys.data?.length === 0 && <p>{t("尚未配置信任密钥。")}</p>}
        {keys.data?.map(key => <div className="plugins-trusted-key" key={key.id}><div className="plugins-section-header">
          <strong>{key.label} · {key.id}</strong><Badge tone={key.enabled ? "success" : "warning"}>{key.enabled ? t("已信任") : t("已吊销")}</Badge>
          <Button variant="danger" size="sm" disabled={busy || !key.enabled} onClick={() => requestAction({ kind: "revoke", key })}>{t("吊销信任")}</Button></div>
          <code className="plugins-digest">{key.fingerprint}</code></div>)}
      </Card>
      <Card><h3>{t("导入签名插件包")}</h3><p className="plugins-notice">{t("仅接收 UTF-8 JSON 文本包，最多 2 MiB；所有文件都会接受路径与隐私检查。")}</p>
        <Field label={t("选择签名包 JSON 文件")}><input type="file" accept=".json,application/json" disabled={busy}
          onChange={event => { const file = event.target.files?.[0]; event.target.value = ""; if (file) importDocument(file); }} /></Field>
        <Field label={t("签名包 JSON")} description={t("保留原始 JSON；宿主会拒绝重复字段和篡改的摘要，包内公钥不能建立信任。")}
          required><Textarea value={publishedJson} rows={7} maxLength={maxPluginEnvelopeBytes} spellCheck={false} disabled={busy}
          onChange={event => { clearReviews(); setPublishedJson(event.target.value); }} /></Field>
        <Button disabled={busy || !publishedJson.trim()} onClick={() => {
          const source = publishedJson; requestInstallationReview(() => previewPluginInstall(source), [], source);
        }}>{t("验证签名并预览安装")}</Button>
      </Card>
    </div>
    <section aria-label={t("已安装插件")}><header className="plugins-section-header"><h3>{t("已安装插件")}</h3>
      <span>{t("已安装及历史记录：{count}", { count: installed.data?.length ?? 0 })}</span></header>
      {desktop && installed.isPending && <p role="status">{t("正在读取本地插件…")}</p>}
      {installed.error && <InlineError message={translateMessage(pluginErrorText(installed.error))} />}
      {installed.data?.length === 0 && <Card><p>{t("尚未安装本地插件。请先配置信任密钥，再导入签名包。")}</p></Card>}
      <div className="plugins-installed-list">{installed.data?.map(plugin => <InstalledPluginCard key={plugin.id} plugin={plugin} busy={busy}
        onReviewClients={client => requestInstallationReview(() => previewInstalledPlugin(plugin.id), client ? [client] : [])}
        onDisable={client => requestAction({ kind: "disable", plugin, client })}
        onUninstall={() => requestAction({ kind: "uninstall", plugin })}
        onRollback={version => requestInstallationReview(() => previewPluginRollback(plugin.id, version))} />)}</div>
    </section>
    <Card><h3>{t("已验证工具调用")}</h3><p className="plugins-notice">{t("这里只列出所选客户端已启用且验证通过的工具。安装授权不会自动批准工具调用。")}</p>
      <div className="plugins-tool-selectors"><Field label={t("调用客户端")} required><Select value={toolClient} disabled={busy}
        onChange={event => { const next = event.target.value; if (next === "" || pluginClients.includes(next as PluginClient)) {
          clearReviews(); setToolResult(null); setSelectedToolKey(""); setArgumentsJson("{}"); setToolClient(next as PluginClient | "");
        } }}><option value="">{t("选择调用客户端")}</option>{pluginClients.map(client => <option key={client} value={client}>{pluginClientLabels[client]}</option>)}</Select></Field>
        <Field label={t("已验证工具")} required><Select value={selectedToolKey} disabled={busy || !toolClient || tools.isPending}
          onChange={event => { clearReviews(); setToolResult(null); setArgumentsJson("{}"); setSelectedToolKey(event.target.value); }}>
          <option value="">{t("选择已验证工具")}</option>{tools.data?.filter(tool => tool.client === toolClient).map(tool => <option key={toolKey(tool)} value={toolKey(tool)}>
            {tool.pluginId} · {tool.tool.name} ({tool.version})</option>)}</Select></Field></div>
      {tools.error && <InlineError message={translateMessage(pluginErrorText(tools.error))} />}
      {desktop && toolClient && tools.isPending && <p role="status">{t("正在读取已验证工具…")}</p>}
      {toolClient && tools.data?.length === 0 && <p>{t("此客户端没有已启用且验证通过的工具。")}</p>}
      {selectedTool && <><p>{selectedTool.tool.description}</p><details><summary>{t("工具输入格式")}</summary>
        <pre className="plugins-text-preview">{JSON.stringify(selectedTool.tool.inputSchema, null, 2)}</pre></details></>}
      <Field label={t("工具参数 JSON")} required><Textarea value={argumentsJson} rows={5} maxLength={64 * 1024} disabled={busy || !selectedTool}
        spellCheck={false} onChange={event => { clearReviews(); setToolResult(null); setArgumentsJson(event.target.value); }} /></Field>
      <Button disabled={busy || !selectedTool || !argumentsJson.trim()} onClick={previewTool}>{t("预览本次工具调用")}</Button>
      {toolResult && <section className="plugins-review" aria-label={t("工具返回结果")}><h4>{t("工具返回结果")}</h4>
        <p>{toolResult.pluginId} · {toolResult.tool} · {pluginClientLabels[toolResult.client]}</p>
        <p className="plugins-notice">{toolResult.redacted ? t("输出已检查并脱敏。") : t("输出已通过宿主隐私检查。")}</p>
        <pre className="plugins-text-preview">{typeof toolResult.value === "string" ? toolResult.value : JSON.stringify(toolResult.value, null, 2)}</pre></section>}
    </Card>
    <Card><h3>{t("签名撤回通知")}</h3><p className="plugins-notice">{t("导入发布者签名的撤回通知。宿主会核对版本、摘要、原因及原发布者密钥。")}</p>
      <Field label={t("撤回通知 JSON")} required><Textarea value={withdrawalJson} rows={4} maxLength={4096} disabled={busy} spellCheck={false}
        onChange={event => { clearReviews(); setWithdrawalJson(event.target.value); }} /></Field>
      <Button variant="secondary" disabled={busy || !withdrawalJson.trim()} onClick={requestWithdrawalReview}>{t("审核撤回通知")}</Button></Card>
    {keyEditor && <TrustedKeyDialog input={keyInput} review={keyReview} busy={op.busy} error={op.error}
      onChange={input => { clearReviews(); setKeyInput(input); }} onPreview={previewKey} onConfirm={confirmKey}
      onClose={() => { clearReviews(); setKeyEditor(false); setKeyInput({ id: "", label: "", publicKey: "" }); }} />}
    {installationReview && <InstallReviewDialog key={installationReview.preview.previewId} preview={installationReview.preview}
      initialClients={installationReview.initialClients} busy={op.busy} error={op.error} onConfirm={confirmInstallation} onClose={clearReviews} />}
    {toolReview && selectedTool && <ToolReviewDialog key={toolReview.preview.previewId} review={toolReview} operation={selectedTool.tool.operation.type}
      busy={op.busy} error={op.error} onConfirm={confirmTool} onClose={() => { clearReviews(); setArgumentsJson("{}"); }} />}
    {withdrawalReview && <WithdrawalReviewDialog review={withdrawalReview} busy={op.busy} error={op.error}
      onConfirm={confirmWithdrawal} onClose={clearReviews} />}
    {pendingAction && <ConfirmActionDialog busy={op.busy} error={translateMessage(op.error)} onCancel={() => setPendingAction(null)} onConfirm={confirmAction}
      title={pendingAction.kind === "revoke" ? t("吊销发布者信任") : pendingAction.kind === "uninstall" ? t("卸载插件") : t("停用插件")}
      confirmLabel={pendingAction.kind === "revoke" ? t("确认吊销") : pendingAction.kind === "uninstall" ? t("确认卸载") : t("确认停用")}
      message={pendingAction.kind === "revoke" ? t("吊销 {publisher} 的信任后，依赖此密钥的插件将停止运行，待确认调用会作废。", { publisher: pendingAction.key.label })
        : pendingAction.kind === "uninstall" ? t("卸载 {plugin} 会删除活跃指针和客户端授权；保留不可变签名历史以便重新审核回滚。", { plugin: pendingAction.plugin.id })
        : pendingAction.client ? t("停用 {plugin} 的 {client} 授权？待确认工具调用会立即作废。", { plugin: pendingAction.plugin.id, client: pluginClientLabels[pendingAction.client] })
          : t("停用 {plugin} 的全部客户端授权？待确认工具调用会立即作废。", { plugin: pendingAction.plugin.id })} />}
  </div>;
}
