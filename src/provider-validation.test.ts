import { describe, expect, it } from "vitest";
import { MAX_API_KEY_BYTES, selectProviderAfterDelete, validateApiKey, validateProvider } from "./lib/provider-validation";
import type { Provider } from "./types/provider";
import { createProviderAdvanced } from "./lib/providers";

const existing: Provider = { id: "demo", name: "Demo", group: "Codex", endpoint: "https://example.com/v1", model: "demo-model", enabled: true, icon: "D", advanced: createProviderAdvanced() };

describe("供应商表单校验", () => {
  it("接受有效的新供应商", () => {
    expect(validateProvider({ id: "new-provider", name: "New", endpoint: "https://api.example.com/v1", model: "model-1" }, [existing], null)).toBe("");
  });

  it("拒绝缺失字段、非法 ID 和非法端点", () => {
    expect(validateProvider({ id: "", name: "", endpoint: "", model: "" }, [], null)).toContain("完整填写");
    expect(validateProvider({ id: "bad id", name: "Bad", endpoint: "https://example.com", model: "m" }, [], null)).toContain("供应商 ID");
    expect(validateProvider({ id: "bad-url", name: "Bad", endpoint: "ftp://example.com", model: "m" }, [], null)).toContain("HTTP");
  });

  it("新增时拒绝重复 ID，但编辑原记录时允许", () => {
    const form = { id: "DEMO", name: "Updated", endpoint: "https://example.com/v2", model: "model-2" };
    expect(validateProvider(form, [existing], null)).toContain("已存在");
    expect(validateProvider(form, [existing], "demo")).toBe("");
  });
});

describe("删除供应商后的选择回退", () => {
  const disabled: Provider = { ...existing, id: "disabled", enabled: false };
  const enabled: Provider = { ...existing, id: "enabled", enabled: true };

  it("删除非当前供应商时保留当前选择", () => {
    expect(selectProviderAfterDelete("demo", "other", [existing])).toBe("demo");
  });

  it("优先选择剩余的已启用供应商", () => {
    expect(selectProviderAfterDelete("demo", "demo", [disabled, enabled])).toBe("enabled");
  });

  it("没有已启用供应商时选择第一项，没有剩余项时清空", () => {
    expect(selectProviderAfterDelete("demo", "demo", [disabled])).toBe("disabled");
    expect(selectProviderAfterDelete("demo", "demo", [])).toBe("");
  });
});

describe("API Key 校验", () => {
  it("允许空值表示不修改现有凭据", () => {
    expect(validateApiKey("")).toBe("");
  });

  it("拒绝纯空格和超过 Windows 凭据限制的值", () => {
    expect(validateApiKey("   ")).toContain("空格");
    expect(validateApiKey("x".repeat(MAX_API_KEY_BYTES + 1))).toContain("2560");
  });
});
