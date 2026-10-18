import { useMemo, useRef, useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { Download, Pencil, Plus, RefreshCw, Search, Trash2 } from "lucide-react";
import { loadMcpServers, loadMcpApps, saveMcpServer, deleteMcpServer, setMcpAppEnabled, importMcpFromApps } from "../../lib/mcp-service";
import { formatKeyValueLines, parseKeyValueLines, validateMcpServer } from "../../lib/mcp-validation";
import { errorText } from "../../lib/error";
import type { McpApp, McpServer, McpTransport } from "../../types/mcp";
import { ConfirmActionDialog } from "../../components/ConfirmActionDialog";
import { Badge, Button, EmptyState, Field, IconButton, InlineError, Input, LoadingState, Modal, Select, Tabs, Textarea, Toast, Toggle } from "../../ui";
import { usePageOperation } from "../shared/usePageOperation";
import "./mcp.css";

const applications: Array<{ id: McpApp; label: string }> = [{ id: "claude", label: "Claude" }, { id: "codex", label: "Codex" }, { id: "gemini", label: "Gemini" }, { id: "opencode", label: "OpenCode" }];
const emptyServer = (): McpServer => ({ id: "mcp-" + crypto.randomUUID().slice(0, 8), name: "", description: "", transport: "stdio", command: "npx", args: [], url: null, env: {}, headers: {}, apps: { claude: true, codex: true, gemini: true, opencode: false }, builtIn: false, secretsConfigured: false });
const presets = [{ name: "Memory", description: "持久化知识图谱记忆", args: ["-y", "@modelcontextprotocol/server-memory"] }, { name: "Filesystem", description: "按指定目录开放文件系统访问", args: ["-y", "@modelcontextprotocol/server-filesystem", "C:\\Users"] }, { name: "Fetch", description: "网页与 API 抓取", args: ["-y", "@modelcontextprotocol/server-fetch"] }, { name: "GitHub", description: "仓库与 Issue 操作", args: ["-y", "@modelcontextprotocol/server-github"] }];

export function McpPage() {
  const client = useQueryClient();
  const servers = useQuery({ queryKey: ["mcp", "servers"], queryFn: loadMcpServers });
  const apps = useQuery({ queryKey: ["mcp", "apps"], queryFn: loadMcpApps });
  const op = usePageOperation();
  const [search, setSearch] = useState("");
  const [transport, setTransport] = useState("all");
  const [editor, setEditor] = useState<{ server: McpServer | null } | null>(null);
  const [pendingDelete, setPendingDelete] = useState<McpServer | null>(null);
  const [importErrors, setImportErrors] = useState<string[]>([]);
  const list = servers.data ?? [];
  const visible = useMemo(() => list.filter(server => (transport === "all" || server.transport === transport) && [server.name, server.description, server.command, server.url].some(value => (value ?? "").toLowerCase().includes(search.trim().toLowerCase()))), [list, search, transport]);
  const refresh = async () => { await client.invalidateQueries({ queryKey: ["mcp"] }); };
  const importFromApps = () => op.run(async () => { const result = await importMcpFromApps(); setImportErrors(result.errors); op.setMessage("已导入 " + result.imported + " 个 MCP 服务器" + (result.errors.length ? "，部分应用导入失败" : "")); await refresh(); });
  const remove = async () => { if (!pendingDelete) return; const target = pendingDelete; if (await op.run(async () => { await deleteMcpServer(target.id); await refresh(); }, "已删除 " + target.name)) setPendingDelete(null); };
  return <div className="workspace-page mcp-page">
    <header className="page-heading"><div className="page-heading__copy"><h1>MCP</h1><p className="page-description">逐项对齐启动方式和应用挂载，不把配置启用当作连接健康。</p></div><div className="page-actions"><Button variant="secondary" size="sm" icon={<Download size={14} />} loading={op.busy} onClick={() => void importFromApps()}>从应用导入</Button><Button size="sm" icon={<Plus size={14} />} disabled={op.busy} onClick={() => setEditor({ server: null })}>添加 MCP</Button></div></header>
    <InlineError message={op.error || (servers.error ? "服务器读取失败：" + errorText(servers.error) : apps.error ? "应用状态读取失败：" + errorText(apps.error) : "")} onRetry={() => void refresh()} retrying={servers.isFetching || apps.isFetching} />
    {op.message && <Toast title={op.message} tone={importErrors.length ? "warning" : "success"} onDismiss={() => { op.setMessage(""); setImportErrors([]); }} />}
    {importErrors.length > 0 && <details className="details-group"><summary>查看 {importErrors.length} 项导入失败</summary><ul className="details-group__body">{importErrors.map((error, i) => <li key={i}>{error}</li>)}</ul></details>}
    <div className="toolbar"><Tabs label="传输方式" value={transport} onChange={setTransport} items={[{ value: "all", label: "全部", count: list.length }, { value: "stdio", label: "本地进程" }, { value: "http", label: "HTTP / SSE" }]} /><div className="inline-actions"><Input className="toolbar__search" aria-label="搜索 MCP" leading={<Search size={14} />} value={search} placeholder="名称、命令或 URL" onChange={event => setSearch(event.target.value)} /><Button variant="quiet" size="sm" icon={<RefreshCw size={14} />} loading={servers.isFetching} disabled={op.busy} onClick={() => void refresh()}>刷新</Button></div></div>
    {servers.isPending ? <LoadingState label="读取 MCP 服务器…" /> : !visible.length ? <EmptyState title={list.length ? "没有匹配的 MCP" : "尚未配置 MCP"} description={list.length ? "更换传输方式或清除搜索条件。" : "从现有应用配置导入，或添加本地进程与 HTTP 服务。"} action={<Button variant="secondary" size="sm" onClick={() => list.length ? (setSearch(""), setTransport("all")) : setEditor({ server: null })}>{list.length ? "清除筛选" : "添加 MCP"}</Button>} /> : <div className="record-list" role="list" aria-label="MCP 服务器列表">{visible.map(server => <article className="mcp-record" key={server.id} role="listitem"><div className="record-row__main"><div className="record-row__title">{server.name}<Badge>{server.transport === "stdio" ? "本地进程" : "HTTP / SSE"}</Badge>{server.builtIn && <Badge>内置</Badge>}{server.secretsConfigured && <Badge>凭据已配置</Badge>}</div><p className="record-row__detail">{server.description || "未填写说明"}</p><code className="record-row__detail mono">{server.transport === "stdio" ? [server.command, ...server.args].join(" ") : server.url}</code></div><div className="mcp-apps" aria-label={server.name + "应用挂载"}>{applications.map(app => { const status = apps.data?.find(item => item.app === app.id); return <div className="mcp-app" key={app.id}><span title={status?.configPath}>{app.label}<small>{status?.installed ? "已检测" : "未检测到"}</small></span><Toggle aria-label={server.name + " · " + app.label + "挂载"} checked={server.apps[app.id]} disabled={op.busy || !status?.installed} onCheckedChange={enabled => void op.run(async () => { await setMcpAppEnabled(server.id, app.id, enabled); await refresh(); }, app.label + (enabled ? "已启用挂载" : "已停用挂载"))} /></div>; })}</div><div className="record-row__actions"><IconButton aria-label={"编辑 " + server.name} disabled={op.busy} onClick={() => setEditor({ server })}><Pencil size={15} /></IconButton><IconButton aria-label={"删除 " + server.name} disabled={op.busy || server.builtIn} title={server.builtIn ? "内置服务器不能删除" : "删除服务器"} onClick={() => { op.setError(""); setPendingDelete(server); }}><Trash2 size={15} /></IconButton></div></article>)}</div>}
    {editor && <McpEditor server={editor.server} existing={list} onClose={() => setEditor(null)} onSaved={refresh} />}
    {pendingDelete && <ConfirmActionDialog title="删除 MCP" message={"删除“" + pendingDelete.name + "”及其应用挂载配置。此操作无法撤销。"} busy={op.busy} error={op.error} onCancel={() => { setPendingDelete(null); op.setError(""); }} onConfirm={() => void remove()} />}
  </div>;
}

export function McpEditor({ server, existing, onClose, onSaved }: { server: McpServer | null; existing: McpServer[]; onClose: () => void; onSaved: () => Promise<void> }) {
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
      if (!Array.isArray(parsed) || parsed.some(value => typeof value !== "string")) throw new Error("启动参数必须是 JSON 字符串数组，每个参数单独一项。");
      next = { ...server, ...form, id: form.id.trim(), name: form.name.trim(), description: form.description.trim(), command: form.transport === "stdio" ? (form.command ?? "").trim() : null, args: parsed, url: form.transport === "http" ? (form.url ?? "").trim() : null, env: parseKeyValueLines(env), headers: parseKeyValueLines(headers), builtIn: server?.builtIn ?? false, secretsConfigured: server?.secretsConfigured ?? false };
      const error = validateMcpServer(next); if (error) throw new Error(error);
      if (!server && existing.some(value => value.id === next.id)) throw new Error("MCP ID 已存在");
    } catch (cause) { op.setError(errorText(cause)); return; }
    if (await op.run(async () => { await saveMcpServer(next); await onSaved(); })) onClose();
  };
  return <Modal open title={server ? "编辑 MCP" : "添加 MCP"} description="参数使用 JSON 数组，保留包含空格的完整参数。凭据配置与内置属性不会被覆盖丢失。" size="lg" busy={op.busy} initialFocusRef={focus} onClose={onClose} footer={<><Button variant="secondary" disabled={op.busy} onClick={onClose}>取消</Button><Button loading={op.busy} onClick={() => void save()}>保存配置</Button></>}><form className="form-stack" onSubmit={event => { event.preventDefault(); void save(); }}><InlineError message={op.error} />{!server && <Field label="快速预设"><Select value="" disabled={op.busy} onChange={event => { const preset = presets.find(item => item.name === event.target.value); if (preset) { patch({ name: preset.name, description: preset.description, transport: "stdio", command: "npx" }); setArgs(JSON.stringify(preset.args, null, 2)); } }}><option value="">手动配置</option>{presets.map(item => <option key={item.name}>{item.name}</option>)}</Select></Field>}
    <div className="form-grid"><Field label="名称" required><Input ref={focus} value={form.name} disabled={op.busy} onChange={event => patch({ name: event.target.value })} /></Field><Field label="ID" required description="保存后固定"><Input className="mono" value={form.id} disabled={op.busy || Boolean(server)} onChange={event => patch({ id: event.target.value })} /></Field><Field className="span-two" label="说明"><Input value={form.description} disabled={op.busy} onChange={event => patch({ description: event.target.value })} /></Field><Field className="span-two" label="传输方式"><Select value={form.transport} disabled={op.busy} onChange={event => patch({ transport: event.target.value as McpTransport })}><option value="stdio">stdio · 本地进程</option><option value="http">HTTP / SSE · 远程服务</option></Select></Field></div>
    {form.transport === "stdio" ? <><Field label="启动命令" required><Input className="mono" value={form.command ?? ""} disabled={op.busy} onChange={event => patch({ command: event.target.value })} placeholder="npx / uvx / node" /></Field><Field label="启动参数（JSON 数组）" description='例如 ["-y", "包名", "C:\\My Folder"]'><Textarea className="mono" value={args} disabled={op.busy} onChange={event => setArgs(event.target.value)} /></Field></> : <Field label="服务 URL" required><Input className="mono" value={form.url ?? ""} disabled={op.busy} onChange={event => patch({ url: event.target.value })} placeholder="https://example.com/mcp" /></Field>}
    <details className="details-group"><summary>环境变量与 HTTP Headers</summary><div className="details-group__body"><Field label="环境变量" description="每行 KEY=value；已有字段完整保留。"><Textarea className="mono" value={env} disabled={op.busy} onChange={event => setEnv(event.target.value)} /></Field><Field label="HTTP Headers" description="每行 Header=value。"><Textarea className="mono" value={headers} disabled={op.busy} onChange={event => setHeaders(event.target.value)} /></Field></div></details>
    <fieldset className="form-stack"><legend>应用挂载</legend>{applications.map(app => <div className="form-switch" key={app.id}><span>{app.label}</span><Toggle aria-label={app.label + "应用挂载"} checked={form.apps[app.id]} disabled={op.busy} onCheckedChange={enabled => patch({ apps: { ...form.apps, [app.id]: enabled } })} /></div>)}</fieldset><button className="sr-only" type="submit" tabIndex={-1} disabled={op.busy}>保存配置</button>
  </form></Modal>;
}
