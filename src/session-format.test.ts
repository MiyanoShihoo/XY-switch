import { describe, expect, it } from "vitest";
import { formatSessionTime, sessionKey, sessionTitle, timestampMs } from "./lib/session-format";

describe("session metadata formatting", () => {
  it.each([undefined, null, 0, -1, NaN, Infinity, -Infinity])("handles an unrecorded timestamp %s", value => {
    expect(timestampMs(value)).toBe(0);
    expect(formatSessionTime(value)).toBe("未记录");
  });
  it("normalizes seconds and milliseconds to the same display", () => {
    expect(timestampMs(1793059200)).toBe(1793059200000);
    expect(timestampMs(1793059200000)).toBe(1793059200000);
    expect(formatSessionTime(1793059200)).toBe(formatSessionTime(1793059200000));
    expect(formatSessionTime(1793059200)).not.toBe("未记录");
  });
  it("does not show Invalid Date for out-of-range values", () => {
    expect(formatSessionTime(Number.MAX_VALUE)).toBe("未记录");
  });
  it("uses title, summary, then ID without treating null as text", () => {
    const session = { providerId: "codex", sessionId: "session-1" };
    expect(sessionTitle({ ...session, title: "  Title  ", summary: "Summary" })).toBe("Title");
    expect(sessionTitle({ ...session, title: "  ", summary: " Summary " })).toBe("Summary");
    expect(sessionTitle({ ...session, title: null, summary: null })).toBe("session-1");
  });
  it("does not collide across clients or joined delimiter characters", () => {
    expect(sessionKey({ providerId: "codex", sessionId: "same" })).not.toBe(sessionKey({ providerId: "claude", sessionId: "same" }));
    expect(sessionKey({ providerId: "a:b", sessionId: "c" })).not.toBe(sessionKey({ providerId: "a", sessionId: "b:c" }));
  });
});
