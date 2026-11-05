import type { PluginPermissions } from "../../types/plugins";
import { t, useLanguage } from "../../i18n/language";

export function PermissionsSummary({ permissions }: { permissions: PluginPermissions }) {
  useLanguage();
  return <dl className="key-values" aria-label={t("插件权限")}>
    <dt>{t("网络站点")}</dt><dd>{permissions.network.length ? permissions.network.join(" · ") : t("无")}</dd>
    <dt>{t("读取文本")}</dt><dd>{permissions.readPaths.length ? permissions.readPaths.join(" · ") : t("无")}</dd>
    <dt>{t("写文件")}</dt><dd>{permissions.writePaths.length ? permissions.writePaths.join(" · ") + t("（当前不可运行）") : t("无")}</dd>
    <dt>{t("运行进程")}</dt><dd>{permissions.process ? t("申请进程访问（当前缺少系统隔离，拒绝运行）") : t("无")}</dd>
    <dt>{t("访问凭据")}</dt><dd>{permissions.credentials.length ? permissions.credentials.join(" · ") + t("（当前缺少系统隔离，拒绝运行）") : t("无")}</dd>
  </dl>;
}
