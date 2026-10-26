import { t } from "../i18n/language";
export type PromptDraft = {
  name: string;
  content: string;
  description?: string;
};

export type PromptValidationErrors = Partial<Record<keyof PromptDraft, string>>;

export const PROMPT_NAME_MAX_LENGTH = 100;
export const PROMPT_DESCRIPTION_MAX_LENGTH = 500;
export const PROMPT_CONTENT_MAX_LENGTH = 200_000;

export function normalizePromptDraft(draft: PromptDraft): PromptDraft {
  return {
    name: draft.name.trim(),
    content: draft.content.trim(),
    description: draft.description?.trim() || undefined,
  };
}

export function validatePromptDraft(draft: PromptDraft): PromptValidationErrors {
  const normalized = normalizePromptDraft(draft);
  const errors: PromptValidationErrors = {};

  if (!normalized.name) errors.name = t("请输入提示词名称");
  else if (normalized.name.length > PROMPT_NAME_MAX_LENGTH) {
    errors.name = t("提示词名称不能超过 {value0} 个字符", { value0: PROMPT_NAME_MAX_LENGTH });
  }

  if (!normalized.content) errors.content = t("请输入提示词内容");
  else if (normalized.content.length > PROMPT_CONTENT_MAX_LENGTH) {
    errors.content = t("提示词内容不能超过 {value0} 个字符", { value0: PROMPT_CONTENT_MAX_LENGTH });
  }

  if ((normalized.description?.length ?? 0) > PROMPT_DESCRIPTION_MAX_LENGTH) {
    errors.description = t("描述不能超过 {value0} 个字符", { value0: PROMPT_DESCRIPTION_MAX_LENGTH });
  }

  return errors;
}

export function isPromptDraftValid(draft: PromptDraft): boolean {
  return Object.keys(validatePromptDraft(draft)).length === 0;
}
