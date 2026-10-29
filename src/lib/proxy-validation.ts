import { t } from "../i18n/language";
import { routeTargets, type ProxySettings } from "../types/proxy";

export function validateProxySettings(settings: ProxySettings): string {
  const host = settings.host.trim();
  const loopbackIpv4 = /^127(?:\.\d{1,3}){3}$/.test(host) && host
    .split(".")
    .every((part) => Number(part) >= 0 && Number(part) <= 255);
  if (!loopbackIpv4 && host !== "::1") {
    return t("监听地址必须是回环地址，例如 127.0.0.1 或 ::1。");
  }
  if (!Number.isInteger(settings.port) || settings.port < 1024 || settings.port > 65535) {
    return t("监听端口必须在 1024 到 65535 之间。");
  }
  if (settings.routeTargets.length === 0) {
    return t("请至少选择一个协议路由目标。");
  }
  if (settings.routeTargets.some((target) => !routeTargets.includes(target))) {
    return t("协议路由目标包含不受支持的客户端。");
  }
  return "";
}
