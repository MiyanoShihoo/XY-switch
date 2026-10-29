import { t } from "../../i18n/language";
import type { ModelPricingInput } from "../../types/usage";

export interface PricingDraft {
  id: string;
  input: string;
  output: string;
  cacheRead: string;
  cacheCreation: string;
}

export const emptyPricingDraft = (): PricingDraft => ({ id: "", input: "", output: "", cacheRead: "", cacheCreation: "" });

export function pricingDraft(value: ModelPricingInput): PricingDraft {
  return { id: value.id, input: String(value.inputPerMillion), output: String(value.outputPerMillion), cacheRead: value.cacheReadPerMillion == null ? "" : String(value.cacheReadPerMillion), cacheCreation: value.cacheCreationPerMillion == null ? "" : String(value.cacheCreationPerMillion) };
}

export function parsePricingDraft(value: PricingDraft): ModelPricingInput {
  const id = value.id.trim();
  if (!id || new TextEncoder().encode(id).length > 256 || /[\s\u0000-\u001f\u007f-\u009f]/u.test(id)) {
    throw new Error(t("模型定价 ID 必须为 1–256 字节，且不能含空白或控制字符"));
  }
  function rate(text: string, optional: true): number | null;
  function rate(text: string, optional?: false): number;
  function rate(text: string, optional = false): number | null {
    const trimmed = text.trim();
    if (optional && !trimmed) return null;
    const number = Number(trimmed);
    if (!/^(?:\d+(?:\.\d*)?|\.\d+)(?:[eE][+-]?\d+)?$/.test(trimmed) || !Number.isFinite(number) || number < 0 || number > 1_000_000) {
      throw new Error(t("模型价格必须是 0–1000000 之间的有限数值（USD / 百万 token）"));
    }
    return number;
  }
  return { id, inputPerMillion: rate(value.input), outputPerMillion: rate(value.output), cacheReadPerMillion: rate(value.cacheRead, true), cacheCreationPerMillion: rate(value.cacheCreation, true) };
}
