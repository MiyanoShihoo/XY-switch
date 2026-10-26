import { t } from "../i18n/language";
import type { SkillImportResult } from "../types/skill";
import { errorText } from "./error";

export function skillImportMessage(results: SkillImportResult[]): string {
  const added = results.reduce((sum, result) => sum + result.added, 0);
  const repeated = results.reduce((sum, result) => sum + result.alreadyImported, 0);
  return t("已新增 {added} 个技能引用；已跳过 {repeated} 个已导入项；源文件未修改", { added, repeated });
}

/** Process every dropped folder; one invalid folder must not discard the rest. */
export async function importSkillFolders(paths: string[], importFolder: (path: string) => Promise<SkillImportResult>) {
  const results: SkillImportResult[] = [];
  const warnings: string[] = [];
  for (const path of [...new Set(paths)]) {
    try {
      const result = await importFolder(path);
      results.push(result);
      warnings.push(...result.warnings);
    } catch (error) {
      warnings.push(path + "：" + errorText(error));
    }
  }
  if (!results.length) throw new Error(warnings.join("；") || t("没有可导入的文件夹"));
  return { results, warnings };
}
