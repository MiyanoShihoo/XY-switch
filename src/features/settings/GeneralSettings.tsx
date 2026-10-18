import { useEffect, useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { Badge, Button, Field, Input, InlineError, LoadingState, Select, Toast, Toggle } from "../../ui";
import { loadAutostart, loadPreferences, logLevels, maxRequestTimeoutSecs, minRequestTimeoutSecs, saveAutostart, savePreferences, validatePreferences, type AppPreferences, type LogLevel } from "../../lib/preferences-service";
import { errorText } from "../../lib/error";
import { usePlatformCapabilities } from "../../lib/platform-service";
import type { ThemePreference } from "../../lib/theme";
import { usePageOperation } from "../shared/usePageOperation";

export function GeneralSettings({ desktop, themePreference, onThemeChange }: { desktop: boolean; themePreference: ThemePreference; onThemeChange: (theme: ThemePreference) => void }) {
  const client = useQueryClient();
  const platform = usePlatformCapabilities();
  const preferences = useQuery({ queryKey: ["preferences"], queryFn: loadPreferences, retry: false, refetchOnWindowFocus: false });
  const autostart = useQuery({ queryKey: ["autostart"], queryFn: loadAutostart, enabled: desktop && platform.data?.autostart === true, retry: false, refetchOnWindowFocus: false });
  const operation = usePageOperation();
  const [autostartUnknown, setAutostartUnknown] = useState(false);
  const changeAutostart = (enabled: boolean) => operation.run(async () => {
    try {
      const actual = await saveAutostart(enabled);
      client.setQueryData(["autostart"], actual); setAutostartUnknown(false);
      if (actual !== enabled) throw new Error("注册表回读状态与请求不一致，开关已显示实际状态。");
    } catch (cause) {
      try { client.setQueryData(["autostart"], await loadAutostart()); setAutostartUnknown(false); }
      catch { setAutostartUnknown(true); }
      throw cause;
    }
  }, "开机自启状态已更新");
  const knownAutostart = !autostartUnknown && !autostart.isError && autostart.data !== undefined;
  return <div className="settings-sections">
    {platform.error && <InlineError message={"平台能力读取失败：" + errorText(platform.error)} onRetry={() => void platform.refetch()} />}
    <section className="settings-section"><div className="settings-section__heading"><h2>外观与窗口</h2><p>主题立即生效，系统行为以实际保存状态为准。</p></div><div className="form-stack">
      <Field label="颜色主题"><Select value={themePreference} onChange={event => onThemeChange(event.target.value as ThemePreference)}><option value="system">跟随系统</option><option value="light">浅色</option><option value="dark">深色</option></Select></Field>
      <div className="form-switch"><div className="form-switch__copy"><strong>开机自启</strong><small>读取 Windows 注册表，不使用本地开关模拟状态。</small></div>{platform.data && !platform.data.autostart ? <Badge>当前平台不支持</Badge> : knownAutostart ? <Toggle aria-label="开机自启" checked={autostart.data!} onCheckedChange={value => void changeAutostart(value)} disabled={!desktop || !platform.data?.autostart || operation.busy || autostart.isFetching} /> : <Badge>{autostart.isPending ? "读取中" : "状态未知"}</Badge>}</div>
      {platform.data?.autostart && (autostart.error || autostartUnknown) && <InlineError message={autostartUnknown ? "写入后的实际自启状态无法读取。" : "自启状态读取失败：" + errorText(autostart.error)} onRetry={() => { void operation.run(async () => { const actual = await loadAutostart(); client.setQueryData(["autostart"], actual); setAutostartUnknown(false); }); }} />}
      <InlineError message={operation.error} />{operation.message && <Toast title={operation.message} tone="success" onDismiss={() => operation.setMessage("")} />}
      <p className="muted">界面语言：简体中文。当前未提供其他语言。</p>
    </div></section>
    <section className="settings-section"><div className="settings-section__heading"><h2>应用偏好</h2><p>偏好保存在本地数据库中，由后端实际读取。</p></div><div className="form-stack">
      {preferences.isPending && <LoadingState label="正在读取应用偏好…" />}
      {preferences.error && <InlineError message={"偏好读取失败：" + errorText(preferences.error)} onRetry={() => void preferences.refetch()} />}
      {preferences.data && <PreferencesForm traySupported={platform.data?.systemTray === true} value={preferences.data} disabled={!desktop || preferences.isError || preferences.isFetching} onSaved={value => client.setQueryData(["preferences"], value)} />}
    </div></section>
  </div>;
}

function PreferencesForm({ value, disabled, onSaved, traySupported }: { value: AppPreferences; disabled: boolean; onSaved: (value: AppPreferences) => void; traySupported: boolean }) {
  const [draft, setDraft] = useState(value);
  const [timeout, setTimeoutText] = useState(String(value.requestTimeoutSecs));
  const operation = usePageOperation();
  useEffect(() => { setDraft(value); setTimeoutText(String(value.requestTimeoutSecs)); }, [value]);
  const request = { ...draft, requestTimeoutSecs: Number(timeout) };
  const invalid = validatePreferences(request);
  const dirty = JSON.stringify(request) !== JSON.stringify(value);
  return <form className="form-stack" onSubmit={event => { event.preventDefault(); if (invalid) { operation.setError(invalid); return; } void operation.run(async () => onSaved(await savePreferences(request)), "偏好已保存。日志等级需重启应用；请求超时在下次启动路由时生效。"); }}>
    <fieldset disabled={disabled || operation.busy} className="settings-fieldset form-stack">
      <div className="form-switch"><div className="form-switch__copy"><strong>关闭窗口时最小化到托盘</strong><small>仅在系统托盘可用时生效；无托盘时正常关闭，不会隐藏失联。</small></div><Toggle aria-label="关闭窗口时最小化到托盘" disabled={!traySupported} checked={traySupported && draft.minimizeToTray} onCheckedChange={checked => setDraft(previous => ({ ...previous, minimizeToTray: checked }))} /></div>
      <div className="form-grid"><Field label="请求超时（秒）" description="15–600 秒，保存后在下次启动路由时生效。" error={dirty && invalid ? invalid : undefined}><Input type="number" min={minRequestTimeoutSecs} max={maxRequestTimeoutSecs} step={1} required value={timeout} onChange={event => setTimeoutText(event.target.value)} /></Field><Field label="日志等级" description="重启应用后生效。"><Select value={draft.logLevel} onChange={event => setDraft(previous => ({ ...previous, logLevel: event.target.value as LogLevel }))}>{logLevels.map(level => <option key={level} value={level}>{level.toUpperCase()}</option>)}</Select></Field></div>
      <div className="inline-actions"><Button type="submit" loading={operation.busy} disabled={!dirty || Boolean(invalid)}>保存偏好</Button><Button variant="secondary" disabled={!dirty} onClick={() => { setDraft(value); setTimeoutText(String(value.requestTimeoutSecs)); operation.setError(""); }}>撤销修改</Button></div>
    </fieldset><InlineError message={operation.error} />{operation.message && <Toast tone="success" title={operation.message} onDismiss={() => operation.setMessage("")} />}
  </form>;
}
