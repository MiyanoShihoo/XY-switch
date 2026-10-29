import { describe, expect, it } from "vitest";
import { emptyPricingDraft, parsePricingDraft, pricingDraft } from "./pricing-validation";

const valid = () => ({ ...emptyPricingDraft(), id: "qa/model", input: "2.5", output: "10" });
describe("pricing form validation", () => {
  it("preserves unknown cache prices and explicit free prices", () => {
    expect(parsePricingDraft({ ...valid(), cacheRead: "0" })).toEqual({ id: "qa/model", inputPerMillion: 2.5, outputPerMillion: 10, cacheReadPerMillion: 0, cacheCreationPerMillion: null });
  });
  it("accepts decimal and scientific token prices without binary rounding in the draft", () => {
    const result = parsePricingDraft({ ...valid(), id: " qa/model ", input: ".005", output: "1e-3", cacheRead: " 1.5E+2 ", cacheCreation: "1000000" });
    expect(result).toEqual({ id: "qa/model", inputPerMillion: .005, outputPerMillion: .001, cacheReadPerMillion: 150, cacheCreationPerMillion: 1_000_000 });
    expect(parsePricingDraft(pricingDraft(result))).toEqual(result);
  });
  it.each(["", " ", "-1", "NaN", "Infinity", "0x10", "1,000", "1_000", "1e999", "1000000.01", "1.2.3"])("rejects invalid required rate %j", input => {
    expect(() => parsePricingDraft({ ...valid(), input })).toThrow("有限数值");
    expect(() => parsePricingDraft({ ...valid(), output: input })).toThrow("有限数值");
  });
  it.each(["-0.5", "Infinity", "0x10", "1000001"])("validates optional cache rates %j", rate => {
    expect(() => parsePricingDraft({ ...valid(), cacheRead: rate })).toThrow("有限数值");
    expect(() => parsePricingDraft({ ...valid(), cacheCreation: rate })).toThrow("有限数值");
  });
  it.each(["", " ", "qa model", "qa\nmodel", "qa\u0000model", "qa\u0085model", "a".repeat(257), "模".repeat(86)])("rejects invalid model ID %j", id => {
    expect(() => parsePricingDraft({ ...valid(), id })).toThrow("1–256 字节");
  });
  it("enforces the ID byte limit, not the UTF-16 length", () => {
    expect(parsePricingDraft({ ...valid(), id: "模".repeat(85) + "x" }).id).toHaveLength(86);
    expect(parsePricingDraft({ ...valid(), id: "a".repeat(256) }).id).toHaveLength(256);
  });
  it("does not share mutable draft state", () => {
    const first = emptyPricingDraft(); first.id = "changed";
    expect(emptyPricingDraft().id).toBe("");
  });
});
