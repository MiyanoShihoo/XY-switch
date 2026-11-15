import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { DateRange, UsageTrendPoint } from "../../types/usage";
import { UsageTrend } from "./UsageTrend";

const selectedMetric = vi.hoisted(() => ({ value: "requests" }));
vi.mock("react", async importOriginal => {
  const react = await importOriginal<typeof import("react")>();
  return { ...react, useState: (initial: unknown) => react.useState(initial === "requests" ? selectedMetric.value : initial) };
});

function readings(count: number): UsageTrendPoint[] {
  return Array.from({ length: count }, (_, index) => {
    const timestamp = Date.UTC(2026, 10, 3) + index * 3_600_000;
    return { timestamp, date: new Date(timestamp).toISOString().slice(0, 10), label: String(index), requests: index + 1, inputTokens: index * 10, outputTokens: index * 2, cacheReadTokens: 0, cacheCreationTokens: 0, tokenCoverage: { input: index + 1, output: index + 1, cacheRead: index + 1, cacheCreation: index + 1 }, cost: index / 100, unknownCostRequests: 0 };
  });
}

function render(points: UsageTrendPoint[], metric = "requests", dateRange: DateRange = "7d") {
  selectedMetric.value = metric;
  return renderToStaticMarkup(createElement(UsageTrend, { points, dateRange }));
}

function region(html: string, tag: string, className: string) {
  const match = html.match(new RegExp(`<${tag}\\b([^>]*class="${className}"[^>]*)>([\\s\\S]*?)</${tag}>`));
  expect(match, `Missing ${className}`).not.toBeNull();
  return { attributes: match![1], body: match![2] };
}

function attribute(attributes: string, name: string) {
  return attributes.match(new RegExp(`\\b${name}="([^"]*)"`, "i"))?.[1];
}

function marks(body: string, tag: string) {
  return [...body.matchAll(new RegExp(`<${tag}\\b([^>]*)>([\\s\\S]*?)</${tag}>`, "g"))].map(match => ({ attributes: match[1], body: match[2], column: Number(attribute(match[1], "style")?.match(/grid-column:(\d+)/)?.[1]) }));
}

function tableRows(html: string) {
  const body = html.match(/<tbody>([\s\S]*?)<\/tbody>/)?.[1] ?? "";
  return marks(body, "tr");
}

afterEach(() => vi.unstubAllGlobals());

describe("usage trend data coordinates", () => {
  it.each(["zh-CN", "en-US"])("keeps 168 readings, sampled ticks and a known peak aligned in %s", language => {
    vi.stubGlobal("navigator", { language, languages: [language] });
    const points = readings(168);
    points[66].requests = 7;
    points[67].requests = 8000;
    points[68].requests = 9;
    const html = render(points);
    const plot = region(html, "ol", "usage-trend__plot");
    const axis = region(html, "div", "usage-trend__time-axis");
    expect(attribute(axis.attributes, "style")).toBe(attribute(plot.attributes, "style"));
    expect(attribute(plot.attributes, "style")).toContain("repeat(168, minmax(0, 1fr))");
    expect(attribute(plot.attributes, "style")).toContain("column-gap:0");
    const bars = marks(plot.body, "li"), ticks = marks(axis.body, "time");
    expect(bars).toHaveLength(168);
    expect(bars.map(bar => bar.column)).toEqual(Array.from({ length: 168 }, (_, index) => index + 1));
    expect(ticks.map(tick => tick.column)).toEqual([1, 34, 68, 101, 135, 168]);
    for (const tick of ticks) expect(attribute(tick.attributes, "datetime")).toBe(new Date(points[tick.column - 1].timestamp).toISOString());
    expect(bars[67].body).toContain("height:100%");
    expect(attribute(bars[67].attributes, "aria-label")).toMatch(/ · 8,000$/);
    expect(attribute(bars[66].attributes, "aria-label")).toMatch(/ · 7$/);
    expect(attribute(bars[68].attributes, "aria-label")).toMatch(/ · 9$/);
    const rows = tableRows(html);
    expect(rows).toHaveLength(168);
    expect(rows.map(row => attribute(row.body, "datetime"))).toEqual(points.map(point => new Date(point.timestamp).toISOString()));
    expect(rows[67].body).toContain(">8,000</td>");
  });

  it("uses all six data columns and the same non-dense spacing for six ticks", () => {
    const points = readings(6), html = render(points);
    const plot = region(html, "ol", "usage-trend__plot"), axis = region(html, "div", "usage-trend__time-axis");
    expect(attribute(axis.attributes, "style")).toBe(attribute(plot.attributes, "style"));
    expect(attribute(plot.attributes, "style")).toContain("repeat(6, minmax(0, 1fr))");
    expect(attribute(plot.attributes, "style")).toContain("column-gap:4px");
    const bars = marks(plot.body, "li"), ticks = marks(axis.body, "time");
    expect(ticks.map(tick => tick.column)).toEqual([1, 2, 3, 4, 5, 6]);
    expect(bars.map(bar => bar.column)).toEqual(ticks.map(tick => tick.column));
    expect(ticks.map(tick => attribute(tick.attributes, "datetime"))).toEqual(points.map(point => new Date(point.timestamp).toISOString()));
    expect(tableRows(html)).toHaveLength(6);
  });

  it.each([2, 3, 7, 12, 13, 25])("keeps both endpoints and unique source indexes for %i readings", count => {
    const points = readings(count), html = render(points, "requests", "30d");
    const ticks = marks(region(html, "div", "usage-trend__time-axis").body, "time");
    expect(ticks).toHaveLength(Math.min(6, count));
    expect(ticks[0].column).toBe(1);
    expect(ticks.at(-1)!.column).toBe(count);
    expect(new Set(ticks.map(tick => tick.column)).size).toBe(ticks.length);
    for (const tick of ticks) expect(attribute(tick.attributes, "datetime")).toBe(new Date(points[tick.column - 1].timestamp).toISOString());
    expect(tableRows(html)).toHaveLength(count);
  });

  it("keeps a single reading as a dated value and an original table row", () => {
    const points = readings(1), html = render(points);
    const single = region(html, "dl", "usage-trend__single");
    expect(attribute(single.body, "datetime")).toBe(new Date(points[0].timestamp).toISOString());
    expect(single.body).toContain("<dd>1</dd>");
    expect(html).not.toContain('class="usage-trend__chart"');
    expect(tableRows(html)).toHaveLength(1);
  });

  it("does not invent a trend or data table for an empty range", () => {
    const html = render([]);
    expect(html).toContain("暂无趋势数据");
    expect(html).not.toContain('class="usage-trend__chart"');
    expect(html).not.toContain("<table");
  });

  it("keeps known zero readings on the zero scale", () => {
    const html = render(readings(6).map(point => ({ ...point, requests: 0 })));
    const bars = marks(region(html, "ol", "usage-trend__plot").body, "li");
    expect(html).toContain('data-flat="true"');
    expect(bars).toHaveLength(6);
    for (const bar of bars) expect(bar.body).toContain("height:0%");
    expect(html).not.toContain("当前指标暂无已知数值");
  });
});

describe("usage trend unknown costs", () => {
  it("marks legacy costs as partial in chart labels and source rows without turning missing costs into zero", () => {
    const costs = [0, 0.125, null];
    const points = readings(costs.length).map((point, index) => ({ ...point, requests: 2, cost: costs[index] }));
    for (const point of points) delete point.unknownCostRequests;
    const html = render(points, "cost");
    const bars = marks(region(html, "ol", "usage-trend__plot").body, "li");
    const rows = tableRows(html);
    expect(bars).toHaveLength(costs.length);
    expect(rows).toHaveLength(costs.length);
    for (const [index, label] of ["$0.0000（已知部分）", "$0.1250（已知部分）", "未知"].entries()) {
      expect(attribute(bars[index].attributes, "aria-label")).toContain(" · " + label);
      expect(attribute(bars[index].attributes, "title")).toBe(attribute(bars[index].attributes, "aria-label"));
      expect(bars[index].body).toContain(">" + label + "</strong>");
      expect(rows[index].body).toContain(">" + label + "</td>");
    }
    expect(bars[0].body).toContain("height:0%");
    expect(bars[1].body).toContain("height:100%");
    expect(bars[2].body).not.toContain('class="usage-trend__bar"');
  });

  it.each([
    { requests: 2, cost: 0, label: "$0.0000（已知部分）" },
    { requests: 2, cost: 0.125, label: "$0.1250（已知部分）" },
    { requests: 2, cost: null, label: "未知" },
    { requests: 0, cost: 0, label: "$0.0000" },
  ])("keeps single legacy cost $cost with $requests requests consistent with the source row", ({ requests, cost, label }) => {
    const point = { ...readings(1)[0], requests, cost };
    delete point.unknownCostRequests;
    const html = render([point], "cost");
    expect(region(html, "dl", "usage-trend__single").body).toContain(">" + label + "</dd>");
    expect(tableRows(html)).toHaveLength(1);
    expect(tableRows(html)[0].body).toContain(">" + label + "</td>");
  });

  it("uses whole-bucket cost coverage for unknown and partially known zero costs", () => {
    const html = render(readings(3).map((point, index) => ({ ...point, requests: 2, cost: 0, unknownCostRequests: [2, 1, 0][index] })), "cost");
    const bars = marks(region(html, "ol", "usage-trend__plot").body, "li");
    expect(attribute(bars[0].attributes, "aria-label")).toMatch(/ · 未知$/);
    expect(bars[0].body).not.toContain('class="usage-trend__bar"');
    expect(attribute(bars[1].attributes, "aria-label")).toMatch(/ · \$0\.0000（已知部分）$/);
    expect(attribute(bars[2].attributes, "aria-label")).toMatch(/ · \$0\.0000$/);
    for (const bar of bars.slice(1)) expect(bar.body).toContain("height:0%");
    const rows = tableRows(html);
    expect(rows[0].body).toContain(">未知</td>");
    expect(rows[1].body).toContain(">$0.0000（已知部分）</td>");
    expect(rows[2].body).toContain(">$0.0000</td>");
  });

  it("preserves unknown, free and known costs in their own data slots and table rows", () => {
    const costs = [null, 0, 2, Number.NaN, Number.POSITIVE_INFINITY];
    const html = render(readings(costs.length).map((point, index) => ({ ...point, cost: costs[index] })), "cost");
    const bars = marks(region(html, "ol", "usage-trend__plot").body, "li");
    expect(bars.map(bar => bar.column)).toEqual([1, 2, 3, 4, 5]);
    for (const index of [0, 3, 4]) {
      expect(attribute(bars[index].attributes, "aria-label")).toMatch(/ · 未知$/);
      expect(bars[index].body).not.toContain('class="usage-trend__bar"');
    }
    expect(bars[1].body).toContain("height:0%");
    expect(bars[2].body).toContain("height:100%");
    const rows = tableRows(html);
    expect(rows).toHaveLength(costs.length);
    expect(rows[0].body).toContain(">未知</td>");
    expect(rows[1].body).toContain(">$0.0000</td>");
    expect(rows[2].body).toContain(">$2.0000</td>");
    expect(html).toContain("未知费用未计入，空值不是零");
  });

  it("shows an unavailable chart and retains every row when all costs are unknown", () => {
    const html = render(readings(168).map(point => ({ ...point, cost: null })), "cost");
    expect(html).toContain("当前指标暂无已知数值");
    expect(html).not.toContain('class="usage-trend__plot"');
    const rows = tableRows(html);
    expect(rows).toHaveLength(168);
    for (const row of rows) expect(row.body).toContain(">未知</td>");
    expect(html).not.toContain("$0.0000");
  });

  it("keeps a single unknown cost distinct from zero", () => {
    const html = render(readings(1).map(point => ({ ...point, cost: null })), "cost");
    expect(region(html, "dl", "usage-trend__single").body).toContain('class="usage-trend__unknown">未知</dd>');
    expect(tableRows(html)).toHaveLength(1);
    expect(html).not.toContain("$0.0000");
  });
});

describe("usage trend token coverage", () => {
  it("keeps missing, known zero and partial token readings in their original slots", () => {
    const points = readings(3).map((point, index) => ({
      ...point, requests: 2, inputTokens: index === 2 ? 11 : 0, outputTokens: index === 2 ? 2 : 0,
      tokenCoverage: { input: [0, 2, 1][index], output: [0, 2, 2][index], cacheRead: 0, cacheCreation: 0 },
    }));
    const html = render(points, "tokens");
    const bars = marks(region(html, "ol", "usage-trend__plot").body, "li");
    expect(bars.map(bar => bar.column)).toEqual([1, 2, 3]);
    expect(attribute(bars[0].attributes, "aria-label")).toMatch(/ · 未知$/);
    expect(bars[0].body).not.toContain('class="usage-trend__bar"');
    expect(attribute(bars[1].attributes, "aria-label")).toMatch(/ · 0$/);
    expect(bars[1].body).toContain("height:0%");
    expect(attribute(bars[2].attributes, "aria-label")).toMatch(/ · 13（已知部分）$/);
    expect(attribute(bars[2].attributes, "title")).toMatch(/ · 13（已知部分）$/);
    expect(bars[2].body).toContain(">13（已知部分）</strong>");
    expect(bars[2].body).toContain("height:100%");
    const rows = tableRows(html);
    expect(rows[0].body).toContain(">未知 / 未知</td>");
    expect(rows[1].body).toContain(">0 / 0</td>");
    expect(rows[2].body).toContain(">11（已知部分） / 2</td>");
    expect(html).toContain("未知 token 未计入；已知部分仅包含已记录数值。");
  });

  it("keeps a known partial zero and excludes counters without coverage", () => {
    const points = readings(2);
    points[0] = { ...points[0], inputTokens: 0, outputTokens: 99, tokenCoverage: { input: 1, output: 0, cacheRead: 0, cacheCreation: 0 } };
    points[1] = { ...points[1], tokenCoverage: undefined };
    const html = render(points, "tokens");
    const bars = marks(region(html, "ol", "usage-trend__plot").body, "li");
    expect(attribute(bars[0].attributes, "aria-label")).toMatch(/ · 0（已知部分）$/);
    expect(bars[0].body).toContain("height:0%");
    expect(attribute(bars[1].attributes, "aria-label")).toMatch(/ · 未知$/);
    expect(bars[1].body).not.toContain('class="usage-trend__bar"');
    const rows = tableRows(html);
    expect(rows[0].body).toContain(">0 / 未知</td>");
    expect(rows[1].body).toContain(">未知 / 未知</td>");
  });

  it.each([1, 168])("retains %i missing readings without inventing zero tokens", count => {
    const html = render(readings(count).map(point => ({ ...point, inputTokens: 0, outputTokens: 0, tokenCoverage: { input: 0, output: 0, cacheRead: 0, cacheCreation: 0 } })), "tokens");
    if (count === 1) expect(region(html, "dl", "usage-trend__single").body).toContain('class="usage-trend__unknown">未知</dd>');
    else expect(html).toContain("当前指标暂无已知数值");
    expect(html).not.toContain('class="usage-trend__plot"');
    const rows = tableRows(html);
    expect(rows).toHaveLength(count);
    for (const row of rows) expect(row.body).toContain(">未知 / 未知</td>");
  });
});
