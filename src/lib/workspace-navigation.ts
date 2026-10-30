import { useCallback, useEffect, useState } from "react";

export type AppView = "providers" | "provider-detail" | "diagnostics" | "mcp" | "prompts" | "skills" | "sessions" | "usage" | "governance" | "settings";
export interface WorkspaceRoute { view: AppView; providerId?: string; }
const pages = new Set<AppView>(["providers", "diagnostics", "mcp", "prompts", "skills", "sessions", "usage", "governance", "settings"]);

export function parseWorkspaceRoute(hash: string): WorkspaceRoute | null {
  // The initial unfragmented URL is the providers home entry in browser history.
  if (hash === "" || hash === "#") return { view: "providers" };
  const parts = hash.replace(/^#/, "").split("/");
  try {
    if (parts.length === 3 && parts[0] === "providers" && parts[2] === "configuration" && parts[1]) {
      return { view: "provider-detail", providerId: decodeURIComponent(parts[1]) };
    }
    if (parts.length === 2 && parts[0] === "diagnostics" && parts[1]) {
      return { view: "diagnostics", providerId: decodeURIComponent(parts[1]) };
    }
    if (parts.length === 1 && pages.has(parts[0] as AppView)) return { view: parts[0] as AppView };
  } catch { /* Ignore malformed external fragments without changing the current workspace. */ }
  return null;
}

export function workspaceRouteHash(route: WorkspaceRoute): string {
  if (route.view === "provider-detail") return route.providerId ? "#providers/" + encodeURIComponent(route.providerId) + "/configuration" : "#providers";
  if (route.view === "diagnostics" && route.providerId) return "#diagnostics/" + encodeURIComponent(route.providerId);
  return "#" + route.view;
}

export function useWorkspaceNavigation() {
  const [route, setRoute] = useState<WorkspaceRoute>(() => (typeof window !== "undefined" && parseWorkspaceRoute(window.location.hash)) || { view: "providers" });
  useEffect(() => {
    const update = () => { const next = parseWorkspaceRoute(window.location.hash); if (next) setRoute(next); };
    window.addEventListener("hashchange", update);
    window.addEventListener("popstate", update);
    return () => { window.removeEventListener("hashchange", update); window.removeEventListener("popstate", update); };
  }, []);
  const navigate = useCallback((view: AppView, providerId?: string) => {
    const next: WorkspaceRoute = view === "provider-detail" && !providerId ? { view: "providers" } : { view, ...((view === "provider-detail" || view === "diagnostics") && providerId ? { providerId } : {}) };
    const hash = workspaceRouteHash(next);
    if (window.location.hash !== hash) window.history.pushState(null, "", hash);
    setRoute(next);
  }, []);
  return { route, navigate };
}
