import { renderToStaticMarkup as render } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { AppShell } from "./AppShell";
import { SideNav } from "./SideNav";
import type { ProxyStatus } from "../types/proxy";
const status: ProxyStatus = { running: true, address: "http://127.0.0.1:15721", lastError: null, settings: { enabled: true, showInHome: true, host: "127.0.0.1", port: 15721, routeTargets: ["codex"] } };
const shell = (proxyStatus: ProxyStatus | null, proxyBusy = false, runtimeStale = false) => render(<AppShell activeView="providers" onNavigate={() => {}} providerCount={4} currentProviderName="Local Relay" proxyStatus={proxyStatus} proxyBusy={proxyBusy} runtimeStale={runtimeStale} onToggleProxy={() => {}} theme="light" onToggleTheme={() => {}}><div>content</div></AppShell>);
describe("workspace application shell", () => {
  it("exposes navigation, current configuration and real runtime state separately", () => {
    const html = shell(status);
    expect(html).toContain('class="xy-shell-root"'); expect(html).toContain('aria-label="主导航"'); expect(html).toContain('aria-current="page"');
    expect(html).toContain("当前配置"); expect(html).toContain("Local Relay"); expect(html).toContain("运行中"); expect(html).toContain("15721"); expect(html).toContain("跳到内容"); expect(html).toContain("content");
    expect(html).not.toContain("已连接"); expect(html).not.toContain("ROUTE WEAVE");
  });
  it("does not claim readiness before runtime state is loaded", () => {
    const html = shell(null); expect(html).toContain("状态未读取"); expect(html).toContain('disabled=""'); expect(html).not.toContain("运行中");
  });
  it("announces runtime failures and prevents duplicate toggles", () => {
    const html = shell({ ...status, running: false, lastError: "端口被占用" }, true); expect(html).toContain("运行异常"); expect(html).toContain("端口被占用"); expect(html).toContain('aria-busy="true"');
  });
  it("keeps the confirmed configuration while stale runtime controls stay disabled", () => {
    const html = shell(status, false, true);
    expect(html).toContain("Local Relay"); expect(html).toContain("运行状态待刷新");
    expect(html).toContain('data-state="warning"'); expect(html).toContain('disabled=""');
    expect(html).not.toContain("运行中"); expect(html).not.toContain("已停止");
  });
  it("keeps daily tasks in the header and all destinations in an on-demand directory", () => {
    const html = shell(status);
    const drawer = render(<SideNav activeView="providers" onNavigate={() => {}} providerCount={4} />);
    expect(html).not.toContain("xy-shell-activity-nav");
    expect(html).not.toContain("xy-shell-section-nav");
    expect(html).toContain('class="xy-shell-primary-nav"');
    expect(html).not.toContain('class="xy-shell-nav"');
    expect(html).toContain('aria-label="展开导航"');
    for (const view of ["providers", "clients", "sessions", "usage", "mcp", "prompts", "skills", "plugins", "marketplace", "agents"]) expect(html).toContain('data-view="' + view + '"');
    expect(drawer.match(/class="xy-shell-nav-item"/g)).toHaveLength(14);
    for (const view of ["overview", "providers", "clients", "accounts", "diagnostics", "plugins", "marketplace", "mcp", "prompts", "skills", "sessions", "usage", "settings", "governance"]) expect(drawer).toContain(`data-view="${view}"`);
    expect(html).toContain('<details class="xy-shell-runtime"');
    expect(html).not.toContain('open=""');
  });
});
