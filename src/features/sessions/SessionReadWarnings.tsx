import { t } from "../../i18n/language";

export function SessionReadWarnings({ warnings }: { warnings?: string[] }) {
  if (!warnings?.length) return null;
  return <details className="workspace-disclosure session-read-warnings">
    <summary>{t("部分会话内容未能读取")} ({warnings.length})</summary>
    <div className="workspace-disclosure__body" role="status">
      <ul>{warnings.map((warning, index) => <li key={index}>{t(warning)}</li>)}</ul>
    </div>
  </details>;
}
