import { useMemo, useState } from "react";
import { ArrowDown, History, RefreshCw } from "lucide-react";
import { Button } from "../../ui";
import type { AgentEvent, AgentSnapshot } from "../../types/agents";
import { calendarDate, count, isoTimestamp, parseDateInput, timestamp } from "./format";
import { eventKindText, useAgentText } from "./i18n";
import { EmptyObservation, StatusBadge } from "./presentation";
import { useEvents } from "./use-events";

export interface EventFilter { agentId: string; taskId: string; since: string; until: string }
export const initialEventFilter: EventFilter = { agentId: "", taskId: "", since: "", until: "" };
export function TimelineView({ snapshot, teamId, enabled, changeVersion, filter, onFilter, onSelect, selectedId }: {
  snapshot: AgentSnapshot | null; teamId: string; enabled: boolean; changeVersion: number; filter: EventFilter;
  onFilter: (filter: EventFilter) => void; onSelect: (event: AgentEvent) => void; selectedId: string | null;
}) {
  const { t, locale } = useAgentText();
  const [limit, setLimit] = useState(100);
  const since = parseDateInput(filter.since), requestedUntil = parseDateInput(filter.until);
  const until = snapshot?.replayAt != null ? Math.min(requestedUntil ?? Infinity, snapshot.replayAt) : requestedUntil;
  const invalidDate = (!!filter.since && since === undefined) || (!!filter.until && requestedUntil === undefined);
  const invalidRange = since != null && until != null && since > until;
  const events = useEvents({ teamId: teamId || undefined, agentId: filter.agentId || undefined, taskId: filter.taskId || undefined, since, until }, enabled && !invalidDate && !invalidRange, changeVersion, snapshot?.replayAt ?? null);
  const agents = useMemo(() => new Map(snapshot?.agents.map(agent => [agent.id, agent]) ?? []), [snapshot?.agents]);
  const tasks = useMemo(() => snapshot?.tasks.filter(task => !filter.agentId || task.agentId === filter.agentId) ?? [], [snapshot?.tasks, filter.agentId]);
  return <section className="ag-timeline"><div className="ag-view-heading"><div><h2>{t("时间线")}</h2><p>{count(events.total, locale)} {t("事件")} · {t("源时间戳")}</p></div><Button variant="quiet" size="sm" icon={<RefreshCw size={14} />} disabled={!enabled || invalidDate || invalidRange} loading={events.loading} onClick={events.refresh}>{t("刷新")}</Button></div>
    <div className="ag-event-filters"><label><span>{t("成员")}</span><select value={filter.agentId} onChange={event => onFilter({ ...filter, agentId: event.target.value, taskId: "" })}><option value="">{t("所有成员")}</option>{snapshot?.agents.map(agent => <option key={agent.id} value={agent.id}>{agent.name}</option>)}{filter.agentId && !agents.has(filter.agentId) && <option value={filter.agentId}>{filter.agentId}</option>}</select></label>
      <label><span>{t("任务")}</span><select value={filter.taskId} onChange={event => onFilter({ ...filter, taskId: event.target.value })}><option value="">{t("所有任务")}</option>{tasks.map(task => <option key={task.id} value={task.id}>{task.title}</option>)}{filter.taskId && !tasks.some(task => task.id === filter.taskId) && <option value={filter.taskId}>{filter.taskId}</option>}</select></label>
      <label><span>{t("起始时间")} · {t("本地时间")}</span><input type="datetime-local" step="1" value={filter.since} onChange={event => onFilter({ ...filter, since: event.target.value })} /></label>
      <label><span>{t("截止时间")} · {t("本地时间")}</span><input type="datetime-local" step="1" value={filter.until} onChange={event => onFilter({ ...filter, until: event.target.value })} /></label></div>
    {(filter.agentId || filter.taskId || filter.since || filter.until) && <Button variant="quiet" size="sm" onClick={() => onFilter(initialEventFilter)}>{t("清除筛选")}</Button>}
    {(invalidDate || invalidRange) && <p className="ag-inline-error" role="alert">{t(invalidDate ? "时间无效" : "起始时间不能晚于截止时间。")}</p>}
    {events.error && <div className="ag-notice" data-tone="danger" role="alert"><span>{t("事件读取失败，保留已加载记录。")}<small>{events.error === "AGENT_READ_TIMEOUT" ? t("读取超时") : events.error}</small></span><Button variant="secondary" size="sm" onClick={events.refresh}>{t("重试")}</Button></div>}
    {events.hasUpdates && <Button className="ag-new-events" variant="secondary" size="sm" onClick={() => { setLimit(100); events.refresh(); }}>{t("有新事件，刷新列表")}</Button>}
    <div aria-busy={events.loading}>{events.events.length ? <ol className="ag-event-list">{events.events.slice(0, limit).map(event => <li key={event.id} data-selected={selectedId === event.id}>
      <button type="button" className="ag-event" onClick={() => onSelect(event)} aria-pressed={selectedId === event.id}>
        <span className="ag-event__time"><time dateTime={isoTimestamp(event.timestamp)} title={timestamp(event.timestamp, locale)}>{timestamp(event.timestamp, locale, true)}</time><small>{calendarDate(event.timestamp, locale)}</small></span>
        <span className="ag-event__marker" data-status={event.status ?? "unknown"} aria-hidden="true" />
        <span className="ag-event__body"><span className="ag-event__heading"><strong>{event.title}</strong>{(event.status || event.kind.startsWith("tool_")) && <StatusBadge status={event.status ?? "unknown"} />}</span>
          {event.summary && <span className="ag-event__summary">{event.summary}</span>}
          <span className="ag-event__meta"><span>{agents.get(event.agentId)?.name ?? event.agentId}</span><span title={event.kind}>{eventKindText(event.kind, t)}</span><span title={`${event.sourcePath}:${event.sourceLine}`}>{event.sourcePath}:{event.sourceLine}</span></span>
        </span>
      </button>
    </li>)}</ol> : !events.loading && !invalidDate && !invalidRange && <EmptyObservation title={t("尚无事件")} icon={<History size={28} strokeWidth={1.3} />}><p>{t("当前范围没有事件。")}</p></EmptyObservation>}
      {events.loading && !events.events.length && <div className="ag-loading" role="status">{t("正在读取")}</div>}</div>
    <div className="ag-list-footer"><span>{t("显示")} {count(Math.min(limit, events.events.length), locale)} / {count(events.total, locale)}</span>{limit < events.events.length ? <Button variant="secondary" size="sm" onClick={() => setLimit(value => value + 100)}>{t("显示更多")}</Button> : events.cursor ? <Button variant="secondary" size="sm" icon={<ArrowDown size={14} />} loading={events.loading} onClick={() => { setLimit(value => value + 60); events.loadMore(); }}>{t("加载更多事件")}</Button> : events.total != null && !events.loading && <span>{t("已到最后一页")}</span>}</div>
  </section>;
}
