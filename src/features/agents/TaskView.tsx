import { useMemo, useState } from "react";
import { ArrowUpRight, CheckCheck, Search } from "lucide-react";
import { Button } from "../../ui";
import type { AgentSnapshot, AgentStatus } from "../../types/agents";
import { count, elapsed, isoTimestamp, timestamp } from "./format";
import { statusText, useAgentText } from "./i18n";
import { EmptyObservation, StatusBadge } from "./presentation";

const statuses: AgentStatus[] = ["running", "waiting", "completed", "failed", "interrupted", "unknown"];
export function TaskView({ snapshot, selectedId, onSelect, onLocate, now }: { snapshot: AgentSnapshot; selectedId: string | null; onSelect: (id: string) => void; onLocate: (id: string) => void; now: number }) {
  const { t, locale } = useAgentText();
  const [status, setStatus] = useState<AgentStatus | "all">("all"), [search, setSearch] = useState("");
  const [limit, setLimit] = useState(40);
  const agents = useMemo(() => new Map(snapshot.agents.map(agent => [agent.id, agent])), [snapshot.agents]);
  const totals = useMemo(() => {
    const result = new Map<AgentStatus, number>();
    snapshot.tasks.forEach(task => result.set(task.status, (result.get(task.status) ?? 0) + 1));
    return result;
  }, [snapshot.tasks]);
  const tasks = useMemo(() => snapshot.tasks.filter(task => (status === "all" || task.status === status) && `${task.title} ${task.summary ?? ""} ${task.id} ${agents.get(task.agentId)?.name ?? task.agentId}`.toLocaleLowerCase().includes(search.trim().toLocaleLowerCase()))
    .sort((left, right) => right.lastEventAt - left.lastEventAt || left.id.localeCompare(right.id)), [snapshot.tasks, status, search, agents]);
  return <section className="ag-task-view"><div className="ag-view-heading"><div><h2>{t("任务")}</h2><p>{count(snapshot.tasks.length, locale)} {t("任务")} · {count(totals.get("running") ?? 0, locale)} {t("运行中")}</p></div>
    <div className="ag-search"><Search size={15} aria-hidden="true" /><input aria-label={t("搜索任务")} placeholder={t("搜索任务")} value={search} onChange={event => { setSearch(event.target.value); setLimit(40); }} /></div></div>
    <div className="ag-status-filters" aria-label={t("全部状态")}><button type="button" aria-pressed={status === "all"} onClick={() => { setStatus("all"); setLimit(40); }}>{t("全部状态")}<span>{count(snapshot.tasks.length, locale)}</span></button>
      {statuses.map(value => <button type="button" key={value} aria-pressed={status === value} onClick={() => { setStatus(value); setLimit(40); }}><StatusBadge status={value} /><span>{count(totals.get(value) ?? 0, locale)}</span></button>)}</div>
    {tasks.length ? <><div className="ag-task-grid">{tasks.slice(0, limit).map(task => {
      const agent = agents.get(task.agentId);
      return <article className="ag-task-card" key={task.id} data-selected={selectedId === task.id}>
        <button type="button" className="ag-task-card__main" aria-label={`${task.title} · ${t("查看详情")}`} aria-pressed={selectedId === task.id} onClick={() => onSelect(task.id)}>
          <span className="ag-task-card__top"><StatusBadge status={task.status} /><span>{t(task.kind === "turn" ? "轮次" : "委派")}</span></span>
          <strong className="ag-task-card__title" title={task.title}>{task.title}</strong>
          <span className="ag-task-card__summary" data-error={!!task.error}>{task.error ?? task.summary ?? t("没有摘要")}</span>
          <span className="ag-task-card__metrics"><span title={timestamp(task.startedAt, locale)}>{t("经过时间")} {elapsed(task.startedAt, task.endedAt, now)}</span><span>{task.usage.records ? count(task.usage.totalTokens, locale) : "—"} Token</span></span>
        </button><footer><Button variant="quiet" size="sm" icon={<ArrowUpRight size={14} />} disabled={!agent} aria-label={`${t("定位成员")} · ${agent?.name ?? task.agentId}`} onClick={() => onLocate(task.agentId)}>{agent?.name ?? task.agentId}</Button><time dateTime={isoTimestamp(task.lastEventAt)} title={`${t("最后事件")} · ${timestamp(task.lastEventAt, locale)}`}>{timestamp(task.lastEventAt, locale, true)}</time></footer>
      </article>;
    })}</div><div className="ag-list-footer"><span>{t("显示")} {count(Math.min(limit, tasks.length), locale)} / {count(tasks.length, locale)}</span>{limit < tasks.length && <Button variant="secondary" size="sm" onClick={() => setLimit(value => value + 40)}>{t("显示更多任务")}</Button>}</div></> : <EmptyObservation title={t(snapshot.tasks.length ? "没有匹配任务" : "尚无任务记录")} icon={<CheckCheck size={28} strokeWidth={1.3} />}><p>{t("调整状态或搜索条件以查看其他任务。")}</p>{(status !== "all" || search) && <Button variant="secondary" onClick={() => { setStatus("all"); setSearch(""); }}>{t("清除筛选")}</Button>}</EmptyObservation>}
    <span className="ag-sr-only" role="status">{status === "all" ? t("全部状态") : statusText(status, t)} · {tasks.length}</span>
  </section>;
}
