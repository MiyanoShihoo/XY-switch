import { describe, expect, it } from "vitest";
import {
  PROMPT_CONTENT_MAX_LENGTH,
  PROMPT_DESCRIPTION_MAX_LENGTH,
  PROMPT_NAME_MAX_LENGTH,
  isPromptDraftValid,
  normalizePromptDraft,
  validatePromptDraft,
} from "./lib/prompt-validation";

describe("prompt validation", () => {
  it("normalizes surrounding whitespace and an empty description", () => {
    expect(normalizePromptDraft({ name: "  Review  ", content: "  Check this  ", description: "   " })).toEqual({
      name: "Review",
      content: "Check this",
      description: undefined,
    });
  });

  it("requires a name and content after normalization", () => {
    expect(validatePromptDraft({ name: "  ", content: "\n", description: "" })).toEqual({
      name: "请输入提示词名称",
      content: "请输入提示词内容",
    });
  });

  it("accepts a complete prompt", () => {
    expect(isPromptDraftValid({ name: "代码审查", content: "请检查安全性和边界条件。", description: "团队模板" })).toBe(true);
  });

  it("enforces all persisted field length limits", () => {
    const errors = validatePromptDraft({
      name: "n".repeat(PROMPT_NAME_MAX_LENGTH + 1),
      content: "c".repeat(PROMPT_CONTENT_MAX_LENGTH + 1),
      description: "d".repeat(PROMPT_DESCRIPTION_MAX_LENGTH + 1),
    });
    expect(errors.name).toContain(String(PROMPT_NAME_MAX_LENGTH));
    expect(errors.content).toContain(String(PROMPT_CONTENT_MAX_LENGTH));
    expect(errors.description).toContain(String(PROMPT_DESCRIPTION_MAX_LENGTH));
  });
});
