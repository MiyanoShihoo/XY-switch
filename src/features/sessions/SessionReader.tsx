import { useEffect, useId, useMemo, useRef, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { ArrowLeft, ChevronDown, ChevronUp, List, Maximize2, Search, Star, Terminal, X } from "lucide-react";
import { t, useLanguage } from "../../i18n/language";
import { getSessionDetail } from "../../lib/session-service";
import { errorText } from "../../lib/error";
import { formatSessionTime, sessionTitle } from "../../lib/session-format";
import type { SessionMeta } from "../../types/session";
import { Button, EmptyState, InlineError, Input, LoadingState, Modal, Toast } from "../../ui";
import { SessionTranscript } from "./SessionTranscript";
import { SessionReadWarnings } from "./SessionReadWarnings";
import { SessionDiscussion, sessionDiscussion } from "./SessionDiscussion";
import { SessionResume, SessionSource } from "./SessionContext";

export interface SessionReaderProps {
  session: SessionMeta;
  favorite: boolean;
  busy: boolean;
  error?: string;
  message?: string;
  favoriteError?: string;
  favoriteUnsaved?: boolean;
  onRetryFavorite?: () => void;
  onClose: () => void;
  onFavorite: () => void;
  onCopyResume: () => void;
  onCopyDirectory?: () => void;
  onDelete: () => void;
  onDismissMessage: () => void;
  onExpand?: (index: number) => void;
  expanded?: boolean;
  initialMessageIndex?: number;
}

export function SessionReader({ session, favorite, busy, error = "", message = "", favoriteError = "", favoriteUnsaved = false, onRetryFavorite, onClose, onFavorite, onCopyResume, onCopyDirectory, onDelete, onDismissMessage, onExpand, expanded = false, initialMessageIndex = 0 }: SessionReaderProps) {
  useLanguage();
  const [find, setFind] = useState("");
  const [cursor, setCursor] = useState(0);
  const [contextPanel, setContextPanel] = useState<"resume" | "discussion" | null>(null);
  const [activeDiscussion, setActiveDiscussion] = useState<number>();
  const transcriptRef = useRef<HTMLDivElement>(null);
  const titleRef = useRef<HTMLHeadingElement>(null);
  const initialJumpDone = useRef(false);
  const sideId = useId();
  const messages = useQuery({ queryKey: ["sessions", "messages", session.providerId, session.sessionId], queryFn: () => getSessionDetail(session.providerId, session.sessionId) });
  const needle = find.trim();
  const matches = useMemo(() => !needle || messages.isError ? [] : (messages.data?.messages ?? []).flatMap((message, index) => message.content.toLocaleLowerCase().includes(needle.toLocaleLowerCase()) ? [index] : []), [needle, messages.data, messages.isError]);
  const discussions = useMemo(() => sessionDiscussion(messages.isError ? [] : messages.data?.messages ?? []), [messages.data, messages.isError]);
  const position = matches.length ? cursor % matches.length : 0;
  const activeIndex = matches[position];
  const move = (direction: number) => setCursor((position + direction + matches.length) % (matches.length || 1));
  useEffect(() => {
    setFind(""); setCursor(0); setContextPanel(null); setActiveDiscussion(undefined);
    initialJumpDone.current = false;
    if (!expanded) titleRef.current?.focus({ preventScroll: true });
  }, [session.providerId, session.sessionId, expanded, initialMessageIndex]);
  useEffect(() => {
    if (initialJumpDone.current || !messages.data || messages.isError) return;
    const frame = requestAnimationFrame(() => {
      initialJumpDone.current = true;
      const index = Math.min(initialMessageIndex, Math.max(0, messages.data.messages.length - 1));
      if (index > 0) {
        const target = transcriptRef.current?.querySelector<HTMLElement>('[data-message-index="' + index + '"]');
        target?.focus({ preventScroll: true });
        target?.scrollIntoView({ block: "start" });
        setActiveDiscussion([...discussions].reverse().find(item => item.index <= index)?.index);
      } else if (transcriptRef.current) transcriptRef.current.scrollTop = 0;
    });
    return () => cancelAnimationFrame(frame);
  }, [messages.data, messages.isError, initialMessageIndex, discussions]);
  useEffect(() => {
    if (activeIndex === undefined) return;
    setActiveDiscussion([...discussions].reverse().find(item => item.index <= activeIndex)?.index);
    transcriptRef.current?.querySelector<HTMLElement>('[data-message-index="' + activeIndex + '"]')?.scrollIntoView({ block: "nearest" });
  }, [activeIndex, needle, discussions]);
  const jumpToDiscussion = (index: number) => {
    setActiveDiscussion(index);
    setContextPanel(null);
    requestAnimationFrame(() => {
      const target = transcriptRef.current?.querySelector<HTMLElement>('[data-message-index="' + index + '"]');
      target?.focus({ preventScroll: true }); target?.scrollIntoView({ block: "start" });
    });
  };
  const expand = () => {
    const top = transcriptRef.current?.getBoundingClientRect().top ?? 0;
    const first = [...transcriptRef.current?.querySelectorAll<HTMLElement>("[data-message-index]") ?? []].find(item => item.getBoundingClientRect().bottom > top + 16);
    onExpand?.(Number(first?.dataset.messageIndex ?? 0));
  };

  return <section className="session-detail session-reader" aria-label={t("会话详情")}>
    <header className="session-reader__heading">
      {!expanded && <Button className="session-reader__back" variant="quiet" size="sm" icon={<ArrowLeft size={15} />} aria-label={t("返回会话目录")} title={t("返回会话目录")} disabled={busy} onClick={onClose}><span className="session-reader__control-label">{t("返回会话目录")}</span></Button>}
      <div className="session-reader__identity">{!expanded && <h2 ref={titleRef} tabIndex={-1} title={sessionTitle(session)}>{sessionTitle(session)}</h2>}<span>{session.providerId} · {formatSessionTime(session.lastActiveAt ?? session.createdAt)}</span></div>
      <Button className="session-favorite" variant="quiet" size="sm" aria-pressed={favorite}
        aria-label={t(favorite ? "取消收藏会话 {title}" : "收藏会话 {title}", { title: sessionTitle(session) })}
        title={t(favorite ? "取消收藏会话 {title}" : "收藏会话 {title}", { title: sessionTitle(session) })}
        icon={<Star size={16} fill={favorite ? "currentColor" : "none"} />} disabled={busy} onClick={onFavorite} />
      {onExpand && !expanded && <Button className="session-reader__expand" variant="quiet" size="sm" icon={<Maximize2 size={15} />} title={t("放大阅读")} disabled={busy} onClick={expand}><span className="session-reader__control-label">{t("放大阅读")}</span></Button>}
    </header>
    <InlineError message={error} /><InlineError message={favoriteError} onRetry={onRetryFavorite} />
    {favoriteUnsaved && <span className="session-reader__unsaved" role="status">{t("收藏变更尚未保存")}</span>}
    {message && <Toast title={message} tone="success" onDismiss={onDismissMessage} />}
    <div className="session-reader__toolbar">
      <div className="session-find" role="search" aria-label={t("查找会话正文")}>
        <Input aria-label={t("查找会话正文")} leading={<Search size={15} />} value={find} placeholder={t("输入关键词定位消息")}
          onChange={event => { setFind(event.target.value); setCursor(0); }}
          onKeyDown={event => {
            if (event.nativeEvent.isComposing) return;
            if (event.key === "Enter") { event.preventDefault(); move(event.shiftKey ? -1 : 1); }
            if (event.key === "Escape" && find) { event.preventDefault(); event.stopPropagation(); setFind(""); setCursor(0); }
          }} />
        <span className="session-find__count" role="status">{needle ? t("{current} / {total} 条匹配消息", { current: matches.length ? position + 1 : 0, total: matches.length }) : messages.data && !messages.isError ? t("{count} 条消息", { count: messages.data.messages.length }) : "—"}</span>
        {find && <div className="session-find__actions">
          <Button variant="quiet" size="sm" aria-label={t("上一条匹配消息")} title={t("上一条匹配消息")} disabled={!matches.length} onClick={() => move(-1)} icon={<ChevronUp size={16} />} />
          <Button variant="quiet" size="sm" aria-label={t("下一条匹配消息")} title={t("下一条匹配消息")} disabled={!matches.length} onClick={() => move(1)} icon={<ChevronDown size={16} />} />
          <Button variant="quiet" size="sm" aria-label={t("清除正文查找")} onClick={() => { setFind(""); setCursor(0); }} icon={<X size={15} />} />
        </div>}
      </div>
      <div className="session-reader__panel-toggles">
        <Button variant="quiet" size="sm" icon={<List size={15} />} aria-expanded={contextPanel === "discussion"} aria-controls={sideId} onClick={() => setContextPanel(value => value === "discussion" ? null : "discussion")}>{t("本次讨论")} <span>{discussions.length}</span></Button>
        <Button variant="secondary" size="sm" icon={<Terminal size={15} />} aria-expanded={contextPanel === "resume"} aria-controls={sideId} onClick={() => setContextPanel(value => value === "resume" ? null : "resume")}>{t("继续工作")}</Button>
      </div>
    </div>
    <div className="session-reader__workspace" data-panel={contextPanel ?? "closed"}>
      <aside className="session-reader__side" id={sideId} aria-label={t("阅读上下文")}>
        <div className="session-reader__resume-panel"><SessionResume session={session} busy={busy} onCopyDirectory={onCopyDirectory} onCopyResume={onCopyResume} /></div>
        <SessionDiscussion items={discussions} activeIndex={activeDiscussion ?? discussions[0]?.index} onJump={jumpToDiscussion} />
        <div className="session-reader__source-panel"><SessionSource session={session} busy={busy} onDelete={onDelete} /></div>
      </aside>
      <div className="session-messages" ref={transcriptRef} tabIndex={0} role="region" aria-label={t("完整会话正文")}>
        <InlineError message={messages.error ? t("消息读取失败：") + errorText(messages.error) : ""} onRetry={() => void messages.refetch()} retrying={messages.isFetching} />
        <SessionReadWarnings warnings={messages.data?.warnings} />
        {messages.isPending ? <LoadingState label={t("读取会话消息…")} /> : messages.isError ? null : !messages.data?.messages.length
          ? <EmptyState title={t("没有可读取的消息")} description={t("源会话可能仅保留元数据，或客户端未记录消息正文。")} />
          : <SessionTranscript messages={messages.data.messages} search={needle} activeIndex={activeIndex} />}
      </div>
    </div>
  </section>;
}

export function SessionDetailPanel(props: SessionReaderProps) {
  useLanguage();
  return <Modal open size="lg" className="session-reader-modal" title={sessionTitle(props.session)}
    busy={props.busy} onClose={props.onClose}>
    <SessionReader {...props} expanded />
  </Modal>;
}
