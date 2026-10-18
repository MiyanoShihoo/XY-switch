import { describe, expect, it } from "vitest";
import { formatSuccessRate, healthDisplay } from "./lib/health-format";
import type { Provider, ProviderHealth } from "./types/provider";
import { createProviderAdvanced } from "./lib/providers";

const provider: Provider = {
  id: "demo",
  name: "Demo",
  group: "Codex",
  endpoint: "https://example.com/v1",
  model: "demo",
  enabled: true,
  icon: "/demo.png",
  advanced: createProviderAdvanced(),
};

const health: ProviderHealth = {
  providerId: "demo",
  status: "healthy",
  latencyMs: 125,
  successRate: 100,
  consecutiveFailures: 0,
  circuitOpenUntil: null,
  lastError: null,
  lastCheckedAt: 1,
};

describe("供应商健康状态显示", () => {
  it("区分禁用、未测速和健康状态", () => {
    expect(healthDisplay({ ...provider, enabled: false }, health).label).toBe("已禁用");
    expect(healthDisplay(provider).label).toBe("未测速");
    expect(healthDisplay(provider, health).label).toBe("健康");
  });

  it("优先显示凭据缺失和有效熔断", () => {
    expect(healthDisplay(provider, { ...health, status: "unhealthy", lastError: "api_key_missing" }).label).toBe("未配置密钥");
    expect(healthDisplay(provider, { ...health, status: "circuit_open", circuitOpenUntil: 2_000 }, 1_000).label).toBe("熔断中");
  });

  it("熔断到期后显示等待恢复", () => {
    expect(healthDisplay(provider, { ...health, status: "circuit_open", circuitOpenUntil: 500 }, 1_000).label).toBe("等待恢复");
  });
});

describe("请求成功率格式化", () => {
  it("无请求时不制造百分比，有请求时保留一位小数", () => {
    expect(formatSuccessRate(0, 0)).toBe("暂无数据");
    expect(formatSuccessRate(2, 3)).toBe("66.7%");
  });
});
