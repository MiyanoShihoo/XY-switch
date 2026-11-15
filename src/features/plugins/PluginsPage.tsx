import { useCallback, useEffect, useRef, useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { Plus, RefreshCw, Search, Settings2 } from "lucide-react";
import { useLanguage, t, translateMessage } from "../../i18n/language";
import { ConfirmActionDialog } from "../../components/ConfirmActionDialog";
import { Badge, Button, EmptyState, Field, InlineError, Input, LoadingState, Modal, Select, Tabs, Textarea } from "../../ui";
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
import { InstalledPluginCard, InstalledPluginDetails } from "./InstalledPluginCard";
import { PluginCallEvidence, PluginWorkspace, toolsForPackage, type PluginTask } from "./PluginWorkspace";
import { InstallReviewDialog, ToolReviewDialog, TrustedKeyDialog, WithdrawalReviewDialog } from "./PluginReviewDialogs";
import {
  confirmInstallReview, confirmKeyReview, confirmToolReview, confirmWithdrawalReview, readWithdrawalReview,
  type InstallChoices, type ToolDraft, type ToolReview, type TrustedKeyReview, type WithdrawalReview,
} from "./review";
import "./plugins.css";

interface InstallationReview { preview: InstallPreview; initialClients: PluginClient[]; source: string | null }
type PluginPanel = "import" | "trust" | "withdrawal";
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
  const [panel, setPanel] = useState<PluginPanel | null>(null);
  const [search, setSearch] = useState("");
  const [installationFilter, setInstallationFilter] = useState("all");
  const [selectedPluginId, setSelectedPluginId] = useState<string | null>(null);
  const [workspaceOpen, setWorkspaceOpen] = useState(false);
  const [task, setTask] = useState<PluginTask>("clients");
  const directoryHeading = useRef<HTMLHeadingElement>(null);
  const returnPoint = useRef<{ trigger: HTMLButtonElement; positions: { element: Element; top: number; left: number }[] } | null>(null);
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
    enabled: desktop && workspaceOpen && task === "tools" && toolClient !== "", retry: false, refetchOnWindowFocus: false });
  const busy = op.busy || !desktop;
  const hasDialog = keyEditor || installationReview !== null || withdrawalReview !== null || toolReview !== null || pendingAction !== null;
  const visible = installed.data?.filter(plugin => (installationFilter === "all" || (installationFilter === "installed" ? plugin.installed : !plugin.installed))
    && [plugin.id, plugin.manifest?.name, plugin.manifest?.description]
      .some(value => value?.toLowerCase().includes(search.trim().toLowerCase()))) ?? [];
  const selectedPlugin = visible.find(plugin => plugin.id === selectedPluginId);
  const inspecting = workspaceOpen && !!selectedPlugin;
  const packageTools = toolsForPackage(selectedPlugin, toolClient, tools.data);
  const selectedTool = packageTools.find(tool => toolKey(tool) === selectedToolKey);

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
  }, [clearReviews, installed.dataUpdatedAt, installed.errorUpdatedAt, keys.dataUpdatedAt, keys.errorUpdatedAt]);
  useEffect(() => { clearReviews(); }, [clearReviews, tools.dataUpdatedAt, tools.errorUpdatedAt]);
  useEffect(() => {
    if (!selectedPluginId || selectedPlugin) return;
    clearReviews(); setWorkspaceOpen(false); setSelectedPluginId(null); setPendingAction(null);
    setArgumentsJson("{}"); setSelectedToolKey(""); setToolClient(""); setToolResult(null);
    if (workspaceOpen) requestAnimationFrame(() => directoryHeading.current?.focus());
  }, [clearReviews, selectedPluginId, selectedPlugin, workspaceOpen]);

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
      if (review.source !== null) { setPublishedJson(""); setPanel(null); }
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
      if (mounted.current && generation.current === revision) {
        setToolResult(result); setTask("evidence"); setSelectedToolKey(""); setToolClient("");
      }
    });
  };

  const clearToolDraft = () => {
    clearReviews(); setArgumentsJson("{}"); setSelectedToolKey(""); setToolClient("");
  };
  const openPanel = (next: PluginPanel) => { clearToolDraft(); op.setError(""); op.setMessage(""); setPanel(next); };
  const selectTask = (next: PluginTask) => { clearToolDraft(); op.setError(""); op.setMessage(""); setTask(next); };
  const openTools = (client: PluginClient) => { selectTask("tools"); setToolClient(client); };
  const openPlugin = (plugin: InstalledPlugin, trigger: HTMLButtonElement) => {
    const positions: { element: Element; top: number; left: number }[] = [];
    for (let element: Element | null = trigger; element; element = element.parentElement) {
      positions.push({ element, top: element.scrollTop, left: element.scrollLeft });
    }
    returnPoint.current = { trigger, positions };
    clearToolDraft(); setToolResult(null); op.setError(""); op.setMessage("");
    setSelectedPluginId(plugin.id); setTask("clients"); setWorkspaceOpen(true);
  };
  const backToDirectory = () => {
    clearToolDraft(); setToolResult(null); setWorkspaceOpen(false);
    requestAnimationFrame(() => {
      const saved = returnPoint.current;
      (saved?.trigger.isConnected ? saved.trigger : directoryHeading.current)?.focus({ preventScroll: true });
      saved?.positions.forEach(({ element, top, left }) => { element.scrollTop = top; element.scrollLeft = left; });
    });
  };

  return <div className="workspace-page plugins-page">
    <header className="page-heading"><div className="page-heading__copy"><h1>{t("本地插件")}</h1>
      <p className="page-description">{t("选择一个包，管理客户端授权、版本与可调用工具。")}</p></div>
      <div className="page-actions"><Button variant="secondary" size="sm" icon={<Settings2 size={14} />} disabled={op.busy} onClick={() => openPanel("trust")}>{t("插件管理")}</Button>
        <Button size="sm" icon={<Plus size={14} />} disabled={busy} onClick={() => openPanel("import")}>{t("导入签名插件包")}</Button></div></header>
    {!desktop && <p className="plugins-notice" role="status">{t("本地插件安装、信任配置与执行需要桌面应用。")}</p>}
    {op.message && !panel && !hasDialog && !inspecting && <p className="plugins-notice" role="status">{translateMessage(op.message)}</p>}
    <InlineError message={panel || hasDialog || inspecting ? "" : translateMessage(op.error)} />
    <div className="plugins-workbench" data-inspecting={inspecting}>
    <section className="plugins-directory" aria-label={t("已安装插件")} hidden={inspecting}>
      <div className="plugins-directory-heading"><h2 ref={directoryHeading} tabIndex={-1}>{t("包目录")}</h2><span>{visible.length} / {installed.data?.length ?? 0}</span></div>
      <div className="plugins-directory-toolbar"><Input aria-label={t("搜索已安装插件")} leading={<Search size={14} />} placeholder={t("按名称、标识或描述搜索")} value={search} onChange={event => setSearch(event.target.value)} />
        <Button variant="secondary" size="sm" icon={<RefreshCw size={14} />} disabled={busy || installed.isFetching} onClick={() => { void run(async () => { clearReviews(); await refresh(); }); }}>{t("刷新")}</Button></div>
      <Tabs label={t("插件状态筛选")} value={installationFilter} onChange={setInstallationFilter} items={[
        { value: "all", label: t("全部"), count: installed.data?.length ?? 0 },
        { value: "installed", label: t("已安装"), count: installed.data?.filter(plugin => plugin.installed).length ?? 0 },
        { value: "history", label: t("仅历史记录"), count: installed.data?.filter(plugin => !plugin.installed).length ?? 0 },
      ]} />
      {desktop && installed.isPending && <LoadingState label={t("正在读取本地插件…")} />}
      {installed.error && <InlineError message={translateMessage(pluginErrorText(installed.error))} />}
      {!(desktop && installed.isPending) && !installed.error && !visible.length && <EmptyState title={installed.data?.length ? t("没有匹配的插件") : t("尚未安装本地插件")}
        description={installed.data?.length ? t("尝试其他名称，或清除搜索条件。") : t("先在插件管理中核对发布者信任，再导入签名包。")}
        action={installed.data?.length ? <Button variant="secondary" size="sm" onClick={() => { setSearch(""); setInstallationFilter("all"); }}>{t("清除筛选")}</Button>
          : <Button variant="secondary" size="sm" disabled={op.busy} onClick={() => openPanel("trust")}>{t("发布者信任")}</Button>} />}
      {visible.length > 0 && <div className="plugins-installed-list" role="list">{visible.map(plugin => <InstalledPluginCard key={plugin.id} plugin={plugin} busy={op.busy} selected={selectedPlugin?.id === plugin.id}
        onOpen={trigger => openPlugin(plugin, trigger)} />)}</div>}
    </section>
    {inspecting && selectedPlugin && <PluginWorkspace plugin={selectedPlugin} task={task} busy={op.busy} onTask={selectTask} onBack={backToDirectory}>
      <InlineError message={panel || hasDialog ? "" : translateMessage(op.error)} />
      {op.message && !panel && !hasDialog && <p className="plugins-notice" role="status">{translateMessage(op.message)}</p>}
      {(task === "clients" || task === "permissions" || task === "versions") && <InstalledPluginDetails key={selectedPlugin.id} plugin={selectedPlugin} busy={busy} task={task}
        onReviewClients={client => requestInstallationReview(() => previewInstalledPlugin(selectedPlugin.id), client ? [client] : [])}
        onDisable={client => requestAction({ kind: "disable", plugin: selectedPlugin, client })}
        onUninstall={() => requestAction({ kind: "uninstall", plugin: selectedPlugin })}
        onRollback={version => requestInstallationReview(() => previewPluginRollback(selectedPlugin.id, version))}
        onOpenTools={openTools} />}
        {task === "tools" && <section className="plugins-panel form-stack"><h3>{t("已验证工具调用")}</h3><p className="plugins-notice">{t("这里只列出此包在所选客户端已授权的当前版本工具。每次调用仍需独立预览和确认。")}</p>
      <div className="plugins-tool-selectors"><Field label={t("调用客户端")} required><Select value={toolClient} disabled={busy}
        onChange={event => { const next = event.target.value; if (next === "" || pluginClients.includes(next as PluginClient)) {
          clearReviews(); setToolResult(null); setSelectedToolKey(""); setArgumentsJson("{}"); setToolClient(next as PluginClient | "");
        } }}><option value="">{t("选择调用客户端")}</option>{selectedPlugin.manifest?.clients.map(client => <option key={client} value={client} disabled={!selectedPlugin.enabledClients.includes(client)}>{pluginClientLabels[client]}</option>)}</Select></Field>
        <Field label={t("已验证工具")} required><Select value={selectedToolKey} disabled={busy || !toolClient || tools.isPending}
          onChange={event => { clearReviews(); setToolResult(null); setArgumentsJson("{}"); setSelectedToolKey(event.target.value); }}>
          <option value="">{t("选择已验证工具")}</option>{packageTools.map(tool => <option key={toolKey(tool)} value={toolKey(tool)}>
            {tool.pluginId} · {tool.tool.name} ({tool.version})</option>)}</Select></Field></div>
      {tools.error && <InlineError message={translateMessage(pluginErrorText(tools.error))} />}
      {desktop && toolClient && tools.isPending && <LoadingState label={t("正在读取已验证工具…")} />}
      {toolClient && !tools.isPending && !tools.error && packageTools.length === 0 && <p>{t("此包在所选客户端没有可调用的已验证工具。")}</p>}
      <Button variant="quiet" onClick={() => selectTask("clients")} disabled={busy}>{t("审核客户端授权")}</Button>
      {selectedTool && <><p>{selectedTool.tool.description}</p><details><summary>{t("工具输入格式")}</summary>
        <pre className="plugins-text-preview">{JSON.stringify(selectedTool.tool.inputSchema, null, 2)}</pre></details></>}
      <Field label={t("工具参数 JSON")} required><Textarea value={argumentsJson} rows={5} maxLength={64 * 1024} disabled={busy || !selectedTool}
        spellCheck={false} onChange={event => { clearReviews(); setToolResult(null); setArgumentsJson(event.target.value); }} /></Field>
      <Button disabled={busy || !selectedTool || !argumentsJson.trim()} onClick={previewTool}>{t("预览本次工具调用")}</Button>
    </section>}
      {task === "evidence" && <PluginCallEvidence plugin={selectedPlugin} result={toolResult} onTools={() => selectTask("tools")} />}
    </PluginWorkspace>}
    </div>
    {panel && !hasDialog && <Modal open className="plugins-dialog" size="lg" title={panel === "import" ? t("导入签名插件包") : t("插件管理")} busy={op.busy} onClose={() => { clearReviews(); setPanel(null); setPublishedJson(""); setWithdrawalJson(""); }}>
      <div className="plugins-panel-stack">
        {panel !== "import" && <Tabs label={t("插件管理选项")} value={panel} onChange={value => openPanel(value as PluginPanel)} items={[
          { value: "trust", label: t("发布者信任"), disabled: op.busy },
          { value: "withdrawal", label: t("签名撤回通知"), disabled: op.busy },
        ]} />}
        <InlineError message={translateMessage(op.error)} />
        {op.message && <p className="plugins-notice" role="status">{translateMessage(op.message)}</p>}
        {panel === "trust" && <section className="plugins-panel form-stack"><header className="plugins-section-header"><h3>{t("发布者信任")}</h3>
        <Button variant="secondary" disabled={busy} onClick={() => { clearReviews(); op.setError(""); setKeyEditor(true); }}>{t("添加信任密钥")}</Button></header>
        <p className="plugins-notice">{t("信任只来自本地配置的公钥；请独立核对指纹。")}</p>
        {desktop && keys.isPending && <LoadingState label={t("正在读取信任密钥…")} />}
        {keys.error && <InlineError message={translateMessage(pluginErrorText(keys.error))} />}
        {keys.data?.length === 0 && <p>{t("尚未配置信任密钥。")}</p>}
        {keys.data?.map(key => <div className="plugins-trusted-key" key={key.id}><div className="plugins-section-header">
          <strong>{key.label} · {key.id}</strong><Badge tone={key.enabled ? "success" : "warning"}>{key.enabled ? t("已信任") : t("已吊销")}</Badge>
          <Button variant="danger" size="sm" disabled={busy || !key.enabled} onClick={() => requestAction({ kind: "revoke", key })}>{t("吊销信任")}</Button></div>
          <code className="plugins-digest">{key.fingerprint}</code></div>)}
      </section>}
        {panel === "import" && <section className="plugins-panel form-stack"><h3>{t("导入签名插件包")}</h3><p className="plugins-notice">{t("仅接收 UTF-8 JSON 文本包，最多 2 MiB；所有文件都会接受路径与隐私检查。")}</p>
        <Field label={t("选择签名包 JSON 文件")}><input type="file" accept=".json,application/json" disabled={busy}
          onChange={event => { const file = event.target.files?.[0]; event.target.value = ""; if (file) importDocument(file); }} /></Field>
        <Field label={t("签名包 JSON")} description={t("保留原始 JSON；宿主会拒绝重复字段和篡改的摘要，包内公钥不能建立信任。")}
          required><Textarea value={publishedJson} rows={7} maxLength={maxPluginEnvelopeBytes} spellCheck={false} disabled={busy}
          onChange={event => { clearReviews(); setPublishedJson(event.target.value); }} /></Field>
        <Button disabled={busy || !publishedJson.trim()} onClick={() => {
          const source = publishedJson; requestInstallationReview(() => previewPluginInstall(source), [], source);
        }}>{t("验证签名并预览安装")}</Button>
      </section>}
        {panel === "withdrawal" && <section className="plugins-panel form-stack"><h3>{t("签名撤回通知")}</h3><p className="plugins-notice">{t("导入发布者签名的撤回通知。宿主会核对版本、摘要、原因及原发布者密钥。")}</p>
      <Field label={t("撤回通知 JSON")} required><Textarea value={withdrawalJson} rows={4} maxLength={4096} disabled={busy} spellCheck={false}
        onChange={event => { clearReviews(); setWithdrawalJson(event.target.value); }} /></Field>
      <Button variant="secondary" disabled={busy || !withdrawalJson.trim()} onClick={requestWithdrawalReview}>{t("审核撤回通知")}</Button></section>}
      </div>
    </Modal>}
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
