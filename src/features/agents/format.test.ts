import { describe, expect, it } from "vitest";
import { calendarDate, elapsed, isValidTimestamp, isoTimestamp, localDateInput, parseDateInput, timestamp } from "./format";

describe("observation date boundaries", () => {
  it("renders missing and non-renderable source dates without throwing", () => {
    for (const value of [null, undefined, NaN, Infinity, -Infinity, 9_007_199_254_740_991, 8_640_000_000_000_001, -8_640_000_000_000_001]) {
      expect(isValidTimestamp(value)).toBe(false);
      expect(timestamp(value, "zh-CN")).toBe("—");
      expect(calendarDate(value, "en-US")).toBe("—");
      expect(isoTimestamp(value)).toBeUndefined();
      expect(localDateInput(value ?? null)).toBe("");
    }
    expect(elapsed(9_007_199_254_740_991, null, Date.now())).toBe("—");
    expect(elapsed(1000, Infinity, 2000)).toBe("—");
  });

  it("handles TimeClip and timezone-offset boundaries while preserving ordinary dates", () => {
    for (const value of [-8_640_000_000_000_000, 8_640_000_000_000_000]) {
      expect(isValidTimestamp(value)).toBe(true);
      expect(() => timestamp(value, "zh-CN")).not.toThrow();
      expect(() => isoTimestamp(value)).not.toThrow();
      expect(localDateInput(value)).toBe("");
    }
    const normal = 1_794_307_716_000;
    expect(isoTimestamp(normal)).toBe(new Date(normal).toISOString());
    expect(parseDateInput(localDateInput(normal))).toBe(normal);
    expect(parseDateInput("not-a-date")).toBeUndefined();
    expect(elapsed(1000, 62000, 70000)).toBe("1m 1s");
  });
});
