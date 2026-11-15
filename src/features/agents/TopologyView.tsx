import { memo, useCallback, useEffect, useId, useMemo, useRef, useState, type PointerEvent as ReactPointerEvent } from "react";
import { ChevronDown, ChevronRight, Expand, Focus, GitBranch, LayoutGrid, Minus, Plus, Search } from "lucide-react";
import { Button, IconButton } from "../../ui";
import type { AgentNode, AgentSnapshot, AgentTask } from "../../types/agents";
import { compactCount, count, elapsed } from "./format";
import { clientName, edgeKindText, statusText, useAgentText } from "./i18n";
import { ConnectionBadge, EmptyObservation, StatusBadge } from "./presentation";
import { ancestorsOf, fitViewport, indexTopology, inViewport, layoutTopology, MIN_SCALE, NODE_HEIGHT, NODE_WIDTH, relatedNodes, type Point, type Viewport } from "./topology";

interface TopologyProps {
  snapshot: AgentSnapshot;
  selectedId: string | null;
  focusRequest: { id: string; nonce: number } | null;
  onSelect: (id: string) => void;
  now: number;
}
interface Drag { pointer: number; kind: "pan" | "node"; id?: string; start: Point; origin: Point; moved: boolean; scale: number }

const NodeCard = memo(function NodeCard({ node, task, now, selected, relation, position, viewport, hiddenCount, hasChildren, collapsed, onToggle, onPointerDown, onSelect }: {
  node: AgentNode; task: AgentTask | undefined; now: number; selected: boolean; relation: string;
  position: Point; viewport: Viewport; hiddenCount: number; hasChildren: boolean; collapsed: boolean;
  onToggle: (id: string) => void; onPointerDown: (event: ReactPointerEvent, id: string) => void; onSelect: (id: string) => void;
}) {
  const { t, locale } = useAgentText();
  return <article className="ag-node" data-selected={selected} data-relation={relation} data-agent-id={node.id}
    style={{ width: NODE_WIDTH, height: NODE_HEIGHT, transform: `translate(${position.x * viewport.scale + viewport.x}px, ${position.y * viewport.scale + viewport.y}px) scale(${viewport.scale})` }}>
    <button type="button" className="ag-node__select" aria-label={`${node.name} · ${t("查看详情")}`} aria-pressed={selected}
      onPointerDown={event => onPointerDown(event, node.id)} onClick={event => { if (event.detail === 0) onSelect(node.id); }}>
      <span className="ag-node__heading"><span className="ag-node__avatar" aria-hidden="true">{node.name.trim().slice(0, 1).toUpperCase() || "A"}</span><strong title={node.name}>{node.name}</strong><StatusBadge status={node.status} /></span>
      <span className="ag-node__metadata"><span className="ag-node__role" title={`${node.role ?? t("未报告角色")} · ${clientName(node.clientId)}`}>{node.role ?? clientName(node.clientId)}</span><span className="ag-node__model" title={node.model ?? t("未报告模型")}>{node.model ?? t("未报告模型")}</span></span>
      <span className="ag-node__task" title={task?.title ?? node.currentTaskId ?? t("没有当前任务")}><span>{t("当前任务")}</span><strong>{task?.title ?? (node.currentTaskId ? t("未报告任务标题") : t("没有当前任务"))}</strong></span>
      <span className="ag-node__metrics"><span title={t("经过时间")}><strong>{elapsed(node.startedAt, node.endedAt, now)}</strong></span><span title={node.usage.records ? count(node.usage.totalTokens, locale) : t("没有用量记录")}><small>{t("累计 Token")}</small><strong>{node.usage.records ? compactCount(node.usage.totalTokens, locale) : "—"}</strong></span>{node.connection !== "connected" && <ConnectionBadge connection={node.connection} />}</span>
    </button>
    {hasChildren && <button type="button" className="ag-node__collapse" aria-label={`${t(collapsed ? "展开子层" : "折叠子层")} · ${node.name}`} aria-expanded={!collapsed} onClick={() => onToggle(node.id)}>{collapsed ? <ChevronRight size={13} /> : <ChevronDown size={13} />}{hiddenCount ? `+${hiddenCount}` : <GitBranch size={12} />}</button>}
  </article>;
});

export function TopologyView({ snapshot, selectedId, focusRequest, onSelect, now }: TopologyProps) {
  const { t, locale } = useAgentText();
  const marker = useId().replace(/:/g, "");
  const canvas = useRef<HTMLDivElement>(null);
  const [size, setSize] = useState({ width: 0, height: 0 });
  const [collapsed, setCollapsed] = useState<Set<string>>(() => new Set());
  const [overrides, setOverrides] = useState<Map<string, Point>>(() => new Map());
  const [viewport, setViewport] = useState<Viewport>({ x: 30, y: 30, scale: 1 });
  const [search, setSearch] = useState("");
  const [pendingFocus, setPendingFocus] = useState<string | null>(null);
  const drag = useRef<Drag | null>(null);
  const frame = useRef<number | null>(null), pendingPointer = useRef<Point | null>(null);
  const fitted = useRef(false);
  const consumedFocus = useRef<number | null>(null);
  const structuralKey = snapshot.agents.map(node => `${node.id}\0${node.parentId}\0${node.createdAt}`).join("\n") + "\n" + snapshot.edges.map(edge => `${edge.id}\0${edge.sourceAgentId}\0${edge.targetAgentId}\0${edge.kind}`).join("\n");
  const index = useMemo(() => indexTopology(snapshot.agents, snapshot.edges), [structuralKey]);
  const agents = useMemo(() => new Map(snapshot.agents.map(node => [node.id, node])), [snapshot.agents]);
  const tasks = useMemo(() => new Map(snapshot.tasks.map(task => [task.id, task])), [snapshot.tasks]);
  const layout = useMemo(() => layoutTopology(index, collapsed), [index, collapsed]);
  const depthSelection = useMemo(() => {
    if (!collapsed.size) return "all";
    for (const depth of [0, 1, 2]) {
      const expected = [...index.depths].filter(([id, level]) => level >= depth && !!index.children.get(id)?.length);
      if (expected.length === collapsed.size && expected.every(([id]) => collapsed.has(id))) return String(depth);
    }
    return "custom";
  }, [index, collapsed]);
  const positions = useMemo(() => new Map([...layout.positions].map(([id, point]) => [id, overrides.get(id) ?? point])), [layout, overrides]);
  const relationships = useMemo(() => relatedNodes(index, selectedId), [index, selectedId]);
  const visible = useMemo(() => layout.visible.filter(id => inViewport(positions.get(id)!, viewport, size.width, size.height)), [layout, positions, viewport, size]);
  const compact = viewport.scale < 0.72;
  const related = (id: string) => id === selectedId ? "selected" : relationships.upstream.has(id) ? "upstream" : relationships.downstream.has(id) ? "downstream" : selectedId ? "unrelated" : "none";
  const fits = useCallback(() => setViewport(fitViewport(positions.values(), size.width, size.height)), [positions, size]);
  useEffect(() => {
    const element = canvas.current;
    if (!element) return;
    const observer = new ResizeObserver(entries => {
      const rect = entries[0].contentRect;
      if (rect.width && rect.height) setSize({ width: rect.width, height: rect.height });
    });
    observer.observe(element);
    return () => observer.disconnect();
  }, []);
  useEffect(() => {
    if (fitted.current || !positions.size || !size.width || !size.height) return;
    const all = fitViewport(positions.values(), size.width, size.height);
    setViewport(all);
    fitted.current = true;
  }, [positions, size]);
  const focusNode = useCallback((id: string) => {
    setCollapsed(previous => { const next = new Set(previous); ancestorsOf(index, id).forEach(parent => next.delete(parent)); return next; });
    setPendingFocus(id); setSearch("");
  }, [index]);
  useEffect(() => {
    if (!focusRequest || consumedFocus.current === focusRequest.nonce || !index.nodes.has(focusRequest.id)) return;
    consumedFocus.current = focusRequest.nonce; focusNode(focusRequest.id);
  }, [focusRequest, focusNode, index]);
  useEffect(() => {
    if (!pendingFocus || !size.width || !size.height) return;
    const point = positions.get(pendingFocus);
    if (!point) return;
    const scale = Math.max(MIN_SCALE, Math.min(1, (size.width - 56) / NODE_WIDTH, (size.height - 80) / NODE_HEIGHT));
    setViewport({ scale, x: size.width / 2 - (point.x + NODE_WIDTH / 2) * scale, y: size.height / 2 - (point.y + NODE_HEIGHT / 2) * scale });
    setPendingFocus(null);
  }, [pendingFocus, positions, size]);
  const zoom = useCallback((factor: number, anchor?: Point) => {
    const center = anchor ?? { x: size.width / 2, y: size.height / 2 };
    setViewport(previous => {
      const scale = Math.min(1.8, Math.max(MIN_SCALE, previous.scale * factor));
      return { scale, x: center.x - (center.x - previous.x) * scale / previous.scale, y: center.y - (center.y - previous.y) * scale / previous.scale };
    });
  }, [size]);
  useEffect(() => {
    const element = canvas.current;
    if (!element) return;
    const wheel = (event: WheelEvent) => {
      if (!event.ctrlKey && !event.metaKey && document.activeElement !== element && !element.contains(document.activeElement)) return;
      event.preventDefault();
      if (event.ctrlKey || event.metaKey) {
        const rect = element.getBoundingClientRect();
        zoom(Math.exp(-event.deltaY * 0.005), { x: event.clientX - rect.left, y: event.clientY - rect.top });
      } else setViewport(previous => ({ ...previous, x: previous.x - event.deltaX, y: previous.y - event.deltaY }));
    };
    element.addEventListener("wheel", wheel, { passive: false });
    return () => element.removeEventListener("wheel", wheel);
  }, [zoom]);
  useEffect(() => () => { if (frame.current != null) cancelAnimationFrame(frame.current); }, []);
  const applyPointer = () => {
    const active = drag.current, pointer = pendingPointer.current;
    if (!active || !pointer) return;
    const deltaX = pointer.x - active.start.x, deltaY = pointer.y - active.start.y;
    if (Math.hypot(deltaX, deltaY) > 4) active.moved = true;
    if (!active.moved) return;
    if (active.kind === "pan") setViewport(previous => ({ ...previous, x: active.origin.x + deltaX, y: active.origin.y + deltaY }));
    else setOverrides(previous => new Map(previous).set(active.id!, { x: active.origin.x + deltaX / active.scale, y: active.origin.y + deltaY / active.scale }));
  };
  const beginDrag = (event: ReactPointerEvent, id?: string) => {
    if (event.button !== 0 || !event.isPrimary || !canvas.current) return;
    event.stopPropagation(); event.preventDefault();
    canvas.current.focus({ preventScroll: true });
    canvas.current.setPointerCapture(event.pointerId);
    pendingPointer.current = { x: event.clientX, y: event.clientY };
    drag.current = { pointer: event.pointerId, kind: id ? "node" : "pan", id, start: { x: event.clientX, y: event.clientY }, origin: id ? positions.get(id)! : viewport, moved: false, scale: viewport.scale };
  };
  const endDrag = (event: ReactPointerEvent, cancelled = false) => {
    if (drag.current?.pointer !== event.pointerId) return;
    if (frame.current != null) { cancelAnimationFrame(frame.current); frame.current = null; }
    if (!cancelled) pendingPointer.current = { x: event.clientX, y: event.clientY };
    applyPointer();
    const active = drag.current; drag.current = null; pendingPointer.current = null;
    if (canvas.current?.hasPointerCapture(event.pointerId)) canvas.current.releasePointerCapture(event.pointerId);
    if (!cancelled && !active.moved && active.id) onSelect(active.id);
  };
  const toggle = useCallback((id: string) => { fitted.current = false; setCollapsed(previous => { const next = new Set(previous); if (next.has(id)) next.delete(id); else next.add(id); return next; }); }, []);
  const matches = search.trim() ? snapshot.agents.filter(node => `${node.name} ${node.role ?? ""} ${node.id} ${node.model ?? ""}`.toLocaleLowerCase().includes(search.trim().toLocaleLowerCase())).slice(0, 10) : [];
  return <section className="ag-topology">
    <div className="ag-graph-toolbar"><div className="ag-search ag-graph-search"><Search size={15} aria-hidden="true" /><input aria-label={t("定位成员")} placeholder={t("搜索成员名称、角色或 ID")} value={search} onChange={event => setSearch(event.target.value)} onKeyDown={event => {
      if (event.key === "Escape") setSearch("");
      if (event.key === "Enter" && matches[0]) { focusNode(matches[0].id); onSelect(matches[0].id); }
    }} />{search.trim() && <div className="ag-search-results">{matches.map(node => <button type="button" key={node.id} onClick={() => { focusNode(node.id); onSelect(node.id); }}><strong>{node.name}</strong><small>{node.role ?? node.id}</small></button>)}{!matches.length && <p>{t("没有匹配成员")}</p>}</div>}</div>
      <div className="ag-graph-actions"><label className="ag-inline-field"><span>{t("层级")}</span><select aria-label={t("层级")} value={depthSelection} onChange={event => {
        const depth = event.target.value === "all" ? Infinity : Number(event.target.value);
        fitted.current = false;
        setCollapsed(new Set([...index.depths].filter(([id, level]) => level >= depth && !!index.children.get(id)?.length).map(([id]) => id)));
      }}><option value="all">{t("全部层级")}</option><option value="0">{t("仅根节点")}</option><option value="1">{t("两层")}</option><option value="2">{t("三层")}</option>{depthSelection === "custom" && <option value="custom" disabled>{t("自定义层级")}</option>}</select></label>
      <Button variant="quiet" size="sm" icon={<LayoutGrid size={15} />} onClick={() => { setOverrides(new Map()); fitted.current = false; }}>{t("自动布局")}</Button>
      <IconButton size="sm" aria-label={t("展开全部")} title={t("展开全部")} onClick={() => { fitted.current = false; setCollapsed(new Set()); }}><Expand size={16} /></IconButton></div>
    </div>
    <div className="ag-canvas" ref={canvas} tabIndex={0} role="region" aria-label={t("拓扑画布")} aria-describedby="ag-canvas-help" onPointerDown={event => { if (event.target instanceof Element && event.target.closest("button")) return; beginDrag(event); }}
      onPointerMove={event => {
        if (drag.current?.pointer !== event.pointerId) return;
        pendingPointer.current = { x: event.clientX, y: event.clientY };
        if (frame.current == null) frame.current = requestAnimationFrame(() => { frame.current = null; applyPointer(); });
      }} onPointerUp={event => endDrag(event)} onPointerCancel={event => endDrag(event, true)} onLostPointerCapture={event => endDrag(event, true)}
      onKeyDown={event => {
        if (event.target !== event.currentTarget) return;
        const direction = { ArrowLeft: [70, 0], ArrowRight: [-70, 0], ArrowUp: [0, 70], ArrowDown: [0, -70] }[event.key];
        if (direction) { event.preventDefault(); setViewport(previous => ({ ...previous, x: previous.x + direction[0], y: previous.y + direction[1] })); }
        if (event.key === "+" || event.key === "=") { event.preventDefault(); zoom(1.25); }
        if (event.key === "-") { event.preventDefault(); zoom(0.8); }
        if (event.key === "0") { event.preventDefault(); fits(); }
      }}>
      <svg className="ag-connections" width="100%" height="100%" aria-hidden="true"><defs><marker id={marker} viewBox="0 0 10 10" refX="9" refY="5" markerWidth="6" markerHeight="6" orient="auto-start-reverse"><path d="M 0 0 L 10 5 L 0 10 z" fill="context-stroke" /></marker></defs>
        {layout.edges.map(edge => {
          const source = positions.get(edge.source)!, target = positions.get(edge.target)!;
          const start = { x: (source.x + NODE_WIDTH) * viewport.scale + viewport.x, y: (source.y + NODE_HEIGHT / 2) * viewport.scale + viewport.y };
          const end = { x: target.x * viewport.scale + viewport.x, y: (target.y + NODE_HEIGHT / 2) * viewport.scale + viewport.y };
          if (Math.max(start.x, end.x) < -80 || Math.min(start.x, end.x) > size.width + 80 || Math.max(start.y, end.y) < -180 || Math.min(start.y, end.y) > size.height + 180) return null;
          const bend = Math.max(32, Math.abs(end.x - start.x) * 0.5), isSelf = edge.source === edge.target;
          const path = isSelf ? `M ${start.x} ${start.y} C ${start.x + 60} ${start.y - 180} ${end.x - 60} ${end.y - 180} ${end.x} ${end.y}` : `M ${start.x} ${start.y} C ${start.x + bend} ${start.y} ${end.x - bend} ${end.y} ${end.x} ${end.y}`;
          const relationship = related(edge.target) === "upstream" || edge.target === selectedId ? "upstream" : related(edge.source) === "downstream" || edge.source === selectedId ? "downstream" : selectedId ? "unrelated" : "none";
          return <path key={edge.id} d={path} className="ag-edge" data-kind={edge.kind} data-relation={relationship} markerEnd={!compact || relationship === "upstream" || relationship === "downstream" ? `url(#${marker})` : undefined}><title>{edgeKindText(edge.kind, t)} · {edge.count}</title></path>;
        })}
      </svg>
      {compact ? <svg className="ag-compact-nodes" width="100%" height="100%" role="group" aria-label={t("缩小概览，放大查看完整节点信息")}>{visible.map(id => {
        const node = agents.get(id)!, point = positions.get(id)!;
        const width = Math.max(2, NODE_WIDTH * viewport.scale), height = Math.max(2, NODE_HEIGHT * viewport.scale);
        const smallLabel = width < 68, insetX = smallLabel ? 3 : 8, insetY = smallLabel ? 3 : 6;
        return <g key={id} data-agent-id={id} data-selected={selectedId === id} data-relation={related(id)} data-status={node.status} className="ag-compact-node" role="button" tabIndex={0} aria-label={`${node.name} · ${statusText(node.status, t)} · ${t("定位成员")}`} transform={`translate(${point.x * viewport.scale + viewport.x},${point.y * viewport.scale + viewport.y})`} onPointerDown={event => { event.stopPropagation(); }} onKeyDown={event => { if (event.key === "Enter" || event.key === " ") { event.preventDefault(); focusNode(id); onSelect(id); } }} onClick={() => { focusNode(id); onSelect(id); }}><title>{node.name}</title><rect width={width} height={height} rx={5} />{width >= 42 && height >= 24 && <foreignObject x={insetX} y={insetY} width={width - insetX * 2} height={height - insetY * 2}><div className="ag-compact-node__copy" data-small={smallLabel}><strong>{node.name}</strong>{height >= 62 && <StatusBadge status={node.status} />}{height >= 85 && <span>{node.role ?? clientName(node.clientId)}</span>}{height >= 104 && <span>{node.model ?? t("未报告模型")}</span>}</div></foreignObject>}</g>;
      })}</svg> : visible.map(id => {
        const node = agents.get(id)!;
        return <NodeCard key={id} node={node} task={node.currentTaskId ? tasks.get(node.currentTaskId) : undefined} now={now} selected={selectedId === id} relation={related(id)} position={positions.get(id)!} viewport={viewport}
          hiddenCount={layout.hiddenCounts.get(id) ?? 0} hasChildren={!!index.children.get(id)?.length} collapsed={collapsed.has(id)} onToggle={toggle} onPointerDown={beginDrag} onSelect={onSelect} />;
      })}
      {!snapshot.agents.length && <EmptyObservation title={t(snapshot.replayAt != null ? "此时刻没有成员" : "尚无成员记录")}><p>{t("读取本地事件后，成员、协作关系与任务会出现在这里。")}</p></EmptyObservation>}
    </div>
    <footer className="ag-graph-footer">
      <div className="ag-zoom-controls"><IconButton size="sm" aria-label={t("缩小")} onClick={() => zoom(0.8)}><Minus size={16} /></IconButton><output>{new Intl.NumberFormat(locale, { style: "percent", maximumFractionDigits: viewport.scale < 0.01 ? 3 : 0 }).format(viewport.scale)}</output><IconButton size="sm" aria-label={t("放大")} onClick={() => zoom(1.25)}><Plus size={16} /></IconButton><IconButton size="sm" aria-label={t("适应画布")} title={t("适应画布")} onClick={fits}><Focus size={16} /></IconButton></div>
      <span>{count(layout.visible.length, locale)} / {count(snapshot.agents.length, locale)} {t("可见成员")}{index.cycles.size > 0 && ` · ${t("包含循环关系")}`}</span><span className="ag-legend"><i data-kind="upstream" />{t("上游")}<i data-kind="downstream" />{t("下游")}<i data-kind="assignment" />{t("任务分配")}<i data-kind="handoff" />{t("交接")}</span>
    </footer>
    <p className="ag-note ag-canvas-help" id="ag-canvas-help">{compact && <span className="ag-overview-hint">{t("缩小概览，放大查看完整节点信息")} · </span>}{t("拖动节点调整位置；拖动画布平移；Ctrl / ⌘ + 滚轮缩放。")} {t("方向键平移，+ / − 缩放，0 适应画布。")}</p>
  </section>;
}
