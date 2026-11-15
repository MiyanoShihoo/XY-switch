import { useLanguage, t } from "../../i18n/language";
import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { RefreshCw, Search, Star, X } from "lucide-react";
import { getSessionCatalog, deleteSessions } from "../../lib/session-service";
import { errorText } from "../../lib/error";
import { sessionKey, sessionTitle } from "../../lib/session-format";
import { filterSessions } from "../../lib/session-filter";
import { useSessionFavorites } from "./useSessionFavorites";
import { SessionReader, SessionDetailPanel } from "./SessionReader";
import { SessionPreview } from "./SessionPreview";
import { SessionReadWarnings } from "./SessionReadWarnings";
import { SessionDirectory, groupSessionsByProject, sessionProjectLabels } from "./SessionDirectory";
export { SessionDetailPanel } from "./SessionReader";
import type { SessionMeta } from "../../types/session";
import { ConfirmActionDialog } from "../../components/ConfirmActionDialog";
import { Button, EmptyState, Field, InlineError, Input, LoadingState, Select, Toast } from "../../ui";
import { usePageOperation } from "../shared/usePageOperation";
import "./sessions.css";

const EMPTY_SESSIONS: SessionMeta[] = [];

export function SessionsPage() {
  const language = useLanguage();
  const client = useQueryClient();
  const op = usePageOperation();
  const native = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
  const favorites = useSessionFavorites();
  const searchInput = useRef<HTMLInputElement>(null);
  const directoryRef = useRef<HTMLDivElement>(null);
  const readingTrigger = useRef<HTMLElement | null>(null);
  const directoryScroll = useRef(0);
  const readingPaneRef = useRef<HTMLDivElement>(null);
  const reopenAfterDelete = useRef(false);
  const [favoritesOnly, setFavoritesOnly] = useState(false);
  const [search, setSearch] = useState("");
  const [app, setApp] = useState("all");
  const [project, setProject] = useState("all");
  const [selectedKey, setSelectedKey] = useState("");
  const [previewOpen, setPreviewOpen] = useState(false);
  const [detailOpen, setDetailOpen] = useState(false);
  const [reading, setReading] = useState(false);
  const [readFrom, setReadFrom] = useState(0);
  const [expandedFrom, setExpandedFrom] = useState(0);
  const [focusDirectory, setFocusDirectory] = useState<false | "result" | "read-action">(false);
  const [focusReader, setFocusReader] = useState(false);
  const [pendingDelete, setPendingDelete] = useState<SessionMeta | null>(null);
  const query = useQuery({ queryKey: ["sessions", "list"], queryFn: getSessionCatalog });
  const list = query.data?.sessions ?? EMPTY_SESSIONS;
  const applications = useMemo(() => [...new Set(list.map(session => session.providerId))].sort(), [list]);
  const projects = useMemo(() => groupSessionsByProject(list), [list]);
  const projectLabels = useMemo(() => sessionProjectLabels(list), [list, language]);
  const favoriteCount = useMemo(() => list.filter(session => favorites.keys.has(sessionKey(session))).length, [list, favorites.keys]);
  const visible = useMemo(() => filterSessions(list, { search, app, favoritesOnly, favoriteKeys: favorites.keys })
    .filter(session => project === "all" || JSON.stringify(session.projectDir || "") === project), [list, app, project, search, favoritesOnly, favorites.keys]);
  const filtersActive = Boolean(search || app !== "all" || project !== "all" || favoritesOnly);
  const favoriteError = favorites.issue === "invalid"
    ? t("收藏记录格式无法识别，未覆盖原记录；当前收藏变更暂存于本页。")
    : favorites.issue ? t("本地收藏暂不可用；当前收藏变更暂存于本页，可重试保存。") : "";
  const selected = visible.find(session => sessionKey(session) === selectedKey) ?? visible[0];
  const activeKey = selected ? sessionKey(selected) : "";
  const position = selected ? visible.indexOf(selected) : -1;
  const clearFilters = () => { setSearch(""); setApp("all"); setProject("all"); setFavoritesOnly(false); searchInput.current?.focus(); };

  useEffect(() => {
    if (selectedKey === activeKey) return;
    if (selectedKey) {
      setDetailOpen(false);
      setReading(false);
      setPreviewOpen(false);
      if (reading || detailOpen || document.activeElement === document.body) setFocusDirectory("result");
    }
    setSelectedKey(activeKey);
  }, [selectedKey, activeKey, reading, detailOpen]);
  useLayoutEffect(() => {
    if (!focusDirectory || reading || detailOpen || pendingDelete) return;
    setFocusDirectory(false);
    if (directoryRef.current) directoryRef.current.scrollTop = directoryScroll.current;
    const preferred = focusDirectory === "read-action" ? readingTrigger.current : null;
    const target = preferred?.isConnected && preferred.getClientRects().length ? preferred
      : directoryRef.current?.querySelector<HTMLButtonElement>('[aria-current="true"]') ?? searchInput.current;
    target?.focus({ preventScroll: true });
    if (focusDirectory === "result") target?.scrollIntoView({ block: "nearest" });
  }, [focusDirectory, reading, detailOpen, pendingDelete]);
  useEffect(() => {
    if (!focusReader || detailOpen || pendingDelete) return;
    setFocusReader(false);
    readingPaneRef.current?.querySelector<HTMLButtonElement>(".session-reader__expand")?.focus();
  }, [focusReader, detailOpen, pendingDelete]);
  useEffect(() => {
    if (query.isSuccess && app !== "all" && !applications.includes(app)) setApp("all");
    if (query.isSuccess && project !== "all" && !projects.some(item => JSON.stringify(item.path) === project)) setProject("all");
  }, [query.isSuccess, app, applications, project, projects]);

  const refresh = async () => { await client.invalidateQueries({ queryKey: ["sessions"] }); };
  const selectSession = (session: SessionMeta) => {
    if (op.busy) return;
    directoryScroll.current = directoryRef.current?.scrollTop ?? 0;
    setSelectedKey(sessionKey(session));
    setPreviewOpen(true);
    op.setError("");
    op.setMessage("");
  };
  const openReader = (index: number) => {
    directoryScroll.current = directoryRef.current?.scrollTop ?? 0;
    readingTrigger.current = document.activeElement as HTMLElement | null;
    setReadFrom(index);
    setReading(true);
  };
  const toggleFavorite = (session: SessionMeta) => {
    if (favoritesOnly && favorites.keys.has(sessionKey(session))) setDetailOpen(false);
    favorites.toggle(session);
  };
  const remove = async () => {
    if (!pendingDelete || op.busy) return;
    const target = pendingDelete;
    if (await op.run(async () => {
      try {
        const removed = await deleteSessions([{ providerId: target.providerId, sessionId: target.sessionId }]);
        if (removed !== 1) throw new Error(t("后端未删除任何会话，记录仍然保留。"));
      } finally { await refresh(); }
    }, t("会话已删除"))) {
      favorites.remove(target);
      setPendingDelete(null);
      setReading(false);
      setPreviewOpen(false);
      setDetailOpen(false);
      setSelectedKey("");
      setFocusDirectory("result");
    }
  };
  const readerProps = selected ? {
    session: selected, favorite: favorites.keys.has(activeKey), busy: op.busy,
    error: pendingDelete ? "" : op.error, message: op.message, favoriteError, favoriteUnsaved: favorites.unsaved,
    onRetryFavorite: favorites.retry, onFavorite: () => toggleFavorite(selected), onDismissMessage: () => op.setMessage(""),
    onCopyResume: () => { if (selected.resumeCommand) void op.run(() => navigator.clipboard.writeText(selected.resumeCommand!), t("恢复命令已复制")); },
    onCopyDirectory: () => { if (selected.projectDir) void op.run(() => navigator.clipboard.writeText(selected.projectDir!), t("工作目录已复制")); },
    onDelete: () => { op.setError(""); reopenAfterDelete.current = detailOpen; setPendingDelete(selected); setDetailOpen(false); },
    onClose: () => { setReading(false); setFocusDirectory("read-action"); },
  } : null;

  return <div className="workspace-page sessions-page" data-reading={reading && Boolean(selected)}>
    <InlineError message={query.error ? t("会话读取失败：") + errorText(query.error) : ""} onRetry={() => void refresh()} retrying={query.isFetching} />
    <SessionReadWarnings warnings={query.data?.warnings} />
    {!selected && <><InlineError message={favoriteError} onRetry={favorites.retry} /><InlineError message={pendingDelete ? "" : op.error} />{op.message && <Toast title={op.message} tone="success" onDismiss={() => op.setMessage("")} />}</>}
    <div className="session-browser" hidden={reading && Boolean(selected)}>
      <header className="session-browser__heading">
        <h1>{t("会话")}</h1>
        <Input ref={searchInput} aria-label={t("搜索会话")} leading={<Search size={17} />} value={search} disabled={op.busy}
          onChange={event => setSearch(event.target.value)} placeholder={t("搜索标题、摘要、路径或 ID")} title={t("按 Esc 清除搜索")}
          onKeyDown={event => { if (event.key === "Escape" && !event.nativeEvent.isComposing && search) { event.preventDefault(); event.stopPropagation(); setSearch(""); } }} />
        <Button variant="quiet" size="sm" icon={<RefreshCw size={16} />} title={t("刷新索引")} loading={query.isFetching} disabled={op.busy} onClick={() => void refresh()}><span className="session-refresh-label">{t("刷新索引")}</span></Button>
      </header>
      <div className="session-toolbar">
        <div className="session-browse" role="group" aria-label={t("浏览会话")}>
          <Button variant="quiet" size="sm" aria-pressed={!favoritesOnly} disabled={op.busy} onClick={() => setFavoritesOnly(false)}>{t("全部会话")}</Button>
          <Button className="session-favorites-filter" variant="quiet" size="sm" aria-label={t("仅看收藏")} aria-pressed={favoritesOnly} disabled={op.busy}
            icon={<Star size={14} fill={favoritesOnly ? "currentColor" : "none"} />} onClick={() => setFavoritesOnly(value => !value)}>{t("收藏")} <span aria-hidden="true">{favoriteCount}</span></Button>
        </div>
        <Field className="session-toolbar__project" label={t("筛选工作目录")}><Select value={project} disabled={op.busy} onChange={event => setProject(event.target.value)}>
          <option value="all">{t("所有项目")}</option>{projects.map(item => <option key={item.path} value={JSON.stringify(item.path)}>{item.path || t("未记录工作目录")} ({item.entries.length})</option>)}
        </Select></Field>
        <Field className="session-toolbar__client" label={t("筛选会话客户端")}><Select value={app} disabled={op.busy} onChange={event => setApp(event.target.value)}>
          <option value="all">{t("所有客户端")}</option>{applications.map(value => <option key={value}>{value}</option>)}
        </Select></Field>
        {filtersActive && <Button variant="quiet" size="sm" icon={<X size={15} />} disabled={op.busy} onClick={clearFilters} aria-label={t("清除筛选")} title={t("清除筛选")} />}
      </div>
      {!native && <p className="session-browser__notice">{t("浏览器预览不读取本地会话；请在桌面端查看历史。")}</p>}
      {query.isPending ? <LoadingState label={t("读取本地会话索引…")} /> : query.isError && !list.length ? null : !visible.length
        ? <EmptyState title={list.length ? favoritesOnly && !favoriteCount ? t("还没有收藏的会话") : t("没有匹配的会话") : t("尚未发现本地会话")}
          description={list.length ? favoritesOnly && !favoriteCount ? t("打开会话后，可将它收藏到本机。") : t("清除搜索、项目、客户端或收藏筛选。") : native ? t("在客户端产生会话后刷新索引。") : t("浏览器预览不读取本地会话；请在桌面端查看历史。")}
          action={list.length || native ? <Button variant="secondary" onClick={() => list.length ? clearFilters() : void refresh()}>{list.length ? t("清除筛选") : t("刷新索引")}</Button> : undefined} />
        : <div className="session-lookup" data-preview-open={previewOpen}>
          <section className="session-results session-directory" aria-label={t("本地会话列表")}>
            <header className="session-results__heading"><h2>{filtersActive ? t("搜索结果") : t("最近会话")}</h2><span role="status" aria-live="polite" aria-atomic="true"><span className="sr-only">{t("{visible} / {total} 条会话", { visible: visible.length, total: list.length })}</span><span aria-hidden="true">{visible.length} / {list.length}</span></span></header>
            <div className="session-library" ref={directoryRef}><SessionDirectory sessions={visible} projectLabels={projectLabels} selectedKey={activeKey} favoriteKeys={favorites.keys} busy={op.busy} onSelect={selectSession} /></div>
            <p className="session-library-note">{t("收藏优先 · 最近活动")}</p>
          </section>
          {readerProps && <SessionPreview key={activeKey} {...readerProps} position={position} total={visible.length} focusOnOpen={previewOpen} onRead={openReader}
            onPrevious={position > 0 ? () => selectSession(visible[position - 1]) : undefined} onNext={position + 1 < visible.length ? () => selectSession(visible[position + 1]) : undefined}
            onClose={() => { setPreviewOpen(false); setFocusDirectory("result"); }} />}
        </div>}
    </div>
    {reading && readerProps && <div className="session-reading-pane" ref={readingPaneRef} hidden={detailOpen}>
      <SessionReader key={activeKey} {...readerProps} initialMessageIndex={readFrom} onExpand={index => { setExpandedFrom(index); setDetailOpen(true); }} />
    </div>}
    {detailOpen && readerProps && !pendingDelete && <SessionDetailPanel key={activeKey} {...readerProps} initialMessageIndex={expandedFrom} onClose={() => { setDetailOpen(false); setFocusReader(true); }} />}
    {pendingDelete && <ConfirmActionDialog title={t("删除会话")}
      message={t("删除“") + sessionTitle(pendingDelete) + t("”的本地会话日志。此操作不可撤销；请先停止该会话。不会修改客户端的索引或数据库，原客户端可能仍显示旧索引。")}
      busy={op.busy} error={op.error} onCancel={() => { setPendingDelete(null); op.setError(""); setDetailOpen(reopenAfterDelete.current && Boolean(selected)); }} onConfirm={() => void remove()} />}
  </div>;
}
