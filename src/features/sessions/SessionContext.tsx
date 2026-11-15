import { Copy, Terminal, Trash2 } from "lucide-react";
import { t } from "../../i18n/language";
import { formatSessionTime, sessionTitle } from "../../lib/session-format";
import type { SessionMeta } from "../../types/session";
import { Button } from "../../ui";

interface SessionContextProps {
  session: SessionMeta;
  busy: boolean;
  onCopyDirectory?: () => void;
  onCopyResume: () => void;
}

export function SessionResume({ session, busy, onCopyDirectory, onCopyResume }: SessionContextProps) {
  return <section className="session-resume-context" aria-label={t("项目上下文")}>
    <h3><Terminal size={16} aria-hidden="true" />{t("继续工作")}</h3>
    <ol className="session-resume-steps">
      <li><div className="session-resume-steps__label"><span aria-hidden="true">1</span><strong>{t("先切换工作目录")}</strong></div>
        <code className="session-resume-context__path" tabIndex={0}>{session.projectDir || t("未记录工作目录")}</code>
        {session.projectDir ? <Button variant="quiet" size="sm" icon={<Copy size={13} />} disabled={busy || !onCopyDirectory} onClick={onCopyDirectory}>{t("复制工作目录")}</Button>
          : <p>{t("未记录工作目录；恢复前请先确认原工作目录。")}</p>}
      </li>
      <li><div className="session-resume-steps__label"><span aria-hidden="true">2</span><strong>{t("再在终端使用恢复命令")}</strong></div>
        {session.resumeCommand && <code className="session-resume-context__command" tabIndex={0}>{session.resumeCommand}</code>}
        <Button className="session-reader__resume" variant="secondary" size="sm" icon={<Copy size={14} />} disabled={busy || !session.resumeCommand}
          title={!session.resumeCommand ? t("此客户端未提供恢复命令") : session.resumeCommand} onClick={onCopyResume}>{t("复制恢复命令")}</Button>
        {!session.resumeCommand && <p>{t("此客户端未提供恢复命令")}</p>}
      </li>
    </ol>
    <p className="session-resume-context__note">{t("恢复命令只复制，不执行。")}</p>
  </section>;
}

export function SessionSource({ session, busy, onDelete }: { session: SessionMeta; busy: boolean; onDelete: () => void }) {
  return <details className="workspace-disclosure session-source">
    <summary>{t("会话信息与管理")}</summary>
    <div className="workspace-disclosure__body">
      <dl className="session-detail__facts">
        <div><dt>{t("会话")}</dt><dd>{sessionTitle(session)}</dd></div>
        <div><dt>{t("客户端")}</dt><dd>{session.providerId}</dd></div>
        <div><dt>{t("工作目录")}</dt><dd><code tabIndex={0}>{session.projectDir || t("未记录工作目录")}</code></dd></div>
        <div><dt>{t("会话 ID")}</dt><dd><code tabIndex={0}>{session.sessionId}</code></dd></div>
        <div><dt>{t("最近活动")}</dt><dd>{formatSessionTime(session.lastActiveAt ?? session.createdAt)}</dd></div>
        {session.sourcePath && <div><dt>{t("源文件")}</dt><dd><code tabIndex={0}>{session.sourcePath}</code></dd></div>}
        {session.resumeCommand && <div><dt>{t("恢复命令")}</dt><dd><code tabIndex={0}>{session.resumeCommand}</code></dd></div>}
      </dl>
      <p>{t("按客户端和工作目录查找记录。恢复命令只复制，不执行。")}</p>
      <Button variant="danger" size="sm" icon={<Trash2 size={14} />} disabled={busy} onClick={onDelete}>{t("删除会话")}</Button>
    </div>
  </details>;
}
