import { useEffect, useMemo, useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { Copy, RefreshCw, Search, Trash2 } from "lucide-react";
import { listSessions, getSessionMessages, deleteSessions } from "../../lib/session-service";
import { errorText } from "../../lib/error";
import { formatSessionTime, sessionKey, sessionTitle, timestampMs } from "../../lib/session-format";
import type { SessionMeta } from "../../types/session";
import { ConfirmActionDialog } from "../../components/ConfirmActionDialog";
import { Badge, Button, Drawer, EmptyState, InlineError, Input, LoadingState, Select, Toast } from "../../ui";
import { useCompact } from "../shared/useCompact";
import { usePageOperation } from "../shared/usePageOperation";
import "./sessions.css";

export function SessionsPage() {
  const client = useQueryClient(); const op = usePageOperation(); const compact = useCompact();
  const [search, setSearch] = useState(""); const [app, setApp] = useState("all");
  const [selectedKey, setSelectedKey] = useState(""); const [drawerOpen, setDrawerOpen] = useState(false);
  const [pendingDelete, setPendingDelete] = useState<SessionMeta | null>(null);
  const query = useQuery({ queryKey: ["sessions", "list"], queryFn: listSessions });
  const list = query.data ?? [];
  const applications = [...new Set(list.map(session => session.providerId))].sort();
  const visible = useMemo(() => list.filter(session => (app === "all" || session.providerId === app) && [session.title, session.summary, session.sessionId, session.projectDir].some(value => (value ?? "").toLowerCase().includes(search.trim().toLowerCase()))).sort((left, right) => timestampMs(right.lastActiveAt || right.createdAt) - timestampMs(left.lastActiveAt || left.createdAt)), [list, app, search]);
  const selected = visible.find(session => sessionKey(session) === selectedKey) ?? visible[0];
  useEffect(() => { if (!compact || !selected) setDrawerOpen(false); }, [compact, selected]);
  const messages = useQuery({ queryKey: ["sessions", "messages", selected?.providerId, selected?.sessionId], queryFn: () => getSessionMessages(selected!.providerId, selected!.sessionId), enabled: Boolean(selected) && (!compact || drawerOpen) });
  const select = (session: SessionMeta) => { setSelectedKey(sessionKey(session)); if (compact) setDrawerOpen(true); };
  const refresh = async () => { await client.invalidateQueries({ queryKey: ["sessions"] }); };
  const remove = async () => {
    if (!pendingDelete) return;
    const target = pendingDelete;
    if (await op.run(async () => {
      const removed = await deleteSessions([{ providerId: target.providerId, sessionId: target.sessionId }]);
      if (removed < 1) throw new Error("后端未删除任何会话，记录仍然保留。");
      await refresh();
    }, "会话已删除")) { setPendingDelete(null); setDrawerOpen(false); }
  };
  const detail = selected ? <section className="session-detail" aria-label="会话详情"><header className="session-detail__header"><div className="section-heading"><h2>{sessionTitle(selected)}</h2><Badge>{selected.providerId}</Badge></div><dl className="key-values"><dt>会话 ID</dt><dd className="mono">{selected.sessionId}</dd><dt>工作目录</dt><dd className="mono">{selected.projectDir || "未记录"}</dd><dt>最近活动</dt><dd>{formatSessionTime(selected.lastActiveAt || selected.createdAt)}</dd></dl>{selected.sourcePath && <details className="session-source"><summary>源文件</summary><code className="mono">{selected.sourcePath}</code></details>}<div className="inline-actions"><Button variant="secondary" size="sm" icon={<Copy size={14} />} disabled={op.busy || !selected.resumeCommand} title={!selected.resumeCommand ? "此客户端未提供恢复命令" : selected.resumeCommand} onClick={() => selected.resumeCommand && void op.run(() => navigator.clipboard.writeText(selected.resumeCommand!), "恢复命令已复制")}>复制恢复命令</Button><Button variant="danger" size="sm" icon={<Trash2 size={14} />} disabled={op.busy} onClick={() => { op.setError(""); setPendingDelete(selected); }}>删除会话</Button></div></header><div className="session-messages"><InlineError message={messages.error ? "消息读取失败：" + errorText(messages.error) : ""} onRetry={() => void messages.refetch()} retrying={messages.isFetching} />{messages.isPending ? <LoadingState label="读取会话消息…" /> : !messages.data?.length ? <EmptyState title="没有可读取的消息" description="源会话可能仅保留元数据，或客户端未记录消息正文。" /> : messages.data.map((message, index) => <article className="session-message" data-role={message.role} key={index}><div className="session-message__meta"><strong>{message.role}</strong><time>{formatSessionTime(message.ts)}</time></div><pre>{message.content}</pre></article>)}</div></section> : <EmptyState title="选择一个会话" description="查看消息、工作目录与恢复命令。" />;
  return <div className="workspace-page sessions-page"><header className="page-heading"><div className="page-heading__copy"><h1>会话</h1><p className="page-description">按客户端和工作目录查找记录。恢复命令只复制，不执行。</p></div><Button variant="secondary" size="sm" icon={<RefreshCw size={14} />} loading={query.isFetching} disabled={op.busy} onClick={() => void refresh()}>刷新</Button></header><InlineError message={op.error || (query.error ? "会话读取失败：" + errorText(query.error) : "")} onRetry={() => void refresh()} retrying={query.isFetching} />{op.message && <Toast title={op.message} tone="success" onDismiss={() => op.setMessage("")} />}<div className="toolbar"><div className="inline-actions"><Select aria-label="筛选会话客户端" value={app} onChange={event => setApp(event.target.value)}><option value="all">所有客户端</option>{applications.map(value => <option key={value}>{value}</option>)}</Select><Input className="toolbar__search" aria-label="搜索会话" leading={<Search size={14} />} value={search} onChange={event => setSearch(event.target.value)} placeholder="标题、会话 ID 或工作目录" /></div><span className="page-description">{visible.length} / {list.length} 条 · 最近活动优先</span></div>
    {query.isPending ? <LoadingState label="读取本地会话索引…" /> : !visible.length ? <EmptyState title={list.length ? "没有匹配的会话" : "尚未发现本地会话"} description={list.length ? "清除搜索与客户端筛选。" : "在客户端产生会话后刷新索引。"} action={<Button variant="secondary" size="sm" onClick={() => list.length ? (setSearch(""), setApp("all")) : void refresh()}>{list.length ? "清除筛选" : "刷新索引"}</Button>} /> : <div className="session-workspace"><div className="session-index" role="list" aria-label="本地会话列表">{visible.map(session => <div role="listitem" key={sessionKey(session)}><button className="session-index__item" data-selected={sessionKey(session) === (selected && sessionKey(selected))} onClick={() => select(session)} aria-label={"查看会话 " + sessionTitle(session)}><div className="section-heading"><strong>{sessionTitle(session)}</strong><Badge>{session.providerId}</Badge></div><span className="record-row__detail mono">{session.projectDir || session.sessionId}</span><small className="muted">{formatSessionTime(session.lastActiveAt || session.createdAt)}</small></button></div>)}</div>{!compact && detail}</div>}
    <Drawer open={compact && drawerOpen} title="会话详情" busy={op.busy} onClose={() => setDrawerOpen(false)}>{detail}</Drawer>
    {pendingDelete && <ConfirmActionDialog title="删除会话" message={"删除“" + sessionTitle(pendingDelete) + "”的本地记录。后端不支持删除时将显示原始错误并保留记录。"} busy={op.busy} error={op.error} onCancel={() => { setPendingDelete(null); op.setError(""); }} onConfirm={() => void remove()} />}
  </div>;
}
