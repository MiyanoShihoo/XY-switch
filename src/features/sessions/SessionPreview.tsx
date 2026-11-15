import { useEffect, useId, useMemo, useRef, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { ArrowLeft, ArrowRight, BookOpen, ChevronLeft, ChevronRight, Star } from "lucide-react";
import { t } from "../../i18n/language";
import { errorText } from "../../lib/error";
import { getSessionDetail } from "../../lib/session-service";
import { formatSessionTime, sessionTitle } from "../../lib/session-format";
import type { SessionMessage } from "../../types/session";
import { Button, EmptyState, InlineError, LoadingState, Toast } from "../../ui";
import { SessionResume, SessionSource } from "./SessionContext";
import { sessionDiscussion } from "./SessionDiscussion";
import { SessionReadWarnings } from "./SessionReadWarnings";
import type { SessionReaderProps } from "./SessionReader";

export function sessionPreviewMessages(messages: SessionMessage[]) {
  const questions = sessionDiscussion(messages);
  const start = questions[questions.length - 1]?.index ?? Math.max(0, messages.length - 2);
  const round = messages.slice(start).map((message, offset) => ({ message, index: start + offset }));
  const replies = round.filter(item => item.message.role === "assistant" && !item.message.tool);
  const excerpts = questions.length
    ? [round[0], replies[replies.length - 1]].filter((item): item is NonNullable<typeof item> => Boolean(item))
    : round;
  return { start, excerpts, otherCount: round.length - excerpts.length, discussionCount: questions.length };
}

interface SessionPreviewProps extends SessionReaderProps {
  position: number;
  total: number;
  focusOnOpen: boolean;
  onPrevious?: () => void;
  onNext?: () => void;
  onRead: (index: number) => void;
}

export function SessionPreview({ session, favorite, busy, error, message, favoriteError, favoriteUnsaved, onRetryFavorite, onFavorite, onCopyDirectory, onCopyResume, onDelete, onDismissMessage, onClose, onRead, position, total, onPrevious, onNext, focusOnOpen }: SessionPreviewProps) {
  const paneRef = useRef<HTMLElement>(null);
  const headingRef = useRef<HTMLHeadingElement>(null);
  const headingId = useId();
  const title = sessionTitle(session);
  const [titleExpanded, setTitleExpanded] = useState(false);
  const [titleOverflow, setTitleOverflow] = useState(false);
  const detail = useQuery({ queryKey: ["sessions", "messages", session.providerId, session.sessionId], queryFn: () => getSessionDetail(session.providerId, session.sessionId) });
  const preview = useMemo(() => sessionPreviewMessages(detail.isError ? [] : detail.data?.messages ?? []), [detail.data, detail.isError]);
  useEffect(() => {
    if (paneRef.current) paneRef.current.scrollTop = 0;
    if (focusOnOpen && (paneRef.current?.closest(".session-lookup")?.clientWidth ?? 0) < 680) headingRef.current?.focus({ preventScroll: true });
  }, [session.providerId, session.sessionId, focusOnOpen]);
  useEffect(() => {
    setTitleExpanded(false);
    const heading = headingRef.current;
    if (!heading) return;
    const measureTitle = () => {
      const lineHeight = Number.parseFloat(getComputedStyle(heading).lineHeight);
      setTitleOverflow(heading.scrollHeight > Math.ceil(lineHeight * 3) + 1);
    };
    measureTitle();
    const observer = typeof ResizeObserver === "undefined" ? undefined : new ResizeObserver(measureTitle);
    observer?.observe(heading);
    return () => observer?.disconnect();
  }, [session.providerId, session.sessionId, title]);

  return <section className="session-preview" aria-label={t("会话预览")} ref={paneRef}>
    <div className="session-preview__navigation">
      <Button variant="quiet" size="sm" icon={<ArrowLeft size={15} />} disabled={busy} onClick={onClose}>{t("返回结果")}</Button>
      <span>{position + 1} / {total}</span>
      <Button variant="quiet" size="sm" aria-label={t("上一条会话")} title={t("上一条会话")} disabled={busy || !onPrevious} onClick={onPrevious} icon={<ChevronLeft size={16} />} />
      <Button variant="quiet" size="sm" aria-label={t("下一条会话")} title={t("下一条会话")} disabled={busy || !onNext} onClick={onNext} icon={<ChevronRight size={16} />} />
    </div>
    <header className="session-preview__heading">
      <div className="session-preview__meta"><strong>{session.providerId}</strong><time>{formatSessionTime(session.lastActiveAt ?? session.createdAt)}</time>
        <Button className="session-favorite" variant="quiet" size="sm" aria-pressed={favorite} disabled={busy} onClick={onFavorite}
          aria-label={t(favorite ? "取消收藏会话 {title}" : "收藏会话 {title}", { title: sessionTitle(session) })}
          title={t(favorite ? "取消收藏会话 {title}" : "收藏会话 {title}", { title: sessionTitle(session) })} icon={<Star size={16} fill={favorite ? "currentColor" : "none"} />} />
      </div>
      <h2 id={headingId} ref={headingRef} tabIndex={-1} data-expanded={titleExpanded}>{title}</h2>
      {titleOverflow && <Button className="session-preview__title-toggle" variant="quiet" size="sm" aria-expanded={titleExpanded} aria-controls={headingId} onClick={() => setTitleExpanded(value => !value)}>{t(titleExpanded ? "收起标题" : "显示完整标题")}</Button>}
      {session.summary && session.summary.trim() !== sessionTitle(session) && <p className="session-preview__summary">{session.summary}</p>}
    </header>
    <InlineError message={error} />
    <InlineError message={favoriteError} onRetry={onRetryFavorite} />
    {favoriteUnsaved && <p className="session-reader__unsaved" role="status">{t("收藏变更尚未保存")}</p>}
    {message && <Toast title={message} tone="success" onDismiss={onDismissMessage} />}
    <div className="session-preview__context">
      <div className="session-preview__section-heading"><h3>{t("最近一轮 · 摘录")}</h3>{detail.data && !detail.isError && <span>{t("{count} 条消息", { count: detail.data.messages.length })}</span>}</div>
      <InlineError message={detail.error ? t("消息读取失败：") + errorText(detail.error) : ""} onRetry={() => void detail.refetch()} retrying={detail.isFetching} />
      <SessionReadWarnings warnings={detail.data?.warnings} />
      {detail.isPending ? <LoadingState label={t("读取会话消息…")} /> : detail.isError ? null : !preview.excerpts.length
        ? <EmptyState title={t("没有可读取的消息")} description={t("源会话可能仅保留元数据，或客户端未记录消息正文。")} />
        : <div className="session-preview__excerpts">{preview.excerpts.map(({ message: excerpt, index }) => <article className="session-excerpt" data-role={excerpt.role} key={index}>
          <h4>{excerpt.role === "user" ? t("你的提问") : excerpt.role === "assistant" ? t("最新回复") : excerpt.role}</h4>
          <p>{excerpt.content}</p>
        </article>)}
        {preview.otherCount > 0 && <p className="session-preview__omitted">{t("{count} 条其他记录可在全文中查看", { count: preview.otherCount })}</p>}</div>}
      <div className="session-preview__read-actions">
        <Button variant="primary" size="sm" icon={<ArrowRight size={15} />} iconPosition="end" disabled={busy || detail.isError || !preview.excerpts.length} onClick={() => onRead(preview.start)}>{t("从此处阅读")}</Button>
        <Button variant="quiet" size="sm" icon={<BookOpen size={15} />} disabled={busy} onClick={() => onRead(0)}>{t("阅读全文")}</Button>
      </div>
    </div>
    <SessionResume session={session} busy={busy} onCopyDirectory={onCopyDirectory} onCopyResume={onCopyResume} />
    <SessionSource session={session} busy={busy} onDelete={onDelete} />
  </section>;
}
