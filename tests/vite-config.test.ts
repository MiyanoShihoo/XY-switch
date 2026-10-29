import { describe, expect, it } from "vitest";
import config from "../vite.config";

describe("local renderer development server", () => {
  it("does not watch generated native builds or locked browser-profile files", () => {
    expect(config.server?.watch?.ignored).toEqual(expect.arrayContaining([
      "**/artifacts/**", "**/src-tauri/target/**",
    ]));
  });
  it("stays on the explicit loopback address and fails on a busy port", () => {
    expect(config.server?.host).toBe("127.0.0.1");
    expect(config.server?.strictPort).toBe(true);
  });
});
