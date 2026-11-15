import { translateMessage, useLanguage, t } from "../../i18n/language";
import { useMemo, useRef, useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { Download, Pencil, Plus, RefreshCw, Search, Trash2 } from "lucide-react";
import { loadMcpServers, loadMcpApps, saveMcpServer, deleteMcpServer, setMcpAppEnabled, importMcpFromApps } from "../../lib/mcp-service";
import { formatKeyValueLines, isSharedMcpServerId, parseKeyValueLines, validateMcpServer } from "../../lib/mcp-validation";
import { errorText } from "../../lib/error";
import type { McpApp, McpAppStatus, McpServer, McpTransport } from "../../types/mcp";
import { SHARED_MCP_QUERY_KEYS, isSharedMcpDesktopRuntime, loadSharedMcpStatus } from "../../lib/shared-mcp-service";
import { ConfirmActionDialog } from "../../components/ConfirmActionDialog";
import { Badge, Button, EmptyState, Field, InlineError, Input, LoadingState, Modal, Select, Tabs, Textarea, Toast, Toggle } from "../../ui";
import { usePageOperation } from "../shared/usePageOperation";
import { SharedMcpPanel } from "./SharedMcpPanel";
import { McpMountMatrix, mcpApplications as applications } from "./McpMountMatrix";
import "./mcp.css";


const emptyServer = (): McpServer => ({ id: "mcp-" + crypto.randomUUID().slice(0, 8), name: "", description: "", transport: "stdio", command: "npx", args: [], url: null, env: {}, headers: {}, apps: { claude: true, codex: true, gemini: true, opencode: false, "claude-desktop": false }, builtIn: false, secretsConfigured: false });
const presets = [{ name: "Memory", description: "持久化知识图谱记忆", args: ["-y", "@modelcontextprotocol/server-memory"] }, { name: "Filesystem", description: "按指定目录开放文件系统访问", args: ["-y", "@modelcontextprotocol/server-filesystem", "C:\\Users"] }, { name: "Fetch", description: "网页与 API 抓取", args: ["-y", "@modelcontextprotocol/server-fetch"] }, { name: "GitHub", description: "仓库与 Issue 操作", args: ["-y", "@modelcontextprotocol/server-github"] }];

export function McpPage() {
  useLanguage();
  const client = useQueryClient();
  const servers = useQuery({ queryKey: ["mcp", "servers"], queryFn: loadMcpServers });
  const apps = useQuery({ queryKey: ["mcp", "apps"], queryFn: loadMcpApps });
  const shared = useQuery({ queryKey: SHARED_MCP_QUERY_KEYS.status, queryFn: loadSharedMcpStatus, enabled: isSharedMcpDesktopRuntime(), retry: false, refetchInterval: 5000 });
  const op = usePageOperation();
  const [area, setArea] = useState("connections");
  const [search, setSearch] = useState("");
  const [transport, setTransport] = useState("all");
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [editor, setEditor] = useState<{ server: McpServer | null } | null>(null);
  const [pendingDelete, setPendingDelete] = useState<McpServer | null>(null);
  const [importOpen, setImportOpen] = useState(false);
  const [importErrors, setImportErrors] = useState<string[]>([]);
  const list = servers.data ?? [];
  const pendingCount = shared.data?.running ? shared.data.pendingCount : 0;
  const visible = useMemo(() => list.filter(server => (transport === "all" || server.transport === transport) && [server.name, server.description, server.command, server.url].some(value => (value ?? "").toLowerCase().includes(search.trim().toLowerCase()))), [list, search, transport]);
  const selected = list.find(server => server.id === selectedId && !isSharedMcpServerId(server.id));
  const refresh = async () => { await client.invalidateQueries({ queryKey: ["mcp"] }); };
  const toggleMount = (server: McpServer, app: McpApp, enabled: boolean) => {
    if (isSharedMcpServerId(server.id)) return;
    void op.run(async () => { await setMcpAppEnabled(server.id, app, enabled); await refresh(); }, (applications.find(candidate => candidate.id === app)?.label ?? app) + (enabled ? t("已启用挂载") : t("已停用挂载")));
  };
  const importFromApps = async () => {
    const done = await op.run(async () => { const result = await importMcpFromApps(); setImportErrors(result.errors); op.setMessage(t("已导入 ") + result.imported + t(" 个 MCP 服务器") + (result.errors.length ? t("，部分应用导入失败") : "")); await refresh(); });
    if (done) setImportOpen(false);
  };
  const remove = async () => {
    if (!pendingDelete || isSharedMcpServerId(pendingDelete.id) || pendingDelete.builtIn) return;
    const target = pendingDelete;
    if (await op.run(async () => { await deleteMcpServer(target.id); await refresh(); }, t("已删除 ") + target.name)) { setPendingDelete(null); setSelectedId(null); }
  };
  return <div className="workspace-page mcp-page">
    <header className="page-heading"><div className="page-heading__copy"><h1>MCP</h1><p className="page-description">{t("集中查看工具连接，按需调整应用挂载与共享授权。")}</p></div>
      <div className="page-actions"><Button variant="secondary" size="sm" icon={<Download size={14} />} disabled={op.busy} onClick={() => { op.setError(""); setImportOpen(true); }}>{t("从应用导入")}</Button>
        <Button size="sm" icon={<Plus size={14} />} disabled={op.busy} onClick={() => { setSelectedId(null); setEditor({ server: null }); }}>{t("添加 MCP")}</Button></div></header>
    <Tabs id="mcp-areas" label={t("MCP 工作区")} value={area} onChange={setArea} items={[{ value: "connections", label: t("服务挂载"), count: list.length }, { value: "shared", label: t("共享插件服务"), count: pendingCount || undefined }]} />
    {area === "connections" ? <section className="mcp-area" id="mcp-areas-panel-connections" role="tabpanel" aria-labelledby="mcp-areas-tab-connections">
      {pendingCount > 0 && <div className="mcp-review-notice" role="status"><span>{t("共享工具调用等待审核")} <Badge tone="warning">{pendingCount}</Badge></span><Button variant="secondary" size="sm" onClick={() => setArea("shared")}>{t("前往审核")}</Button></div>}
      {shared.error && <div className="mcp-review-notice"><span>{t("共享服务状态暂不可用")}</span><Button variant="quiet" size="sm" onClick={() => setArea("shared")}>{t("查看共享服务")}</Button></div>}
      <InlineError message={(importOpen || pendingDelete ? "" : op.error) || (servers.error ? t("服务器读取失败：") + errorText(servers.error) : apps.error ? t("应用状态读取失败：") + errorText(apps.error) : "")} onRetry={() => void refresh()} retrying={servers.isFetching || apps.isFetching} />
      {op.message && <Toast title={op.message} tone={importErrors.length ? "warning" : "success"} onDismiss={() => { op.setMessage(""); setImportErrors([]); }} />}
      {importErrors.length > 0 && <details className="workspace-disclosure"><summary>{t("查看")} {importErrors.length} {t("项导入失败")}</summary><ul className="workspace-disclosure__body">{importErrors.map((error, index) => <li key={index}>{translateMessage(error)}</li>)}</ul></details>}
      <div className="mcp-directory-toolbar"><Input aria-label={t("搜索 MCP")} leading={<Search size={16} />} value={search} placeholder={t("名称、命令或 URL")} onChange={event => setSearch(event.target.value)} />
        <Select aria-label={t("传输方式")} value={transport} onChange={event => setTransport(event.target.value)}><option value="all">{t("全部传输方式")}</option><option value="stdio">{t("本地进程")}</option><option value="http">HTTP / SSE</option></Select>
        <Button variant="quiet" size="sm" icon={<RefreshCw size={14} />} loading={servers.isFetching} disabled={op.busy} onClick={() => void refresh()}>{t("刷新")}</Button></div>
      <section className="mcp-inventory" aria-label={t("MCP 服务器列表")}>
        <div className="mcp-inventory-heading"><h2>{t("服务清单")}</h2><span>{visible.length} / {list.length}</span></div>
        <p className="mcp-directory-note" id="mcp-mount-note">{t("开关控制配置挂载，不代表服务连接健康。")}</p>
        {servers.isPending ? <LoadingState label={t("读取 MCP 服务器…")} /> : servers.isError && !list.length ? null : !visible.length ? <EmptyState title={list.length ? t("没有匹配的 MCP") : t("尚未配置 MCP")} description={list.length ? t("更换传输方式或清除搜索条件。") : t("从现有应用配置导入，或添加本地进程与 HTTP 服务。")} action={<Button variant="secondary" size="sm" onClick={() => list.length ? (setSearch(""), setTransport("all")) : setEditor({ server: null })}>{list.length ? t("清除筛选") : t("添加 MCP")}</Button>} /> :
          <McpMountMatrix servers={visible} apps={apps.data ?? []} appsPending={apps.isPending} appsFailed={apps.isError} busy={op.busy} onInspect={server => { op.setError(""); isSharedMcpServerId(server.id) ? setArea("shared") : setSelectedId(server.id); }} onToggle={toggleMount} />}
      </section>
    </section> : <div id="mcp-areas-panel-shared" role="tabpanel" aria-labelledby="mcp-areas-tab-shared"><SharedMcpPanel /></div>}
    <Modal className="mcp-dialog" open={importOpen} title={t("从应用导入 MCP")} busy={op.busy} onClose={() => setImportOpen(false)} footer={<><Button variant="secondary" disabled={op.busy} onClick={() => setImportOpen(false)}>{t("取消")}</Button><Button loading={op.busy} onClick={() => void importFromApps()}>{t("读取并导入")}</Button></>}>
      <p>{t("读取现有应用中的 MCP 连接并加入列表。导入后可逐项查看配置和应用挂载。")}</p>
      <div className="mcp-import-apps">{applications.map(app => <span key={app.id}>{app.label} <Badge>{t(apps.isPending ? "正在加载…" : apps.error ? "读取失败" : apps.data?.find(item => item.app === app.id)?.installed ? "已检测" : "未检测到")}</Badge></span>)}</div>
      <InlineError message={op.error} />
    </Modal>
    {selected && !editor && !pendingDelete && area === "connections" && <Modal className="mcp-dialog" open size="lg" title={t("连接详情")} busy={op.busy} onClose={() => setSelectedId(null)}>
      <InlineError message={op.error} />
      <McpConnectionDetails server={selected} apps={apps.data ?? []} appsPending={apps.isPending} appsFailed={apps.isError} busy={op.busy} onEdit={() => setEditor({ server: selected })} onDelete={() => { op.setError(""); setPendingDelete(selected); }} onManageShared={() => { setSelectedId(null); setArea("shared"); }} onToggle={(app, enabled) => toggleMount(selected, app, enabled)} />
    </Modal>}
    {editor && <McpEditor server={editor.server} existing={list} onClose={() => setEditor(null)} onSaved={refresh} />}
    {pendingDelete && <ConfirmActionDialog title={t("删除 MCP")} message={t("删除“") + pendingDelete.name + t("”及其应用挂载配置。此操作无法撤销。")} busy={op.busy} error={op.error} onCancel={() => { setPendingDelete(null); op.setError(""); }} onConfirm={() => void remove()} />}
  </div>;
}

export function McpConnectionDetails({ server, apps, appsPending = false, appsFailed = false, busy, onEdit, onDelete, onToggle, onManageShared }: {
  server: McpServer; apps: McpAppStatus[]; busy: boolean; onEdit: () => void; onDelete: () => void;
  appsPending?: boolean; appsFailed?: boolean;
  onToggle: (app: McpApp, enabled: boolean) => void; onManageShared: () => void;
}) {
  useLanguage();
  const shared = isSharedMcpServerId(server.id);
  const managedTitle = shared ? t("共享插件连接请在共享插件服务中管理。") : undefined;
  return <div className="mcp-connection-details">
    <div className="record-row__title"><h3>{server.name}</h3><Badge>{server.transport === "stdio" ? t("本地进程") : "HTTP / SSE"}</Badge>{server.builtIn && <Badge>{t("内置")}</Badge>}{server.secretsConfigured && <Badge>{t("凭据已配置")}</Badge>}</div>
    <p className="muted">{server.description || t("未填写说明")}</p>
    <dl className="key-values"><dt>ID</dt><dd><code>{server.id}</code></dd><dt>{t(server.transport === "stdio" ? "启动命令" : "服务 URL")}</dt><dd><code>{server.transport === "stdio" ? [server.command, ...server.args].join(" ") : server.url}</code></dd></dl>
    {shared && <div className="mcp-review-notice"><p>{managedTitle}</p><Button variant="secondary" disabled={busy} onClick={onManageShared}>{t("管理共享连接")}</Button></div>}
    <section className="mcp-connection-details" aria-label={t("应用挂载")}><h3>{t("应用挂载")}</h3><p className="mcp-directory-note">{t("开关控制配置挂载，不代表服务连接健康。")}</p>
      <div className="mcp-apps">{applications.map(app => {
        const status = apps.find(item => item.app === app.id);
        return <div className="mcp-app" key={app.id}><span><span className="mcp-app-identity">{app.label}<small>{t(appsPending ? "正在加载…" : appsFailed ? "读取失败" : status?.installed ? "已检测" : "未检测到")}</small></span>{status?.configPath && <code className="mcp-app-path">{status.configPath}</code>}</span>
          <Toggle aria-label={server.name + " · " + app.label + t("挂载")} title={managedTitle} checked={server.apps[app.id]} disabled={busy || shared || appsPending || appsFailed || !status?.installed} onCheckedChange={enabled => { if (!shared && status?.installed) onToggle(app.id, enabled); }} /></div>;
      })}</div>
    </section>
    <div className="mcp-detail-actions"><Button variant="secondary" icon={<Pencil size={14} />} disabled={busy || shared} title={managedTitle} onClick={() => { if (!shared) onEdit(); }}>{t("编辑配置")}</Button>
      <Button variant="danger" icon={<Trash2 size={14} />} disabled={busy || shared || server.builtIn} title={managedTitle ?? (server.builtIn ? t("内置服务器不能删除") : t("删除服务器"))} onClick={() => { if (!shared && !server.builtIn) onDelete(); }}>{t("删除服务器")}</Button></div>
  </div>;
}

export function McpEditor({ server, existing, onClose, onSaved }: { server: McpServer | null; existing: McpServer[]; onClose: () => void; onSaved: () => Promise<void> }) {
  useLanguage();
  const [form, setForm] = useState<McpServer>(() => server ? structuredClone(server) : emptyServer());
  const [args, setArgs] = useState(() => JSON.stringify(form.args, null, 2));
  const [env, setEnv] = useState(() => formatKeyValueLines(form.env));
  const [headers, setHeaders] = useState(() => formatKeyValueLines(form.headers));
  const op = usePageOperation();
  const focus = useRef<HTMLInputElement>(null);
  const patch = (change: Partial<McpServer>) => setForm(value => ({ ...value, ...change }));
  const save = async () => {
    if (op.busy) return;
    let next: McpServer;
    try {
      const parsed: unknown = form.transport === "stdio" ? JSON.parse(args || "[]") : [];
      if (!Array.isArray(parsed) || parsed.some(value => typeof value !== "string")) throw new Error(t("启动参数必须是 JSON 字符串数组，每个参数单独一项。"));
      next = { ...server, ...form, id: form.id.trim(), name: form.name.trim(), description: form.description.trim(), command: form.transport === "stdio" ? (form.command ?? "").trim() : null, args: parsed, url: form.transport === "http" ? (form.url ?? "").trim() : null, env: parseKeyValueLines(env), headers: parseKeyValueLines(headers), builtIn: server?.builtIn ?? false, secretsConfigured: server?.secretsConfigured ?? false };
      const error = validateMcpServer(next); if (error) throw new Error(error);
      if (!server && existing.some(value => value.id === next.id)) throw new Error(t("MCP ID 已存在"));
    } catch (cause) { op.setError(errorText(cause)); return; }
    if (await op.run(async () => { await saveMcpServer(next); await onSaved(); })) onClose();
  };
  return <Modal className="mcp-dialog" open title={server ? t("编辑 MCP") : t("添加 MCP")} description={t("参数使用 JSON 数组，保留包含空格的完整参数。凭据配置与内置属性不会被覆盖丢失。")} size="lg" busy={op.busy} initialFocusRef={focus} onClose={onClose} footer={<><Button variant="secondary" disabled={op.busy} onClick={onClose}>{t("取消")}</Button><Button loading={op.busy} onClick={() => void save()}>{t("保存配置")}</Button></>}><form className="form-stack" onSubmit={event => { event.preventDefault(); void save(); }}><InlineError message={op.error} />{!server && <Field label={t("快速预设")}><Select value="" disabled={op.busy} onChange={event => { const preset = presets.find(item => item.name === event.target.value); if (preset) { patch({ name: preset.name, description: t(preset.description), transport: "stdio", command: "npx" }); setArgs(JSON.stringify(preset.args, null, 2)); } }}><option value="">{t("手动配置")}</option>{presets.map(item => <option key={item.name}>{item.name}</option>)}</Select></Field>}
    <div className="form-grid"><Field label={t("名称")} required><Input ref={focus} value={form.name} disabled={op.busy} onChange={event => patch({ name: event.target.value })} /></Field><Field label="ID" required description={t("保存后固定")}><Input className="mono" value={form.id} disabled={op.busy || Boolean(server)} onChange={event => patch({ id: event.target.value })} /></Field><Field className="span-two" label={t("说明")}><Input value={form.description} disabled={op.busy} onChange={event => patch({ description: event.target.value })} /></Field><Field className="span-two" label={t("传输方式")}><Select value={form.transport} disabled={op.busy} onChange={event => patch({ transport: event.target.value as McpTransport })}><option value="stdio">{t("stdio · 本地进程")}</option><option value="http">{t("HTTP / SSE · 远程服务")}</option></Select></Field></div>
    {form.transport === "stdio" ? <><Field label={t("启动命令")} required><Input className="mono" value={form.command ?? ""} disabled={op.busy} onChange={event => patch({ command: event.target.value })} placeholder="npx / uvx / node" /></Field><Field label={t("启动参数（JSON 数组）")} description={t("例如 [\"-y\", \"包名\", \"C:\\\\My Folder\"]")}><Textarea className="mono" value={args} disabled={op.busy} onChange={event => setArgs(event.target.value)} /></Field></> : <Field label={t("服务 URL")} required><Input className="mono" value={form.url ?? ""} disabled={op.busy} onChange={event => patch({ url: event.target.value })} placeholder="https://example.com/mcp" /></Field>}
    <details className="workspace-disclosure"><summary>{t("环境变量与 HTTP Headers")}</summary><div className="workspace-disclosure__body"><Field label={t("环境变量")} description={t("每行 KEY=value；已有字段完整保留。")}><Textarea className="mono" value={env} disabled={op.busy} onChange={event => setEnv(event.target.value)} /></Field><Field label="HTTP Headers" description={t("每行 Header=value。")}><Textarea className="mono" value={headers} disabled={op.busy} onChange={event => setHeaders(event.target.value)} /></Field></div></details>
    <fieldset className="form-stack mcp-editor-mounts"><legend>{t("应用挂载")} · {applications.filter(app => form.apps[app.id]).length} / {applications.length}</legend><p className="mcp-directory-note">{t("保存会替换完整挂载范围；未选中的客户端将移除这项挂载。")}</p>{applications.map(app => <div className="form-switch" key={app.id}><span>{app.label}</span><Toggle aria-label={app.label + t("应用挂载")} checked={form.apps[app.id]} disabled={op.busy} onCheckedChange={enabled => patch({ apps: { ...form.apps, [app.id]: enabled } })} /></div>)}</fieldset><button className="sr-only" type="submit" tabIndex={-1} disabled={op.busy}>{t("保存配置")}</button>
  </form></Modal>;
}
