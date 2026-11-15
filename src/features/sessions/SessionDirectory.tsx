import { Star } from "lucide-react";
import { t } from "../../i18n/language";
import { formatSessionTime, sessionKey, sessionTitle } from "../../lib/session-format";
import type { SessionMeta } from "../../types/session";

export function groupSessionsByProject(sessions: SessionMeta[]) {
  const projects = new Map<string, SessionMeta[]>();
  for (const session of sessions) {
    const path = session.projectDir || "";
    const group = projects.get(path);
    if (group) group.push(session);
    else projects.set(path, [session]);
  }
  return [...projects].map(([path, entries]) => ({ path, entries }));
}

export function sessionProjectLabels(sessions: SessionMeta[]) {
  const paths = [...new Set(sessions.map(session => session.projectDir || ""))];
  const segments = new Map(paths.map(path => [path, path.replace(/[\\/]+$/, "").split(/[\\/]/)]));
  return new Map(paths.map(path => {
    if (!path) return [path, t("未记录工作目录")];
    const parts = segments.get(path)!;
    for (let count = Math.min(2, parts.length); count <= parts.length; count++) {
      const suffix = parts.slice(-count).join(" / ");
      if (!paths.some(other => other !== path && segments.get(other)!.slice(-count).join(" / ") === suffix)) return [path, suffix];
    }
    return [path, path];
  }));
}

interface SessionDirectoryProps {
  sessions: SessionMeta[];
  projectLabels: ReadonlyMap<string, string>;
  selectedKey: string;
  favoriteKeys: ReadonlySet<string>;
  busy: boolean;
  onSelect: (session: SessionMeta, trigger: HTMLButtonElement) => void;
}

export function SessionDirectory({ sessions, projectLabels, selectedKey, favoriteKeys, busy, onSelect }: SessionDirectoryProps) {
  return <div className="session-index" role="list">{sessions.map((session, position) => <div className="session-index__row" role="listitem" key={sessionKey(session)}>
    <button type="button" className="session-index__item" disabled={busy} tabIndex={selectedKey === sessionKey(session) ? 0 : -1}
      aria-current={selectedKey === sessionKey(session) ? "true" : undefined}
      aria-label={t("预览会话 {title}", { title: sessionTitle(session) })}
      onClick={event => onSelect(session, event.currentTarget)}
      onKeyDown={event => {
        if (event.altKey || event.ctrlKey || event.metaKey || !["ArrowDown", "ArrowUp", "Home", "End"].includes(event.key)) return;
        event.preventDefault();
        const next = event.key === "Home" ? 0 : event.key === "End" ? sessions.length - 1 : Math.max(0, Math.min(sessions.length - 1, position + (event.key === "ArrowDown" ? 1 : -1)));
        const target = event.currentTarget.closest(".session-index")?.querySelectorAll<HTMLButtonElement>(".session-index__item")[next];
        if (target) { target.focus({ preventScroll: true }); target.scrollIntoView({ block: "nearest" }); onSelect(sessions[next], target); }
      }}>
      <span className="session-index__title"><strong title={sessionTitle(session)}>{sessionTitle(session)}</strong>{favoriteKeys.has(sessionKey(session)) && <Star size={14} fill="currentColor" aria-label={t("已收藏")} />}</span>
      <span className="session-index__path" title={session.projectDir || t("未记录工作目录")}>{projectLabels.get(session.projectDir || "")}</span>
      <span className="session-index__meta"><span>{session.providerId}</span><time title={formatSessionTime(session.lastActiveAt ?? session.createdAt)}>{formatSessionTime(session.lastActiveAt ?? session.createdAt)}</time></span>
    </button>
  </div>)}</div>;
}
