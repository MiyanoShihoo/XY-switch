import { useLanguage, t } from "../../i18n/language";
import { useEffect, useRef, useState } from "react";
import { ArrowLeft, ArrowRight, Database, Info, KeyRound, RefreshCw, Route, Settings2, Shield } from "lucide-react";
import { Button } from "../../ui";
import type { ThemePreference } from "../../lib/theme";
import type { ProxyStatus } from "../../types/proxy";
import type { ProvidersController } from "../providers/useProviders";
import { GeneralSettings } from "./GeneralSettings";
import { RoutingSettings, type ProxyMutation } from "./RoutingSettings";
import { CredentialsSettings } from "./CredentialsSettings";
import { AppInformation, DataSettings } from "./DataSettings";
import { EncryptedSyncSettings } from "./EncryptedSyncSettings";
import { PrivacySettings } from "./PrivacySettings";
import { TaskNavigation } from "./TaskNavigation";
import "./settings.css";

interface SettingsPageProps {
  themePreference: ThemePreference;
  onThemeChange: (theme: ThemePreference) => void;
  providers: ProvidersController;
  proxyStatus: ProxyStatus | null;
  proxyBusy: boolean;
  runtimeError: string;
  mutateProxy: ProxyMutation;
  onRefreshRuntime: () => Promise<void>;
  onImported: () => Promise<void>;
}

export function SettingsPage(props: SettingsPageProps) {
  useLanguage();
  const [tab, setTab] = useState<string | null>(() => typeof window !== "undefined" && window.history.state?.settingsTask === "routing" ? "routing" : null);
  const triggerId = useRef("");
  const heading = useRef<HTMLHeadingElement | null>(null);
  useEffect(() => {
    if (window.history.state?.settingsTask) { const { settingsTask: _task, ...state } = window.history.state; window.history.replaceState(state, "", window.location.href); }
  }, []);
  useEffect(() => { if (tab) heading.current?.focus(); else if (triggerId.current) document.getElementById(triggerId.current)?.focus(); }, [tab]);
  const desktop = "__TAURI_INTERNALS__" in window;
  const categories = [
    { value: "general", label: t("通用"), description: t("调整外观、语言与应用行为"), effect: t("主题和语言立即保存；请求与日志分别在下次启动路由、重启应用后生效。"), state: t(props.themePreference === "system" ? "跟随系统" : props.themePreference === "dark" ? "深色" : "浅色"), icon: Settings2 },
    { value: "routing", label: t("本地路由"), description: t("检查运行、恢复接管或修改路由配置"), effect: t("运行状态以回读为准；接管与恢复会修改对应客户端配置。"), state: props.proxyStatus ? props.proxyStatus.running ? t("运行中") : t("已停止") : t("状态未知"), icon: Route },
    { value: "credentials", label: t("凭据"), description: t("处理缺失凭据或查询失败"), effect: t("这里只查询存在状态；进入对应供应商编辑器后再保存密钥变更。"), state: t("只读检查"), icon: KeyRound },
    { value: "privacy", label: t("隐私"), description: t("编辑保护策略并在本地预览"), effect: t("预览使用当前草稿；保存的策略在每次代理发送前读取。"), state: t("保存后生效"), icon: Shield },
    { value: "data", label: t("数据"), description: t("备份或恢复普通配置 JSON"), effect: t("文件不含凭据；确认导入后写入，失败时可能已经完成部分配置。"), state: t("确认后写入"), icon: Database },
    { value: "sync", label: t("加密同步"), description: t("审核加密交换冲突或恢复交换备份"), effect: t("选择对象、核对差异并完成确认后提交；退出清理口令和票据。"), state: t("逐项审核"), icon: RefreshCw },
    { value: "about", label: t("关于"), description: t("查看版本与项目信息"), effect: t("只读资料"), state: "XY Switch", icon: Info },
  ];
  const groups = [
    { label: t("本机偏好"), values: ["general", "privacy"] },
    { label: t("连接与凭据"), values: ["routing", "credentials"] },
    { label: t("配置与恢复"), values: ["data", "sync"] },
    { label: t("应用信息"), values: ["about"] },
  ];
  const selected = categories.find(category => category.value === tab);
  return <TaskNavigation>{({ leave, busy }) => <section className="workspace-page settings-page" aria-labelledby="settings-title">
    <header className="page-heading"><div className="page-heading__copy">
      {selected && <Button variant="quiet" icon={<ArrowLeft size={16} />} disabled={busy} onClick={() => leave(() => setTab(null))}>{t("返回设置任务")}</Button>}
      <h1 id="settings-title" ref={heading} tabIndex={-1}>{selected?.label ?? t("设置")}</h1><p className="page-description">{selected?.description ?? t("选择要完成的任务，进入后查看实际状态与生效方式。")}</p>
    </div></header>
    {!selected ? <div className="settings-task-index" aria-label={t("设置任务")}>
      {groups.map(group => <section className="settings-task-group" key={group.label}><h2>{group.label}</h2><div>
        {categories.filter(category => group.values.includes(category.value)).map(({ value, label, description, effect, state, icon: Icon }) => <button key={value} id={"settings-task-" + value} type="button" className="settings-task-entry" onClick={event => { triggerId.current = event.currentTarget.id; setTab(value); }}>
          <Icon size={19} aria-hidden="true" /><span className="settings-task-entry__copy"><strong>{label}<span>{description}</span></strong><small>{effect}</small></span><span className="settings-task-entry__state">{state}</span><ArrowRight size={16} aria-hidden="true" />
        </button>)}
      </div></section>)}
    </div> : <div className="settings-content" role="region" id={"settings-panel-" + tab} aria-label={selected.label}>
        <p className="settings-effect"><strong>{t("生效方式")}</strong>{selected.effect}</p>
        {!desktop && <p className="notice">{t("浏览器预览：可调整主题与检查界面，系统偏好、凭据和 CLI 配置为只读；实际写入需在桌面应用中进行。")}</p>}
        {tab === "general" && <GeneralSettings desktop={desktop} themePreference={props.themePreference} onThemeChange={props.onThemeChange} />}
        {tab === "routing" && <RoutingSettings desktop={desktop} status={props.proxyStatus} runtimeBusy={props.proxyBusy} runtimeError={props.runtimeError} mutateProxy={props.mutateProxy} onRefresh={props.onRefreshRuntime} />}
        {tab === "credentials" && <CredentialsSettings desktop={desktop} providers={props.providers} />}
        {tab === "privacy" && <PrivacySettings desktop={desktop} />}
        {tab === "data" && <DataSettings desktop={desktop} onImported={props.onImported} />}
        {tab === "sync" && <EncryptedSyncSettings proxyStopped={props.proxyStatus?.running === false} onImported={props.onImported} />}
        {tab === "about" && <div className="settings-sections"><AppInformation /></div>}
      </div>}
  </section>}</TaskNavigation>;
}
