import { useLanguage, t } from "../../i18n/language";
import { useEffect, useMemo, useRef, useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { Copy, RefreshCw, Search, Star, Trash2, X } from "lucide-react";
import { getSessionCatalog, getSessionDetail, deleteSessions } from "../../lib/session-service";
import { errorText } from "../../lib/error";
import { formatSessionTime, sessionKey, sessionTitle } from "../../lib/session-format";
import { filterSessions } from "../../lib/session-filter";
import { useSessionFavorites } from "./useSessionFavorites";
import type { SessionMeta } from "../../types/session";
import { ConfirmActionDialog } from "../../components/ConfirmActionDialog";
import { Badge, Button, Drawer, EmptyState, IconButton, InlineError, Input, LoadingState, Select, Toast } from "../../ui";
import { useCompact } from "../shared/useCompact";
import { usePageOperation } from "../shared/usePageOperation";
import "./sessions.css";

const EMPTY_SESSIONS: SessionMeta[] = [];

function ReadWarnings({ warnings }: { warnings?: string[] }) {
  if (!warnings?.length) return null;
  return <div className="session-read-warnings" role="status">
    <strong>{t("部分会话内容未能读取")}</strong>
    <ul>{warnings.map((warning, index) => <li key={index}>{warning}</li>)}</ul>
  </div>;
}

export function SessionsPage() {
  useLanguage();
  const client = useQueryClient();
  const op = usePageOperation();
  const compact = useCompact();
  const favorites = useSessionFavorites();
  const searchInput = useRef<HTMLInputElement>(null);
  const favoritesFilterButton = useRef<HTMLButtonElement>(null);
  const focusAfterRemoval = useRef(false);
  const [favoritesOnly, setFavoritesOnly] = useState(false);
  const [search, setSearch] = useState("");
  const [app, setApp] = useState("all");
  const [selectedKey, setSelectedKey] = useState("");
  const [drawerOpen, setDrawerOpen] = useState(false);
  const [pendingDelete, setPendingDelete] = useState<SessionMeta | null>(null);
  const query = useQuery({ queryKey: ["sessions", "list"], queryFn: getSessionCatalog });
  const list = query.data?.sessions ?? EMPTY_SESSIONS;
  const applications = useMemo(() => [...new Set(list.map(session => session.providerId))].sort(), [list]);
  const favoriteCount = useMemo(() => list.filter(session => favorites.keys.has(sessionKey(session))).length, [list, favorites.keys]);
  const visible = useMemo(() => filterSessions(list, { search, app, favoritesOnly, favoriteKeys: favorites.keys }), [list, app, search, favoritesOnly, favorites.keys]);
  const filtersActive = Boolean(search || app !== "all" || favoritesOnly);
  const clearFilters = () => { setSearch(""); setApp("all"); setFavoritesOnly(false); searchInput.current?.focus(); };
  const selected = visible.find(session => sessionKey(session) === selectedKey) ?? visible[0];
  useEffect(() => { if (!compact) setDrawerOpen(false); }, [compact]);
  useEffect(() => {
    const nextKey = selected ? sessionKey(selected) : "";
    if (selectedKey !== nextKey) {
      // Anchor the initial/fallback selection before an external favorite update reorders it.
      if (drawerOpen) focusAfterRemoval.current = true;
      setSelectedKey(nextKey);
      setDrawerOpen(false);
    }
  }, [selected, selectedKey, drawerOpen]);
  useEffect(() => {
    // Wait until the drawer releases its focus trap; its original row may no longer exist.
    if (!drawerOpen && focusAfterRemoval.current) {
      focusAfterRemoval.current = false;
      favoritesFilterButton.current?.focus();
    }
  });
  useEffect(() => {
    if (query.isSuccess && app !== "all" && !applications.includes(app)) setApp("all");
  }, [query.isSuccess, app, applications]);
  const messages = useQuery({
    queryKey: ["sessions", "messages", selected?.providerId, selected?.sessionId],
    queryFn: () => getSessionDetail(selected!.providerId, selected!.sessionId),
    enabled: Boolean(selected) && (!compact || drawerOpen),
  });
  const select = (session: SessionMeta) => { setSelectedKey(sessionKey(session)); if (compact) setDrawerOpen(true); };
  const refresh = async () => { await client.invalidateQueries({ queryKey: ["sessions"] }); };
  const toggleFavorite = (session: SessionMeta) => {
    // Reordering a bookmarked row must not change the conversation being inspected.
    if (selected) setSelectedKey(sessionKey(selected));
    if (favoritesOnly && favorites.keys.has(sessionKey(session))) {
      focusAfterRemoval.current = true;
      setDrawerOpen(false);
    }
    favorites.toggle(session);
  };
  const remove = async () => {
    if (!pendingDelete) return;
    const target = pendingDelete;
    if (await op.run(async () => {
      try {
        const removed = await deleteSessions([{ providerId: target.providerId, sessionId: target.sessionId }]);
        if (removed !== 1) throw new Error(t("后端未删除任何会话，记录仍然保留。"));
      } finally {
        // A batch may fail after removing some files. Never leave stale rows on
        // screen or mistake an error/zero count for a successful deletion.
        await refresh();
      }
    }, t("会话已删除"))) {
      favorites.remove(target);
      setPendingDelete(null);
      setDrawerOpen(false);
      if (selectedKey === sessionKey(target)) setSelectedKey("");
    }
  };

  const detail = selected ? <section className="session-detail" aria-label={t("会话详情")}>
    <header className="session-detail__header">
      <div className="section-heading"><h2>{sessionTitle(selected)}</h2><Badge>{selected.providerId}</Badge></div>
      <dl className="key-values">
        <dt>{t("会话 ID")}</dt><dd className="mono">{selected.sessionId}</dd>
        <dt>{t("工作目录")}</dt><dd className="mono">{selected.projectDir || t("未记录")}</dd>
        <dt>{t("最近活动")}</dt><dd>{formatSessionTime(selected.lastActiveAt ?? selected.createdAt)}</dd>
      </dl>
      {selected.sourcePath && <details className="session-source"><summary>{t("源文件")}</summary><code className="mono">{selected.sourcePath}</code></details>}
      <div className="inline-actions">
        <Button variant="secondary" size="sm" aria-pressed={favorites.keys.has(sessionKey(selected))}
          icon={<Star size={14} fill={favorites.keys.has(sessionKey(selected)) ? "currentColor" : "none"} />}
          disabled={op.busy} title={favorites.keys.has(sessionKey(selected)) ? t("取消收藏") : t("收藏会话")}
          onClick={() => toggleFavorite(selected)}>{favorites.keys.has(sessionKey(selected)) ? t("已收藏") : t("收藏会话")}</Button>
        <Button variant="secondary" size="sm" icon={<Copy size={14} />} disabled={op.busy || !selected.resumeCommand}
          title={!selected.resumeCommand ? t("此客户端未提供恢复命令") : selected.resumeCommand}
          onClick={() => selected.resumeCommand && void op.run(() => navigator.clipboard.writeText(selected.resumeCommand!), t("恢复命令已复制"))}>{t("复制恢复命令")}</Button>
        <Button variant="danger" size="sm" icon={<Trash2 size={14} />} disabled={op.busy}
          onClick={() => { op.setError(""); setPendingDelete(selected); }}>{t("删除会话")}</Button>
      </div>
    </header>
    <div className="session-messages">
      <InlineError message={messages.error ? t("消息读取失败：") + errorText(messages.error) : ""} onRetry={() => void messages.refetch()} retrying={messages.isFetching} />
      <ReadWarnings warnings={messages.data?.warnings} />
      {messages.isPending ? <LoadingState label={t("读取会话消息…")} /> : messages.isError ? null : !messages.data?.messages.length
        ? <EmptyState title={t("没有可读取的消息")} description={t("源会话可能仅保留元数据，或客户端未记录消息正文。")} />
        : messages.data.messages.map((message, index) => <article className="session-message" data-role={message.role} key={index}>
          <div className="session-message__meta"><strong>{message.role}</strong><time>{formatSessionTime(message.ts)}</time></div>
          <pre>{message.content}</pre>
        </article>)}
    </div>
  </section> : <EmptyState title={t("选择一个会话")} description={t("查看消息、工作目录与恢复命令。")} />;

  return <div className="workspace-page sessions-page">
    <header className="page-heading">
      <div className="page-heading__copy"><h1>{t("会话")}</h1><p className="page-description">{t("按客户端和工作目录查找记录。恢复命令只复制，不执行。")}</p></div>
      <Button variant="secondary" size="sm" icon={<RefreshCw size={14} />} loading={query.isFetching} disabled={op.busy} onClick={() => void refresh()}>{t("刷新")}</Button>
    </header>
    <p className="page-description">{t("支持 Codex、Claude Code 和 Gemini CLI 的本地会话日志；恢复前请切换到记录的工作目录。")}</p>
    <InlineError message={op.error || (query.error ? t("会话读取失败：") + errorText(query.error) : "")} onRetry={() => void refresh()} retrying={query.isFetching} />
    <ReadWarnings warnings={query.data?.warnings} />
    <InlineError message={favorites.issue === "invalid"
      ? t("收藏记录格式无法识别，未覆盖原记录；当前收藏变更暂存于本页。")
      : favorites.issue ? t("本地收藏暂不可用；当前收藏变更暂存于本页，可重试保存。") : ""}
      onRetry={favorites.retry} />
    {op.message && <Toast title={op.message} tone="success" onDismiss={() => op.setMessage("")} />}
    <div className="toolbar session-toolbar"><div className="inline-actions">
      <Select aria-label={t("筛选会话客户端")} value={app} onChange={event => setApp(event.target.value)}>
        <option value="all">{t("所有客户端")}</option>{applications.map(value => <option key={value}>{value}</option>)}
      </Select>
      <Input ref={searchInput} className="toolbar__search" aria-label={t("搜索会话")} leading={<Search size={14} />} value={search}
        onChange={event => setSearch(event.target.value)} placeholder={t("标题、会话 ID 或工作目录")} title={t("按 Esc 清除搜索")}
        onKeyDown={event => { if (event.key === "Escape" && !event.nativeEvent.isComposing && search) { event.preventDefault(); event.stopPropagation(); setSearch(""); } }} />
      <Button ref={favoritesFilterButton} className="session-favorites-filter" variant="secondary" size="sm" aria-label={t("仅看收藏")} aria-pressed={favoritesOnly}
        icon={<Star size={14} fill={favoritesOnly ? "currentColor" : "none"} />} onClick={() => setFavoritesOnly(value => !value)}>
        {t("仅看收藏")} <span aria-hidden="true">({favoriteCount})</span>
      </Button>
      {filtersActive && <Button variant="quiet" size="sm" icon={<X size={14} />} onClick={clearFilters}>{t("清除筛选")}</Button>}
    </div><span className="page-description session-results-summary" role="status" aria-live="polite">{visible.length} / {list.length} {t("条 · 收藏优先，最近活动排序")}</span></div>
    {query.isPending ? <LoadingState label={t("读取本地会话索引…")} /> : query.isError && !list.length ? null : !visible.length
      ? <EmptyState title={list.length ? favoritesOnly && !favoriteCount ? t("还没有收藏的会话") : t("没有匹配的会话") : t("尚未发现本地会话")}
        description={list.length ? favoritesOnly && !favoriteCount ? t("点击会话旁的星标，将常用会话收藏到本机。") : t("清除搜索、客户端或收藏筛选。") : t("在客户端产生会话后刷新索引。")}
        action={<Button variant="secondary" size="sm" onClick={() => list.length ? clearFilters() : void refresh()}>{list.length ? favoritesOnly && !favoriteCount ? t("查看全部会话") : t("清除筛选") : t("刷新索引")}</Button>} />
      : <div className="session-workspace"><div className="session-index" role="list" aria-label={t("本地会话列表")}>
        {visible.map(session => <div className="session-index__row" role="listitem" key={sessionKey(session)}
          data-selected={sessionKey(session) === (selected && sessionKey(selected))}>
          <button type="button" className="session-index__item"
            data-selected={sessionKey(session) === (selected && sessionKey(selected))}
            aria-current={sessionKey(session) === (selected && sessionKey(selected)) ? "true" : undefined}
            onClick={() => select(session)} aria-label={t("查看会话 ") + sessionTitle(session)}>
            <div className="section-heading"><strong>{sessionTitle(session)}</strong><Badge>{session.providerId}</Badge></div>
            <span className="record-row__detail mono">{session.projectDir || session.sessionId}</span>
            <small className="muted">{formatSessionTime(session.lastActiveAt ?? session.createdAt)}</small>
          </button>
          <IconButton className="session-favorite" aria-pressed={favorites.keys.has(sessionKey(session))} disabled={op.busy}
            aria-label={t(favorites.keys.has(sessionKey(session)) ? "取消收藏会话 {title}" : "收藏会话 {title}", { title: sessionTitle(session) })}
            title={favorites.keys.has(sessionKey(session)) ? t("取消收藏") : t("收藏会话")}
            onClick={() => toggleFavorite(session)}>
            <Star size={16} aria-hidden="true" fill={favorites.keys.has(sessionKey(session)) ? "currentColor" : "none"} />
          </IconButton>
        </div>)}
      </div>{!compact && detail}</div>}
    <Drawer open={compact && drawerOpen} title={t("会话详情")} busy={op.busy} onClose={() => setDrawerOpen(false)}>{detail}</Drawer>
    {pendingDelete && <ConfirmActionDialog title={t("删除会话")}
      message={t("删除“") + sessionTitle(pendingDelete) + t("”的本地会话日志。此操作不可撤销；请先停止该会话。不会修改客户端的索引或数据库，原客户端可能仍显示旧索引。")}
      busy={op.busy} error={op.error} onCancel={() => { setPendingDelete(null); op.setError(""); }} onConfirm={() => void remove()} />}
  </div>;
}
