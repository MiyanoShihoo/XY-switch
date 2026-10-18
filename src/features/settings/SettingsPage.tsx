import { useState } from "react";
import { Tabs } from "../../ui";
import type { ThemePreference } from "../../lib/theme";
import type { ProxyStatus } from "../../types/proxy";
import type { ProvidersController } from "../providers/useProviders";
import { GeneralSettings } from "./GeneralSettings";
import { RoutingSettings, type ProxyMutation } from "./RoutingSettings";
import { CredentialsSettings } from "./CredentialsSettings";
import { AppInformation, DataSettings } from "./DataSettings";
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
  const [tab, setTab] = useState("general");
  const desktop = "__TAURI_INTERNALS__" in window;
  return <section className="workspace-page settings-page" aria-labelledby="settings-title">
    <header className="page-heading"><div className="page-heading__copy"><h1 id="settings-title">设置</h1><p className="page-description">管理外观、系统行为、路由与本地数据。</p></div></header>
    <Tabs id="settings" label="设置分类" value={tab} onChange={setTab} items={[{ value: "general", label: "通用" }, { value: "routing", label: "本地路由" }, { value: "credentials", label: "凭据" }, { value: "data", label: "数据" }, { value: "about", label: "关于" }]} />
    {!desktop && <p className="notice">浏览器预览：可调整主题与检查界面，系统偏好、凭据和 CLI 配置为只读；实际写入需在桌面应用中进行。</p>}
    <div role="tabpanel" id={"settings-panel-" + tab} aria-labelledby={"settings-tab-" + tab}>
      {tab === "general" && <GeneralSettings desktop={desktop} themePreference={props.themePreference} onThemeChange={props.onThemeChange} />}
      {tab === "routing" && <RoutingSettings desktop={desktop} status={props.proxyStatus} runtimeBusy={props.proxyBusy} runtimeError={props.runtimeError} mutateProxy={props.mutateProxy} onRefresh={props.onRefreshRuntime} />}
      {tab === "credentials" && <CredentialsSettings desktop={desktop} providers={props.providers} />}
      {tab === "data" && <DataSettings desktop={desktop} onImported={props.onImported} />}
      {tab === "about" && <div className="settings-sections"><AppInformation /></div>}
    </div>
  </section>;
}
