import { t, useLanguage } from "../../i18n/language";
import { Button, Field, Input, Modal, Select } from "../../ui";
import type { ClientSettings } from "../../types/clients";

interface ClientPathSettingsDialogProps {
  draft: ClientSettings;
  disabled: boolean;
  busy: boolean;
  pendingLogin: boolean;
  pendingConfiguration: boolean;
  settingsState?: "loading" | "error";
  onChange: (settings: ClientSettings) => void;
  onClose: () => void;
  onSave: () => void;
}

export function ClientPathSettingsDialog({ draft, disabled, busy, pendingLogin, pendingConfiguration, settingsState, onChange, onClose, onSave }: ClientPathSettingsDialogProps) {
  useLanguage();
  const locked = disabled || busy || pendingLogin || pendingConfiguration || Boolean(settingsState);
  return <div className="client-dialog"><Modal open size="lg" title={t("客户端路径")} busy={busy} onClose={onClose} footer={<>
    <Button variant="secondary" disabled={busy} onClick={onClose}>{t("取消")}</Button>
    <Button disabled={locked} onClick={onSave}>{t("保存路径并在重启后应用")}</Button>
  </>}>
    <div className="client-settings">
      <p className="client-detail-note">{t("可选择 OpenCode 的实际 JSON/JSONC 配置、Claude Desktop 已保存的第三方配置，以及 Codex 和 OpenCode 原生可执行文件。留空使用系统默认位置。")}</p>
      {pendingLogin && <p className="workspace-route-notice" role="status">{t("原生登录期间锁定客户端路径，请先完成或取消登录。")}</p>}
      {pendingConfiguration && <p className="workspace-route-notice" role="status">{t("客户端路径已保存，请重启 XY Switch 后继续操作。")}</p>}
      {settingsState && <p className="workspace-route-notice" role="status">{settingsState === "loading" ? t("正在读取已保存的路径，读取完成后可编辑。") : t("路径读取失败。请关闭窗口、刷新状态后重试，避免覆盖已有路径。")}</p>}
      <section className="client-settings__group" aria-labelledby="client-paths-opencode"><h3 id="client-paths-opencode">OpenCode</h3><div className="client-settings__fields">
        <Field className="client-settings__wide" label={t("OpenCode 配置文件")} htmlFor="client-opencode-config"><Input id="client-opencode-config" value={draft.opencodeConfig ?? ""} disabled={locked} onChange={event => onChange({ ...draft, opencodeConfig: event.target.value.trim() || null })} /></Field>
        <Field label={t("OpenCode 配置版本")} htmlFor="client-opencode-schema"><Select id="client-opencode-schema" value={draft.opencodeSchema ?? ""} disabled={locked} onChange={event => onChange({ ...draft, opencodeSchema: (event.target.value || null) as ClientSettings["opencodeSchema"] })}><option value="">{t("从现有配置识别")}</option><option value="v1">V1</option><option value="v2">V2</option></Select></Field>
        <Field label={t("OpenCode 原生可执行文件")} htmlFor="client-opencode-binary"><Input id="client-opencode-binary" value={draft.opencodeBinary ?? ""} disabled={locked} onChange={event => onChange({ ...draft, opencodeBinary: event.target.value.trim() || null })} /></Field>
      </div></section>
      <section className="client-settings__group" aria-labelledby="client-paths-codex"><h3 id="client-paths-codex">Codex</h3>
        <Field label={t("Codex CLI 可执行文件")} htmlFor="client-codex-binary"><Input id="client-codex-binary" value={draft.codexBinary ?? ""} disabled={locked} onChange={event => onChange({ ...draft, codexBinary: event.target.value.trim() || null })} /></Field>
      </section>
      <section className="client-settings__group" aria-labelledby="client-paths-claude"><h3 id="client-paths-claude">Claude Desktop</h3>
        <Field label={t("Claude Desktop 第三方配置文件")} htmlFor="client-claude-profile"><Input id="client-claude-profile" value={draft.claudeDesktopProfile ?? ""} disabled={locked} onChange={event => onChange({ ...draft, claudeDesktopProfile: event.target.value.trim() || null })} /></Field>
      </section>
    </div>
  </Modal></div>;
}
