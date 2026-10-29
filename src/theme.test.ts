import { describe, expect, it } from "vitest";
import { nextExplicitTheme, readThemePreference, resolveTheme } from "./lib/theme";

describe("主题偏好", () => {
  it("只接受显式亮色或暗色偏好，未保存和无效值均视为系统模式", () => {
    expect(readThemePreference(null)).toBe("system");
    expect(readThemePreference("system")).toBe("system");
    expect(readThemePreference("unknown")).toBe("system");
    expect(readThemePreference("light")).toBe("light");
    expect(readThemePreference("dark")).toBe("dark");
  });

  it("系统模式随系统设置解析，显式偏好不受系统设置影响", () => {
    expect(resolveTheme("system", true)).toBe("dark");
    expect(resolveTheme("system", false)).toBe("light");
    expect(resolveTheme("light", true)).toBe("light");
    expect(resolveTheme("dark", false)).toBe("dark");
  });

  it("主题按钮会切换到相反的显式偏好", () => {
    expect(nextExplicitTheme("light")).toBe("dark");
    expect(nextExplicitTheme("dark")).toBe("light");
  });
});
