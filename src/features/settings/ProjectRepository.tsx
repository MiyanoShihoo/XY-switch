import { useLanguage, t } from "../../i18n/language";
import type { MouseEvent } from "react";
import { Copy, ExternalLink } from "lucide-react";
import { Button, InlineError, Toast } from "../../ui";
import { copyProjectRepository, openProjectRepository, PROJECT_REPOSITORY_URL, repositoryUrlFromMetadata } from "../../lib/project-info";
import { usePageOperation } from "../shared/usePageOperation";

export function ProjectRepository({ repositoryUrl = PROJECT_REPOSITORY_URL }: { repositoryUrl?: string | null }) {
  useLanguage();
  const operation = usePageOperation();
  const url = repositoryUrlFromMetadata(repositoryUrl);
  if (!url) return <div className="settings-repository"><strong>{t("项目仓库")}</strong><p className="muted">{t("仓库地址尚未配置。")}</p></div>;

  const open = (event: MouseEvent<HTMLAnchorElement>) => {
    if (event.button !== 0 && event.button !== 1) return;
    if (typeof window !== "undefined" && "__TAURI_INTERNALS__" in window) {
      event.preventDefault();
      void operation.run(() => openProjectRepository(url));
    }
  };

  return <div className="settings-repository">
    <strong>{t("项目仓库")}</strong>
    <p className="muted">{t("查看项目源码和更新记录。")}</p>
    <div className="settings-repository__row">
      <a className="settings-repository__link" href={url} target="_blank" rel="noopener noreferrer" onClick={open} onAuxClick={open}>
        <span>{url}</span><ExternalLink size={15} aria-hidden="true" />
      </a>
      <Button variant="quiet" size="sm" icon={<Copy size={14} />} disabled={operation.busy}
        onClick={() => void operation.run(() => copyProjectRepository(url), t("仓库地址已复制"))}>{t("复制地址")}</Button>
    </div>
    <InlineError message={operation.error} />
    {operation.message && <Toast title={operation.message} tone="success" onDismiss={() => operation.setMessage("")} />}
  </div>;
}
