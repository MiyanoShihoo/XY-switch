import { describe, expect, it } from "vitest";
import { createProviderAdvanced } from "./lib/providers";
import { parseBodyOverrides, parseHeaderOverrides, validateProviderAdvanced } from "./lib/provider-advanced";

describe("供应商高级配置", () => {
  it("解析并规范化安全的请求覆盖", () => {
    expect(parseHeaderOverrides('{"X-Provider":"xy-switch"}')).toEqual({ "x-provider": "xy-switch" });
    expect(parseBodyOverrides('{"temperature":0.2}')).toEqual({ temperature: 0.2 });
  });

  it("拒绝认证头和凭据字段", () => {
    expect(() => parseHeaderOverrides('{"Authorization":"Bearer secret"}')).toThrow("不能覆盖");
    expect(() => parseBodyOverrides('{"api_key":"secret"}')).toThrow("不能包含");
    expect(() => parseBodyOverrides('{"stream":false}')).toThrow("model 或 stream");
  });

  it("校验模型映射、User-Agent 和 Codex 配置", () => {
    const config = createProviderAdvanced();
    config.modelMappings = [
      { model: "model-a", displayName: "A", contextWindow: null },
      { model: "MODEL-A", displayName: "A2", contextWindow: null },
    ];
    expect(validateProviderAdvanced(config)).toContain("重复");
    config.modelMappings.pop();
    config.customUserAgent = "bad\nagent";
    expect(validateProviderAdvanced(config)).toContain("控制字符");
    config.customUserAgent = "";
    config.codex.configToml = 'api_key = "must-not-persist"';
    expect(validateProviderAdvanced(config)).toContain("不能保存");
  });
});

describe("模型窗口与压缩阈值边界", () => {
  it.each([0, -1, 1.5, NaN, Infinity, Number.MAX_SAFE_INTEGER + 1])("拒绝无效上下文窗口 %s", (contextWindow) => {
    const config = createProviderAdvanced();
    config.modelMappings = [{ model: "model-a", displayName: "A", contextWindow }];
    expect(validateProviderAdvanced(config)).toContain("上下文窗口必须是正整数");
  });
  it.each([null, 1, 128000])("接受默认或正整数窗口 %s", (contextWindow) => {
    const config = createProviderAdvanced();
    config.modelMappings = [{ model: "model-a", displayName: "A", contextWindow }];
    expect(validateProviderAdvanced(config)).toBe("");
  });
  it.each([NaN, 9999, 10000.5, 10000001])("拒绝无效压缩阈值 %s", (compactionThreshold) => {
    const config = createProviderAdvanced();
    config.codex.context1m = true;
    config.codex.compactionThreshold = compactionThreshold;
    expect(validateProviderAdvanced(config)).toContain("压缩阈值");
  });
});
