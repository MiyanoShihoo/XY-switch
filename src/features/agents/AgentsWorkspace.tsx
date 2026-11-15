import { useCallback, useEffect, useMemo, useRef, useState, type KeyboardEvent } from "react";
import { ArrowLeft, CheckCheck, ChevronDown, Database, GitBranch, History, Info, LockKeyhole, PanelRightOpen, RefreshCw, Unplug, Users, Workflow } from "lucide-react";
import { Button } from "../../ui";
import type { AgentSnapshotQuery } from "../../types/agents";
import { AgentInspector, type AgentSelection } from "./AgentInspector";
import { TopologyView } from "./TopologyView";
import { TaskView } from "./TaskView";
import { TimelineView, initialEventFilter, type EventFilter } from "./TimelineView";
import { ResourceView } from "./ResourceView";
import { ReplayControls } from "./ReplayControls";
import { ConnectionBadge, EmptyObservation } from "./presentation";
import { modeText, useAgentText, type AgentText } from "./i18n";
import { count, timestamp } from "./format";
import { queryKey } from "./observation-store";
import { useObservation, useObservationClock } from "./use-observation";
import "./agents.css";

const tabs = [
  { id: "topology", label: "拓扑", icon: GitBranch },
  { id: "tasks", label: "任务", icon: CheckCheck },
  { id: "timeline", label: "时间线", icon: History },
  { id: "resources", label: "资源", icon: Database },
] as const;
type AgentTab = typeof tabs[number]["id"];

export function AgentsWorkspace({ onReturn }: { onReturn: () => void }) {
  const { t, locale } = useAgentText();
  const [teamId, setTeamId] = useState("");
  const [at, setAt] = useState<number | null>(null);
  const [tab, setTab] = useState<AgentTab>("topology");
  const [selection, setSelection] = useState<AgentSelection | null>(null);
  const [inspectorOpen, setInspectorOpen] = useState(false);
  const [filter, setFilter] = useState<EventFilter>(initialEventFilter);
  const [focusRequest, setFocusRequest] = useState<{ id: string; nonce: number } | null>(null);
  const query = useMemo<AgentSnapshotQuery>(() => ({ teamId: teamId || undefined, at: at ?? undefined }), [teamId, at]);
  const observation = useObservation(query);
  const snapshot = observation.snapshot;
  const nativeRequired = observation.connection === "native-required";
  const sameScope = !!snapshot && !!observation.snapshotQuery && queryKey(query) === queryKey(observation.snapshotQuery);
  const disconnected = observation.connection === "disconnected";
  const now = useObservationClock(snapshot?.replayAt ?? null, snapshot && (disconnected || !sameScope) ? snapshot.generatedAt : null);
  const inspector = useRef<HTMLElement>(null);
  const returnFocus = useRef<HTMLElement | SVGElement | null>(null);
  const returnAgentId = useRef<string | null>(null);
  const focusFrame = useRef<number | null>(null);
  const detailsVisible = inspectorOpen && !!selection && !!snapshot;
  const displayedTeamId = observation.snapshotQuery?.teamId;
  const displayedTeam = displayedTeamId ? observation.teams.find(team => team.id === displayedTeamId)?.name ?? displayedTeamId : t("所有团队");

  const openDetails = useCallback((next: AgentSelection) => {
    const origin = document.activeElement;
    if ((origin instanceof HTMLElement || origin instanceof SVGElement) && !inspector.current?.contains(origin)) {
      returnFocus.current = origin;
      returnAgentId.current = origin.closest("[data-agent-id]")?.getAttribute("data-agent-id") ?? null;
    }
    setSelection(next);
    setInspectorOpen(true);
    if (focusFrame.current != null) cancelAnimationFrame(focusFrame.current);
    focusFrame.current = requestAnimationFrame(() => {
      focusFrame.current = null;
      inspector.current?.focus({ preventScroll: true });
      if (window.matchMedia("(max-width: 1100px)").matches) inspector.current?.scrollIntoView({ block: "start", behavior: "instant" });
    });
  }, []);
  const closeDetails = useCallback(() => {
    setInspectorOpen(false);
    if (focusFrame.current != null) cancelAnimationFrame(focusFrame.current);
    focusFrame.current = requestAnimationFrame(() => {
      focusFrame.current = null;
      const member = returnAgentId.current ? document.querySelector<HTMLElement | SVGElement>(`[data-agent-id="${CSS.escape(returnAgentId.current)}"]`) : null;
      const target = returnFocus.current?.isConnected ? returnFocus.current : member?.querySelector<HTMLElement>(".ag-node__select") ?? member;
      target?.focus({ preventScroll: true });
    });
  }, []);
  const locateAgent = useCallback((id: string) => {
    setTab("topology");
    setSelection({ kind: "agent", id });
    setFocusRequest(previous => ({ id, nonce: (previous?.nonce ?? 0) + 1 }));
    if (window.matchMedia("(max-width: 1100px)").matches) setInspectorOpen(false);
    requestAnimationFrame(() => document.getElementById("ag-panel-topology")?.scrollIntoView({ block: "nearest" }));
  }, []);
  const showEvents = useCallback((agentId: string, taskId?: string) => {
    setFilter(previous => ({ ...previous, agentId, taskId: taskId ?? "" }));
    setTab("timeline");
    if (window.matchMedia("(max-width: 1100px)").matches) setInspectorOpen(false);
  }, []);
  const selectAgent = useCallback((id: string) => openDetails({ kind: "agent", id }), [openDetails]);
  useEffect(() => () => { if (focusFrame.current != null) cancelAnimationFrame(focusFrame.current); }, []);

  const changeTeam = (value: string) => {
    setTeamId(value); setSelection(null); setInspectorOpen(false); setFocusRequest(null); setFilter(initialEventFilter);
  };
  const tabKeyDown = (event: KeyboardEvent<HTMLButtonElement>, index: number) => {
    const next = event.key === "ArrowRight" ? (index + 1) % tabs.length : event.key === "ArrowLeft" ? (index + tabs.length - 1) % tabs.length : event.key === "Home" ? 0 : event.key === "End" ? tabs.length - 1 : null;
    if (next == null) return;
    event.preventDefault(); setTab(tabs[next].id); document.getElementById(`ag-tab-${tabs[next].id}`)?.focus();
  };
  const connectionLabel: AgentText = nativeRequired ? "真实数据未连接" : disconnected ? "已断开" : observation.connection === "connecting" ? "正在读取" : "已连接";
  const navigation = <div className="ag-tabs-bar"><div className="ag-tabs" role="tablist" aria-label={t("Agent 编队")}>{tabs.map((item, index) => <button type="button" role="tab" key={item.id} id={`ag-tab-${item.id}`} aria-controls={`ag-panel-${item.id}`} aria-selected={tab === item.id} tabIndex={tab === item.id ? 0 : -1} onKeyDown={event => tabKeyDown(event, index)} onClick={() => setTab(item.id)}><item.icon size={16} aria-hidden="true" />{t(item.label)}</button>)}</div>{snapshot && selection && !detailsVisible && <Button variant="quiet" size="sm" icon={<PanelRightOpen size={16} />} onClick={() => openDetails(selection)}>{t("展开详情")}</Button>}</div>;

  return <section className="xy-agent-workspace xy-agents" aria-labelledby="agent-workspace-title">
    <header className="ag-workspace-header">
      <div className="ag-workspace-heading"><span className="ag-workspace-icon"><Workflow size={20} strokeWidth={1.5} aria-hidden="true" /></span><div className="ag-title-line"><h1 id="agent-workspace-title">{t("Agent 编队")}</h1><span className="ag-readonly" title={t("跟随真实执行，读懂成员之间的协作。")}><LockKeyhole size={11} aria-hidden="true" />{t("只读观测")}</span></div></div>
      <div className="ag-header-actions">{!nativeRequired && <Button variant="secondary" size="sm" icon={<RefreshCw size={14} />} loading={observation.loading} onClick={observation.refresh}>{t("刷新")}</Button>}</div>
    </header>

    <div className="ag-scope-bar"><label className="ag-team-field"><Users size={16} aria-hidden="true" /><span>{t("团队")}</span><select aria-label={t("团队")} value={teamId} disabled={nativeRequired} onChange={event => changeTeam(event.target.value)}><option value="">{t("所有团队")}</option>{observation.teams.map(team => <option key={team.id} value={team.id}>{team.name}</option>)}{teamId && !observation.teams.some(team => team.id === teamId) && <option value={teamId}>{teamId}</option>}</select></label>
      {snapshot && <dl className="ag-summary" aria-label={t("只读观测")}><div><dt>{t("成员")}</dt><dd>{count(snapshot.agents.length, locale)}</dd></div><div><dt>{t("任务")}</dt><dd>{count(snapshot.tasks.length, locale)}</dd></div><div title={`${count(snapshot.totals.records, locale)} ${t("用量记录")}`}><dt>{t("累计 Token")}</dt><dd>{snapshot.totals.records ? count(snapshot.totals.totalTokens, locale) : "—"}</dd></div></dl>}
      <div className="ag-scope-status"><span className="ag-transport" data-state={observation.connection}><i aria-hidden="true" />{t(connectionLabel)}{observation.loading && snapshot && <small>{t("正在读取")}</small>}</span>
        {snapshot && <details className="ag-source-menu"><summary><Database size={13} aria-hidden="true" />{t("真实数据源")}<span>{snapshot.sources.filter(source => source.connection === "connected").length}/{snapshot.sources.length}</span><ChevronDown size={12} aria-hidden="true" /></summary><div>{snapshot.sources.map(source => <button type="button" key={source.id} className="ag-source-chip" onClick={event => {
          const disclosure = event.currentTarget.closest("details");
          openDetails({ kind: "source", id: source.id });
          if (disclosure) { disclosure.open = false; returnFocus.current = disclosure.querySelector("summary"); }
        }} title={`${source.detail} · ${t("源能力与连接详情")}`}><strong>{source.name}</strong><ConnectionBadge connection={source.connection} /><span className="ag-mode" data-mode={source.mode}>{modeText(source.mode, t)}</span></button>)}{!snapshot.sources.length && <p>{t("没有可用数据源")}</p>}</div></details>}
      </div>
    </div>

    {!nativeRequired && <div className="ag-observation-notices" aria-live="polite">
      {disconnected && <div className="ag-notice" data-tone="danger" role="alert"><Unplug size={18} aria-hidden="true" /><div><strong>{t(snapshot ? "连接中断，保留上次快照" : "读取失败")}</strong><small>{observation.error === "AGENT_READ_TIMEOUT" ? t("读取超时") : observation.error}</small>{snapshot && <small>{t("最后读取成功")} · {timestamp(observation.lastSuccessAt, locale)}</small>}</div><Button variant="secondary" size="sm" loading={observation.loading} onClick={observation.refresh}>{t("重试")}</Button></div>}
      {snapshot && !sameScope && <div className="ag-notice" data-tone="warning"><Info size={17} aria-hidden="true" /><div><strong>{t(disconnected ? "所选范围读取失败，当前仍显示上次成功快照。" : "正在加载所选范围，当前仍显示上次成功快照。")}</strong><small>{displayedTeam} · {snapshot.replayAt == null ? t("实时观测") : `${t("历史回放")} ${timestamp(snapshot.replayAt, locale)}`}</small></div></div>}
      {observation.notifications === "disconnected" && <div className="ag-notice" data-tone="warning"><Info size={17} aria-hidden="true" /><div><strong>{t("通知连接中断，正在低频校准")}</strong><small>{observation.notificationError}</small></div></div>}
    </div>}

    {nativeRequired ? navigation : <ReplayControls key={teamId} navigation={navigation} at={at} displayedAt={snapshot?.replayAt ?? null} history={observation.history} ready={sameScope} loading={observation.loading} failed={disconnected} onChange={setAt} />}

    {nativeRequired ? <div className="ag-native-required" id={`ag-panel-${tab}`} role="tabpanel" aria-labelledby={`ag-tab-${tab}`}><span className="ag-native-required__icon"><Unplug size={32} strokeWidth={1.35} aria-hidden="true" /></span><span className="ag-eyebrow">{t("真实数据未连接")}</span><h2>{t("需要原生桌面程序")}</h2><p>{t("请在 XY Switch 桌面程序中打开 Agent 编队，读取本地客户端的真实事件。浏览器预览不连接本地观测源。")}</p><Button variant="secondary" icon={<ArrowLeft size={15} />} onClick={onReturn}>{t("返回配置工作区")}</Button></div>
      : snapshot ? <div className="ag-workspace-body" data-details={detailsVisible}>
        <div className="ag-workspace-content">
          <div id="ag-panel-topology" role="tabpanel" aria-labelledby="ag-tab-topology" hidden={tab !== "topology"}><TopologyView key={displayedTeamId ?? "all"} snapshot={snapshot} selectedId={selection?.kind === "agent" ? selection.id : null} focusRequest={focusRequest} onSelect={selectAgent} now={now} /></div>
          {tab === "tasks" && <div id="ag-panel-tasks" role="tabpanel" aria-labelledby="ag-tab-tasks"><TaskView snapshot={snapshot} now={now} selectedId={selection?.kind === "task" ? selection.id : null} onSelect={id => openDetails({ kind: "task", id })} onLocate={locateAgent} /></div>}
          {tab === "timeline" && <div id="ag-panel-timeline" role="tabpanel" aria-labelledby="ag-tab-timeline"><TimelineView snapshot={snapshot} teamId={displayedTeamId ?? ""} enabled={sameScope} changeVersion={observation.changeVersion} filter={filter} onFilter={setFilter} onSelect={event => openDetails({ kind: "event", event })} selectedId={selection?.kind === "event" ? selection.event.id : null} /></div>}
          {tab === "resources" && <div id="ag-panel-resources" role="tabpanel" aria-labelledby="ag-tab-resources"><ResourceView snapshot={snapshot} onSource={id => openDetails({ kind: "source", id })} onUsage={(dimension, id) => openDetails({ kind: "usage", dimension, id })} /></div>}
        </div>
        {detailsVisible && <aside className="ag-inspector" ref={inspector} tabIndex={-1} aria-labelledby="ag-inspector-title" onKeyDown={event => { if (event.key === "Escape") { event.stopPropagation(); closeDetails(); } }}><AgentInspector selection={selection!} snapshot={snapshot} now={now} onClose={closeDetails} onSelect={openDetails} onLocate={locateAgent} onEvents={showEvents} onReplay={setAt} /></aside>}
      </div> : <div className="ag-initial-state" id={`ag-panel-${tab}`} role="tabpanel" aria-labelledby={`ag-tab-${tab}`} aria-busy={observation.loading}><EmptyObservation title={t(disconnected ? "真实数据未连接" : "等待首次快照")} icon={disconnected ? <Unplug size={30} strokeWidth={1.3} /> : <Database size={30} strokeWidth={1.3} />}><p>{t("读取本地事件后，成员、协作关系与任务会出现在这里。")}</p></EmptyObservation></div>}

    {snapshot && <footer className="ag-workspace-footer"><span>{t("快照生成于")} · {timestamp(snapshot.generatedAt, locale)}</span><details className="ag-observation-details"><summary><Info size={13} aria-hidden="true" />{t("观测说明")}{snapshot.warnings.length > 0 && ` · ${snapshot.warnings.length}`}</summary><div><p>{t("没有收到新事件不会改变成员的执行状态。")}</p><dl><div><dt>{t("快照读取")}</dt><dd>{t(connectionLabel)}</dd></div><div><dt>{t("通知通道")}</dt><dd>{t(observation.notifications === "connected" ? "已连接" : observation.notifications === "connecting" ? "正在恢复通知连接" : "已断开")}</dd></div><div><dt>{t("保留天数")}</dt><dd>{count(snapshot.history.retentionDays, locale)} · {t("原始文件不受影响")}</dd></div><div><dt>{t("可用历史")}</dt><dd>{timestamp(observation.history?.oldestAt, locale)} — {timestamp(observation.history?.latestAt, locale)}</dd></div></dl>{snapshot.history.truncated && <p>{t("历史已截断")}</p>}{snapshot.warnings.length > 0 && <ul>{snapshot.warnings.map((warning, index) => <li key={index}>{warning}</li>)}</ul>}</div></details></footer>}
  </section>;
}
