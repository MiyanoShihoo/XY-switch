import { describe, expect, it } from "vitest";
import { parseWorkspaceRoute, workspaceRouteHash, type WorkspaceRoute } from "./lib/workspace-navigation";

describe("workspace navigation", () => {
  it.each(["", "#", "#providers"])("resolves the home history entry %j", hash => {
    expect(parseWorkspaceRoute(hash)).toEqual({ view: "providers" });
  });
  it.each(["providers", "diagnostics", "mcp", "prompts", "skills", "sessions", "usage", "governance", "settings"] as const)("round-trips the %s page", view => {
    const route = { view };
    expect(parseWorkspaceRoute(workspaceRouteHash(route))).toEqual(route);
  });
  it.each(["relay", "a/b #?%", "中文配置 空格", "embedded/configuration"])("round-trips encoded provider ID %j", providerId => {
    for (const view of ["provider-detail", "diagnostics"] as const) {
      const route: WorkspaceRoute = { view, providerId };
      expect(parseWorkspaceRoute(workspaceRouteHash(route))).toEqual(route);
    }
  });
  it("returns from details to the original empty-fragment home entry", () => {
    const history = ["", workspaceRouteHash({ view: "provider-detail", providerId: "relay" }), "#diagnostics/relay"];
    expect(parseWorkspaceRoute(history[2])).toEqual({ view: "diagnostics", providerId: "relay" });
    expect(parseWorkspaceRoute(history[1])).toEqual({ view: "provider-detail", providerId: "relay" });
    expect(parseWorkspaceRoute(history[0])).toEqual({ view: "providers" });
  });
  it.each(["#workspace-content", "#unknown", "#Providers", "#providers//configuration", "#providers/relay/other", "#diagnostics/", "#providers/%/configuration", "#diagnostics/%E0%A4%A"])("ignores external or malformed fragment %j", hash => {
    expect(parseWorkspaceRoute(hash)).toBeNull();
  });
  it("does not create a detail URL without an ID", () => {
    expect(workspaceRouteHash({ view: "provider-detail" })).toBe("#providers");
    expect(workspaceRouteHash({ view: "diagnostics" })).toBe("#diagnostics");
  });
});
