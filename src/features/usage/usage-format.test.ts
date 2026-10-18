import { describe, expect, it } from "vitest";
import { formatTokens, formatUsd, lastUsagePage, normalizeUsageFilter } from "./usage-format";

describe("usage values and pagination", () => {
  it("distinguishes a real zero cost from missing/invalid cost", () => {
    for (const value of [0, "0", "0.0000"]) expect(formatUsd(value)).toBe("$0.0000");
    for (const value of [null, undefined, "", "  ", "unknown", NaN, Infinity]) expect(formatUsd(value)).toBe("—");
    expect(formatUsd("0.123456")).toBe("$0.1235");
  });
  it("preserves token zero and rejects non-finite counters", () => {
    expect(formatTokens(0)).toBe("0");
    expect(formatTokens(12000)).not.toBe("—");
    expect(formatTokens(NaN)).toBe("—");
  });
  it.each([[0,20,0],[1,20,0],[20,20,0],[21,20,1],[40,20,1],[41,20,2],[-1,20,0],[3,0,2],[NaN,20,0]])("last page for %s/%s is %s", (total,size,page) => {
    expect(lastUsagePage(total,size)).toBe(page);
  });
  it("normalizes draft strings without deleting numeric zero or changing yesterday", () => {
    const input = { dateRange: "1d" as const, providerId: "  p1 ", model: "  ", status: undefined, startTimeMs: 0 };
    expect(normalizeUsageFilter(input)).toEqual({ dateRange: "1d", providerId: "p1", startTimeMs: 0 });
    expect(input.providerId).toBe("  p1 ");
  });
});
