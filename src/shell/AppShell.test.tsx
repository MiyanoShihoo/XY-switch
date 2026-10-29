import { renderToStaticMarkup as render } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { AppShell } from "./AppShell";
import type { ProxyStatus } from "../types/proxy";
const status: ProxyStatus = { running: true, address: "http://127.0.0.1:15721", lastError: null, settings: { enabled: true, showInHome: true, host: "127.0.0.1", port: 15721, routeTargets: ["codex"] } };
const shell = (proxyStatus: ProxyStatus | null, proxyBusy = false) => render(<AppShell activeView="providers" onNavigate={() => {}} providerCount={4} currentProviderName="Local Relay" proxyStatus={proxyStatus} proxyBusy={proxyBusy} onToggleProxy={() => {}} theme="light" onToggleTheme={() => {}}><div>content</div></AppShell>);
describe("aligned application shell", () => {
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
});
