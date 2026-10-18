import { invoke } from "@tauri-apps/api/core";
import type { Prompt } from "../types/prompt";

const previewPrompts: Prompt[] = [
  {
    id: "code-review",
    name: "代码审查",
    content: "# 代码审查\n\n请检查安全性、边界条件和可维护性。",
    description: "适用于常规代码审查",
    enabled: true,
  },
];

function isDesktopRuntime(): boolean {
  return "__TAURI_INTERNALS__" in window;
}

export async function loadPrompts(): Promise<Prompt[]> {
  if (!isDesktopRuntime()) return previewPrompts;
  return invoke<Prompt[]>("list_prompts");
}

export async function savePrompt(prompt: Prompt): Promise<void> {
  if (!isDesktopRuntime()) throw new Error("浏览器预览模式不能修改提示词");
  await invoke("save_prompt", { prompt });
}

export async function deletePrompt(id: string): Promise<void> {
  if (!isDesktopRuntime()) throw new Error("浏览器预览模式不能删除提示词");
  await invoke("delete_prompt", { id });
}

export async function setPromptEnabled(id: string, enabled: boolean): Promise<void> {
  if (!isDesktopRuntime()) throw new Error("浏览器预览模式不能切换提示词");
  await invoke("set_prompt_enabled", { id, enabled });
}
