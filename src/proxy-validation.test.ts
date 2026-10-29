import { describe, expect, it } from "vitest";
import { validateProxySettings } from "./lib/proxy-validation";
import type { ProxySettings } from "./types/proxy";

const valid: ProxySettings = {
  enabled: false,
  showInHome: false,
  host: "127.0.0.1",
  port: 15721,
  routeTargets: ["claude", "codex", "gemini", "grok", "opencode"],
};

describe("本地路由设置校验", () => {
  it("接受 IPv4 和 IPv6 回环地址", () => {
    expect(validateProxySettings(valid)).toBe("");
    expect(validateProxySettings({ ...valid, host: "::1" })).toBe("");
  });

  it("拒绝非回环地址", () => {
    expect(validateProxySettings({ ...valid, host: "0.0.0.0" })).toContain("回环");
    expect(validateProxySettings({ ...valid, host: "192.168.1.2" })).toContain("回环");
  });

  it("拒绝系统保留端口和越界端口", () => {
    expect(validateProxySettings({ ...valid, port: 80 })).toContain("1024");
    expect(validateProxySettings({ ...valid, port: 65536 })).toContain("65535");
  });

  it("要求至少选择一个受支持的协议目标", () => {
    expect(validateProxySettings({ ...valid, routeTargets: [] })).toContain("至少选择一个");
    expect(validateProxySettings({ ...valid, routeTargets: ["claude"] })).toBe("");
  });
});
