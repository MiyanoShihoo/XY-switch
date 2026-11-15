import type { AgentEdge, AgentNode } from "../../types/agents";

export const NODE_WIDTH = 252;
export const NODE_HEIGHT = 166;
export const MIN_SCALE = 0.000001;
const COLUMN_GAP = 80, ROW_GAP = 28;
export interface Point { x: number; y: number }
export interface Viewport extends Point { scale: number }
export interface TopologyLink {
  id: string; source: string; target: string; kind: AgentEdge["kind"] | "parent"; count: number;
}
export interface TopologyIndex {
  nodes: Map<string, AgentNode>;
  parents: Map<string, string>;
  children: Map<string, string[]>;
  roots: string[];
  depths: Map<string, number>;
  links: TopologyLink[];
  upstream: Map<string, string[]>;
  downstream: Map<string, string[]>;
  cycles: Set<string>;
}

export function indexTopology(agents: AgentNode[], edges: AgentEdge[]): TopologyIndex {
  const ordered = [...agents].sort((left, right) => left.createdAt - right.createdAt || left.id.localeCompare(right.id));
  const nodes = new Map(ordered.map(agent => [agent.id, agent]));
  const parents = new Map<string, string>();
  const links: TopologyLink[] = [];
  const parentPairs = new Set<string>();
  for (const agent of ordered) {
    if (agent.parentId && nodes.has(agent.parentId) && agent.parentId !== agent.id) parents.set(agent.id, agent.parentId);
  }
  for (const edge of edges) {
    if (!nodes.has(edge.sourceAgentId) || !nodes.has(edge.targetAgentId)) continue;
    links.push({ id: edge.id, source: edge.sourceAgentId, target: edge.targetAgentId, kind: edge.kind, count: 1 });
    if (edge.kind === "spawn") {
      parentPairs.add(JSON.stringify([edge.sourceAgentId, edge.targetAgentId]));
      if (!parents.has(edge.targetAgentId) && edge.sourceAgentId !== edge.targetAgentId) parents.set(edge.targetAgentId, edge.sourceAgentId);
    }
  }
  for (const agent of ordered) {
    if (agent.parentId && nodes.has(agent.parentId) && !parentPairs.has(JSON.stringify([agent.parentId, agent.id]))) {
      links.push({ id: `parent:${agent.id}`, source: agent.parentId, target: agent.id, kind: "parent", count: 1 });
    }
  }
  const cycles = new Set<string>(), settled = new Set<string>();
  for (const agent of ordered) {
    const path: string[] = [], offsets = new Map<string, number>();
    let current: string | undefined = agent.id;
    while (current && !settled.has(current)) {
      const offset = offsets.get(current);
      if (offset !== undefined) {
        const cycle = path.slice(offset);
        cycle.forEach(id => cycles.add(id));
        parents.delete([...cycle].sort()[0]);
        break;
      }
      offsets.set(current, path.length); path.push(current); current = parents.get(current);
    }
    path.forEach(id => settled.add(id));
  }
  const children = new Map<string, string[]>(), upstream = new Map<string, string[]>(), downstream = new Map<string, string[]>();
  for (const agent of ordered) { children.set(agent.id, []); upstream.set(agent.id, []); downstream.set(agent.id, []); }
  for (const agent of ordered) { const parent = parents.get(agent.id); if (parent) children.get(parent)!.push(agent.id); }
  for (const link of links) { upstream.get(link.target)!.push(link.source); downstream.get(link.source)!.push(link.target); if (link.source === link.target) cycles.add(link.source); }
  const roots = ordered.filter(agent => !parents.has(agent.id)).map(agent => agent.id);
  const depths = new Map<string, number>(), queue = roots.map(id => ({ id, depth: 0 }));
  for (let cursor = 0; cursor < queue.length; cursor++) {
    const item = queue[cursor]; depths.set(item.id, item.depth);
    for (const child of children.get(item.id) ?? []) queue.push({ id: child, depth: item.depth + 1 });
  }
  return { nodes, parents, children, roots, depths, links, upstream, downstream, cycles };
}

export function layoutTopology(index: TopologyIndex, collapsed: ReadonlySet<string>) {
  const representative = new Map<string, string>(), visible: string[] = [], hiddenCounts = new Map<string, number>();
  const queue = index.roots.map(id => ({ id, hiddenBy: null as string | null }));
  for (let cursor = 0; cursor < queue.length; cursor++) {
    const { id, hiddenBy } = queue[cursor];
    representative.set(id, hiddenBy ?? id);
    if (hiddenBy) hiddenCounts.set(hiddenBy, (hiddenCounts.get(hiddenBy) ?? 0) + 1);
    else visible.push(id);
    const nextHidden = hiddenBy ?? (collapsed.has(id) ? id : null);
    for (const child of index.children.get(id) ?? []) queue.push({ id: child, hiddenBy: nextHidden });
  }
  const levels = new Map<number, string[]>();
  for (const id of visible) {
    const depth = index.depths.get(id) ?? 0;
    if (!levels.has(depth)) levels.set(depth, []);
    levels.get(depth)!.push(id);
  }
  const positions = new Map<string, Point>();
  let rowLimit = 1, bestExtent = Infinity;
  const widestLevel = Math.max(1, ...[...levels.values()].map(ids => ids.length));
  for (let rows = 1; rows <= Math.min(widestLevel, 12); rows++) {
    const columns = [...levels.values()].reduce((total, ids) => total + Math.ceil(ids.length / rows), 0);
    const width = columns * (NODE_WIDTH + COLUMN_GAP) - COLUMN_GAP + 96;
    const height = rows * (NODE_HEIGHT + ROW_GAP) - ROW_GAP + 96;
    const extent = Math.max(width / 2, height);
    if (extent < bestExtent) { rowLimit = rows; bestExtent = extent; }
  }
  let offset = 48;
  const height = rowLimit * (NODE_HEIGHT + ROW_GAP) - ROW_GAP + 96;
  for (const [, ids] of [...levels].sort(([left], [right]) => left - right)) {
    const rows = Math.min(ids.length, rowLimit);
    const top = 48 + (rowLimit - rows) * (NODE_HEIGHT + ROW_GAP) / 2;
    ids.forEach((id, position) => positions.set(id, { x: offset + Math.floor(position / rows) * (NODE_WIDTH + COLUMN_GAP), y: top + (position % rows) * (NODE_HEIGHT + ROW_GAP) }));
    offset += Math.ceil(ids.length / rows) * (NODE_WIDTH + COLUMN_GAP);
  }
  const edges = new Map<string, TopologyLink>();
  for (const link of index.links) {
    const source = representative.get(link.source), target = representative.get(link.target);
    if (!source || !target || (source === target && link.source !== link.target)) continue;
    const key = JSON.stringify([source, target, link.kind]);
    const existing = edges.get(key);
    if (existing) existing.count += link.count;
    else edges.set(key, { ...link, source, target });
  }
  return { positions, visible, representative, hiddenCounts, edges: [...edges.values()], width: Math.max(offset - COLUMN_GAP + 48, NODE_WIDTH + 96), height };
}

export function ancestorsOf(index: TopologyIndex, id: string) {
  const result = new Set<string>();
  let parent = index.parents.get(id);
  while (parent && !result.has(parent)) { result.add(parent); parent = index.parents.get(parent); }
  return result;
}
export function relatedNodes(index: TopologyIndex, id: string | null) {
  const walk = (adjacency: Map<string, string[]>) => {
    const visited = new Set<string>();
    if (!id) return visited;
    const stack = [...adjacency.get(id) ?? []];
    while (stack.length) {
      const current = stack.pop()!;
      if (current === id || visited.has(current)) continue;
      visited.add(current); stack.push(...adjacency.get(current) ?? []);
    }
    return visited;
  };
  return { upstream: walk(index.upstream), downstream: walk(index.downstream) };
}
export function inViewport(position: Point, viewport: Viewport, width: number, height: number, margin = 80) {
  const left = position.x * viewport.scale + viewport.x, top = position.y * viewport.scale + viewport.y;
  return left < width + margin && top < height + margin && left + NODE_WIDTH * viewport.scale > -margin && top + NODE_HEIGHT * viewport.scale > -margin;
}
export function fitViewport(positions: Iterable<Point>, width: number, height: number): Viewport {
  let minX = Infinity, minY = Infinity, maxX = -Infinity, maxY = -Infinity;
  for (const position of positions) { minX = Math.min(minX, position.x); minY = Math.min(minY, position.y); maxX = Math.max(maxX, position.x + NODE_WIDTH); maxY = Math.max(maxY, position.y + NODE_HEIGHT); }
  if (!Number.isFinite(minX)) return { x: 0, y: 0, scale: 1 };
  const scale = Math.max(MIN_SCALE, Math.min(1, Math.max(1, width - 48) / (maxX - minX), Math.max(1, height - 64) / (maxY - minY)));
  return { scale, x: (width - (maxX - minX) * scale) / 2 - minX * scale, y: (height - (maxY - minY) * scale) / 2 - minY * scale };
}
