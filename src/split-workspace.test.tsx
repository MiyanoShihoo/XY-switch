import { describe, expect, it } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";
import { DEFAULT_INSPECTOR_WIDTH, MIN_INSPECTOR_WIDTH, MIN_LIST_WIDTH, SPLIT_HANDLE_WIDTH, inspectorWidthForKey, parseInspectorWidth, splitWorkspaceLayout } from "./lib/split-workspace";
import { ResizableWorkspace } from "./ui/ResizableWorkspace";

describe("split workspace bounds and preference", () => {
  it("reserves both panels and a real pointer target", () => {
    const r = splitWorkspaceLayout(960, 340);
    expect(r.inspectorWidth).toBe(340);
    expect(r.listWidth).toBe(608);
    expect(r.handleWidth).toBe(SPLIT_HANDLE_WIDTH);
    expect(r.minimum).toBe(MIN_INSPECTOR_WIDTH);
    expect(r.maximum).toBe(628);
  });
  it("stops at either edge even when a pointer leaves the window", () => {
    const left = splitWorkspaceLayout(960, 10000);
    expect(left.listWidth).toBe(MIN_LIST_WIDTH);
    expect(left.inspectorWidth).toBe(628);
    expect(splitWorkspaceLayout(960, -10000).inspectorWidth).toBe(MIN_INSPECTOR_WIDTH);
    expect(splitWorkspaceLayout(960, 0).inspectorWidth).toBe(MIN_INSPECTOR_WIDTH);
  });
  it("temporarily clamps on shrink without destroying the saved wide preference", () => {
    const preference = 620;
    expect(splitWorkspaceLayout(782, preference).inspectorWidth).toBe(450);
    expect(splitWorkspaceLayout(1220, preference).inspectorWidth).toBe(preference);
  });
  it.each([0, 6, 12, 100, 400, 619, 620, 782.25, 960, 3840])("never exceeds a %s px container", width => {
    for (const preference of [-5000, 1, 288, 340, 10000]) {
      const r = splitWorkspaceLayout(width, preference);
      expect(r.listWidth).toBeGreaterThanOrEqual(0);
      expect(r.inspectorWidth).toBeGreaterThanOrEqual(0);
      expect(r.listWidth + r.inspectorWidth + r.handleWidth).toBeCloseTo(width);
      expect(r.inspectorWidth).toBeGreaterThanOrEqual(r.minimum);
      expect(r.inspectorWidth).toBeLessThanOrEqual(r.maximum);
    }
  });
  it("handles unmeasured or invalid dimensions without NaN CSS", () => {
    for (const width of [NaN, Infinity, -50]) {
      const r = splitWorkspaceLayout(width, 340);
      expect(r.available).toBe(0);
      expect(r.inspectorWidth).toBe(0);
    }
    expect(splitWorkspaceLayout(960, NaN).inspectorWidth).toBe(DEFAULT_INSPECTOR_WIDTH);
  });
  it("reads a finite stored width and rejects missing or corrupt local storage", () => {
    expect(parseInspectorWidth("476.5")).toBe(476.5);
    for (const raw of [null, "", "  ", "NaN", "Infinity", "-40", "0", "{}", "undefined"]) {
      expect(parseInspectorWidth(raw)).toBe(DEFAULT_INSPECTOR_WIDTH);
    }
  });
});

describe("splitter keyboard controls", () => {
  it("moves in the visual direction and supports accelerated steps", () => {
    expect(inspectorWidthForKey("ArrowLeft", 400, 288, 628)).toBe(416);
    expect(inspectorWidthForKey("ArrowRight", 400, 288, 628)).toBe(384);
    expect(inspectorWidthForKey("ArrowLeft", 400, 288, 628, true)).toBe(448);
    expect(inspectorWidthForKey("ArrowRight", 400, 288, 628, true)).toBe(352);
  });
  it("clamps arrow keys and supports edge/reset shortcuts", () => {
    expect(inspectorWidthForKey("ArrowLeft", 624, 288, 628)).toBe(628);
    expect(inspectorWidthForKey("ArrowRight", 290, 288, 628)).toBe(288);
    expect(inspectorWidthForKey("Home", 400, 288, 628)).toBe(628);
    expect(inspectorWidthForKey("End", 400, 288, 628)).toBe(288);
    expect(inspectorWidthForKey("Enter", 400, 288, 628)).toBe(DEFAULT_INSPECTOR_WIDTH);
    expect(inspectorWidthForKey("Tab", 400, 288, 628)).toBeNull();
  });
});

describe("resizable workspace presentation", () => {
  const render = (compact = false) => renderToStaticMarkup(<ResizableWorkspace storageKey="qa-layout" compact={compact}
    primaryLabel="供应商列表" secondaryLabel="配置检查" primary={<p>列表内容</p>} secondary={<p>仅一份检查内容</p>} />);
  it("labels a vertical separator, its controlled panel and keyboard help", () => {
    const html = render();
    expect(html).toContain('role="separator"');
    expect(html).toContain('aria-orientation="vertical"');
    expect(html).toContain('aria-label="调整供应商列表与配置检查宽度"');
    const controlled = html.match(/aria-controls="([^"]+)"/)![1];
    expect(html).toContain('id="' + controlled + '"');
    expect(html).toContain("Escape 取消本次拖动");
    expect(html).toContain("宽度自动保存");
  });
  it("makes the list scrollable by keyboard without consuming row drag controls", () => {
    expect(render()).toContain('role="region" aria-label="供应商列表" tabindex="0"');
  });
  it("does not mount a hidden second inspector in compact/drawer mode", () => {
    const html = render(true);
    expect(html).toContain('data-compact="true"');
    expect(html).toContain("列表内容");
    expect(html).not.toContain("仅一份检查内容");
    expect(html).not.toContain('role="separator"');
  });
});
