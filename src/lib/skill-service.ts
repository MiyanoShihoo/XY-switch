import { t } from "../i18n/language";
import { invoke } from "@tauri-apps/api/core";
import type { InstalledSkill, DiscoverableSkill, SkillUpdateInfo, SkillImportSettings, SkillCatalog, SkillImportResult } from "../types/skill";

// 浏览器预览（vite dev 直开网页）里没有 Tauri 运行时，读接口返回空数据，
// 写接口给一句人话，别抛 "__TAURI_INTERNALS__ is not defined"。
const IS_PREVIEW = typeof window === "undefined" || !("__TAURI_INTERNALS__" in window);

function previewUnavailable(): never {
  throw new Error(t("当前为浏览器预览模式，技能操作需要在桌面端运行"));
}

export async function listInstalledSkills(): Promise<InstalledSkill[]> {
  if (IS_PREVIEW) return [];
  return invoke<InstalledSkill[]>("get_installed_skills");
}

export async function installSkill(skill: DiscoverableSkill): Promise<InstalledSkill> {
  if (IS_PREVIEW) previewUnavailable();
  return invoke<InstalledSkill>("install_skill_unified", { skill });
}

export async function uninstallSkill(id: string): Promise<void> {
  if (IS_PREVIEW) previewUnavailable();
  return invoke("uninstall_skill_unified", { id });
}

export async function toggleSkillApp(id: string, app: string, enabled: boolean): Promise<void> {
  if (IS_PREVIEW) previewUnavailable();
  return invoke("toggle_skill_app", { id, app, enabled });
}

export async function discoverSkills(): Promise<DiscoverableSkill[]> {
  if (IS_PREVIEW) return [];
  return invoke<DiscoverableSkill[]>("discover_available_skills");
}

export async function checkSkillUpdates(): Promise<SkillUpdateInfo[]> {
  if (IS_PREVIEW) return [];
  return invoke<SkillUpdateInfo[]>("check_skill_updates");
}

export async function updateSkill(id: string): Promise<InstalledSkill> {
  if (IS_PREVIEW) previewUnavailable();
  return invoke<InstalledSkill>("update_skill", { id });
}

export async function importSkillFolder(path: string): Promise<SkillImportResult> {
  if (IS_PREVIEW) previewUnavailable();
  return invoke("import_skill_folder", { path });
}

/** 选择单个技能或技能集合，递归导入引用并返回计数；取消时返回 null。 */
export async function pickAndImportSkillFolder(): Promise<SkillImportResult | null> {
  if (IS_PREVIEW) previewUnavailable();
  return invoke<SkillImportResult | null>("pick_and_import_skill_folder");
}

// 注意：这里曾经导出过 listSkillRepos / addSkillRepo / removeSkillRepo，
// 对应的 list_skill_repos / add_skill_repo / remove_skill_repo 三个命令
// 从来没有注册进 generate_handler!，调用只会得到
// "Command xxx not found"。没有界面用到它们，已直接删除；
// 等仓库管理真正落地时，请先在 src-tauri/src/lib.rs 里注册命令再加回来。

export async function loadSkillImportSettings(): Promise<SkillImportSettings> {
  if (IS_PREVIEW) return { codex: false, deepseekHarness: false, claude: false, gemini: false, agents: false };
  return invoke("get_skill_import_settings");
}
export async function saveSkillImportSettings(settings: SkillImportSettings): Promise<void> {
  if (IS_PREVIEW) previewUnavailable();
  return invoke("save_skill_import_settings", { settings });
}
export async function getSkillCatalog(): Promise<SkillCatalog> {
  if (IS_PREVIEW) return { skills: [], warnings: [], sources: [], isolated: false };
  return invoke("get_skill_catalog");
}
