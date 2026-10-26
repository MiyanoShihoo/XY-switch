import { useRef, useState } from "react";
import { Field, InlineError, Select } from "../../ui";
import { retryLanguageSynchronization, setLanguagePreference, t, useLanguage, useLanguagePreference, useLanguageSyncError, type LanguagePreference } from "../../i18n/language";

export function LanguageSettings() {
  useLanguage();
  const preference = useLanguagePreference();
  const syncError = useLanguageSyncError();
  const lock = useRef(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const change = async (next: LanguagePreference) => {
    if (lock.current) return;
    lock.current = true; setBusy(true); setError("");
    try { await setLanguagePreference(next); }
    catch (cause) { setError(t("语言设置保存失败：") + String(cause)); }
    finally { lock.current = false; setBusy(false); }
  };
  return <div className="form-stack">
    <Field label={t("界面语言")} description={t("立即切换中英文界面并保存选择；不会修改供应商名称、提示词正文或原始日志。")}>
      <Select aria-label={t("界面语言")} value={preference} disabled={busy} onChange={event => void change(event.target.value as LanguagePreference)}>
        <option value="system">{t("跟随系统")}</option><option value="zh-CN">简体中文</option><option value="en">English</option>
      </Select>
    </Field>
    <InlineError message={error} />
    {syncError && <InlineError message={t(syncError === "read" ? "语言偏好读取失败；当前使用本地选择，可重试。" : "后台保存语言设置失败；界面语言已保留，请重试。")} onRetry={() => void retryLanguageSynchronization()} />}
  </div>;
}
