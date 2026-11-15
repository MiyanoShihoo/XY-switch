import { describe, expect, it } from "vitest";
import { ancestorsOf, fitViewport, indexTopology, inViewport, layoutTopology, NODE_HEIGHT, NODE_WIDTH, relatedNodes } from "./topology";
import { edge, node } from "./test-fixtures";

describe("observation topology", () => {
  it("preserves nested folds and projects cross-branch links onto the visible ancestor", () => {
    const index = indexTopology([node("root"), node("branch", "root"), node("leaf", "branch"), node("peer")], [edge("leaf", "peer", "handoff")]);
    const folded = new Set(["root", "branch"]);
    const outer = layoutTopology(index, folded);
    expect(new Set(outer.visible)).toEqual(new Set(["root", "peer"]));
    expect(outer.hiddenCounts.get("root")).toBe(2);
    expect(outer.edges).toEqual([expect.objectContaining({ source: "root", target: "peer", kind: "handoff" })]);
    folded.delete("root");
    const inner = layoutTopology(index, folded);
    expect(new Set(inner.visible)).toEqual(new Set(["root", "branch", "peer"]));
    expect(inner.representative.get("leaf")).toBe("branch");
    expect(inner.hiddenCounts.get("branch")).toBe(1);
    expect(ancestorsOf(index, "leaf")).toEqual(new Set(["branch", "root"]));
    expect(relatedNodes(index, "branch").downstream).toEqual(new Set(["leaf", "peer"]));
    folded.clear();
    expect(layoutTopology(index, folded).visible).toHaveLength(4);
  });

  it("keeps every node reachable with cycles, self-links and missing parents", () => {
    const agents = [node("a", "c"), node("b", "a"), node("c", "b"), node("self", "self"), node("orphan", "missing")];
    const index = indexTopology(agents, [edge("missing", "a")]);
    expect(index.cycles).toEqual(new Set(["a", "b", "c", "self"]));
    expect(index.parents.has("a")).toBe(false);
    expect(index.roots).toEqual(["a", "orphan", "self"]);
    expect(relatedNodes(index, "a").downstream).toEqual(new Set(["b", "c"]));
    const layout = layoutTopology(index, new Set());
    expect(new Set(layout.visible)).toEqual(new Set(agents.map(agent => agent.id)));
    expect(layout.positions).toEqual(layoutTopology(indexTopology([...agents].reverse(), []), new Set()).positions);
  });

  it.each(["chain", "wide"] as const)("fits and culls a 1000-node %s without recursive traversal", shape => {
    const agents = Array.from({ length: 1000 }, (_, position) => node(String(position), position ? String(shape === "chain" ? position - 1 : 0) : null, { createdAt: position }));
    const index = indexTopology(agents, []), layout = layoutTopology(index, new Set());
    expect(layout.visible).toHaveLength(1000);
    expect(new Set([...layout.positions.values()].map(point => `${point.x},${point.y}`)).size).toBe(1000);
    const fit = fitViewport(layout.positions.values(), 390, 500);
    for (const point of layout.positions.values()) {
      expect(point.x * fit.scale + fit.x).toBeGreaterThanOrEqual(0);
      expect(point.y * fit.scale + fit.y).toBeGreaterThanOrEqual(0);
      expect((point.x + NODE_WIDTH) * fit.scale + fit.x).toBeLessThanOrEqual(390);
      expect((point.y + NODE_HEIGHT) * fit.scale + fit.y).toBeLessThanOrEqual(500);
    }
    const visible = [...layout.positions.values()].filter(point => inViewport(point, { x: 0, y: 0, scale: 1 }, 820, 500));
    expect(visible.length).toBeGreaterThan(0);
    expect(visible.length).toBeLessThan(12);
    const folded = layoutTopology(index, new Set(["0"]));
    expect(folded.visible).toEqual(["0"]);
    expect(folded.hiddenCounts.get("0")).toBe(999);
  });
});
