import { useEffect, useRef, useState, type ReactNode } from "react";
import { ArrowLeft } from "lucide-react";
import { t, useLanguage } from "../../i18n/language";
import { Badge, Button, EmptyState, LoadingState } from "../../ui";
import type { MarketplaceCatalogEntry, MarketplacePageResult } from "../../types/marketplace";
import { pluginClientLabels } from "../plugins/clients";

interface CatalogProps {
  page: MarketplacePageResult<MarketplaceCatalogEntry>;
  selected: MarketplaceCatalogEntry | null;
  busy: boolean; loading: boolean; hasError: boolean; filtered: boolean; canReport: boolean;
  filters: ReactNode;
  onSelect: (entry: MarketplaceCatalogEntry) => void;
  onRefresh: () => void; onLoad: (offset: number) => void;
  onPreview: () => void; onVersions: () => void; onDownload: () => void; onReport: () => void;
}

function kindLabel(entry: MarketplaceCatalogEntry): string {
  return entry.kind === "mcp" ? "MCP" : t(entry.kind === "skill" ? "技能" : "模型映射");
}

export function MarketplaceCandidate({ entry, busy, canReport, onPreview, onVersions, onDownload, onReport }: {
  entry: MarketplaceCatalogEntry; busy: boolean; canReport: boolean;
  onPreview: () => void; onVersions: () => void; onDownload: () => void; onReport: () => void;
}) {
  useLanguage();
  return <div className="marketplace-candidate-content">
    <header className="marketplace-candidate-heading"><p className="marketplace-meta">{kindLabel(entry)} · {entry.version}</p>
      <h2 id="marketplace-candidate-title" tabIndex={-1}>{entry.name}</h2>
      <p className="marketplace-meta">{entry.authorDisplayName}</p></header>
    <p className="marketplace-candidate-purpose">{entry.description}</p>
    <section className="marketplace-candidate-section" aria-label={t("支持客户端")}><h3>{t("支持客户端")}</h3>
      <div className="marketplace-candidate-clients">{entry.clients.map(client => <Badge key={client}>{pluginClientLabels[client]}</Badge>)}</div></section>
    <section className="marketplace-candidate-section"><h3>{t("安装前审查")}</h3>
      <p>{t("当前显示公开目录中的资料。完整权限、权限变化与本机签名验证将在安装预览中读取。")}</p>
      <Button disabled={busy} onClick={onPreview}>{t("预览安装")}</Button></section>
    <section className="marketplace-candidate-section"><h3>{t("版本与来源")}</h3>
      <dl className="key-values"><dt>{t("插件标识")}</dt><dd><code>{entry.id}</code></dd>
        <dt>{t("签名密钥标识")}</dt><dd><code>{entry.signingKeyId}</code></dd>
        <dt>SHA-256</dt><dd><code>{entry.digest}</code></dd></dl>
      <div className="marketplace-row-actions"><Button variant="secondary" disabled={busy} onClick={onVersions}>{t("版本历史")}</Button>
        <Button variant="secondary" disabled={busy} onClick={onDownload}>{t("下载签名包")}</Button></div></section>
    <div className="marketplace-candidate-report"><Button variant="quiet" size="sm" disabled={busy || !canReport} onClick={onReport}>{t("举报")}</Button>
      {!canReport && <p className="marketplace-meta">{t("登录市场后可举报。")}</p>}</div>
  </div>;
}

export function MarketplaceCatalog({ page, selected, busy, loading, hasError, filtered, canReport, filters, onSelect, onRefresh, onLoad, onPreview, onVersions, onDownload, onReport }: CatalogProps) {
  useLanguage();
  const rootRef = useRef<HTMLDivElement>(null);
  const resultsHeading = useRef<HTMLHeadingElement>(null);
  const [detailOpen, setDetailOpen] = useState(false);
  const [split, setSplit] = useState(false);
  const returnPoint = useRef<{ trigger: HTMLElement; positions: { element: Element; top: number; left: number }[] } | null>(null);
  useEffect(() => {
    const workspace = rootRef.current?.closest(".workspace-page");
    if (!workspace || typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(entries => setSplit(entries[0].contentRect.width >= 960));
    observer.observe(workspace);
    return () => observer.disconnect();
  }, []);
  useEffect(() => {
    if (detailOpen && !split && selected) rootRef.current?.querySelector<HTMLElement>("#marketplace-candidate-title")?.focus();
  }, [detailOpen, split, selected?.id]);

  useEffect(() => {
    if (!detailOpen || selected) return;
    setDetailOpen(false);
    requestAnimationFrame(() => resultsHeading.current?.focus());
  }, [detailOpen, selected]);

  function select(entry: MarketplaceCatalogEntry, trigger: HTMLElement) {
    const positions: { element: Element; top: number; left: number }[] = [];
    for (let element: Element | null = trigger; element; element = element.parentElement) {
      positions.push({ element, top: element.scrollTop, left: element.scrollLeft });
    }
    returnPoint.current = { trigger, positions };
    onSelect(entry); setDetailOpen(true);
  }
  function back() {
    setDetailOpen(false);
    requestAnimationFrame(() => {
      const saved = returnPoint.current;
      const target = saved?.trigger.isConnected ? saved.trigger : resultsHeading.current;
      target?.focus({ preventScroll: true });
      saved?.positions.forEach(({ element, top, left }) => { element.scrollTop = top; element.scrollLeft = left; });
    });
  }

  return <div className="marketplace-browser" ref={rootRef} data-detail-open={detailOpen && !!selected}>
    <div className="marketplace-browser-filters">{filters}</div>
    <div className="marketplace-comparison">
      <section className="marketplace-results" aria-labelledby="marketplace-results-title">
        <div className="marketplace-section-heading"><div><h2 id="marketplace-results-title" tabIndex={-1} ref={resultsHeading}>{t("公开已发布插件")}</h2>
          <p className="marketplace-meta">{t("当前页 {count} 项", { count: page.items.length })}</p></div>
          <Button variant="quiet" size="sm" loading={busy} onClick={onRefresh}>{t("刷新目录")}</Button></div>
        <div className="marketplace-result-scroll">
          {loading ? <LoadingState label={t("正在读取公开目录…")} /> : !page.items.length ? !hasError && <EmptyState
            title={filtered ? t("没有符合条件的插件") : t("当前没有可公开下载的已发布插件。")}
            description={filtered ? t("调整搜索内容或筛选条件，然后应用筛选。") : undefined} />
            : <div className="marketplace-list" role="list">{page.items.map(entry => <article key={entry.id + "-" + entry.version} role="listitem" className="marketplace-result-row" data-selected={selected?.id === entry.id}>
              <button type="button" className="marketplace-result-select" disabled={busy} aria-pressed={selected?.id === entry.id}
                aria-label={t("查看插件 {name}", { name: entry.name })} onClick={event => select(entry, event.currentTarget)}>
                <span className="marketplace-result-identity"><strong>{entry.name}</strong><code>{entry.version}</code></span>
                <span className="marketplace-description">{entry.description}</span>
                <span className="marketplace-meta">{kindLabel(entry)} · {entry.authorDisplayName}</span>
                <span className="marketplace-meta">{entry.clients.map(client => pluginClientLabels[client]).join(" · ")}</span>
              </button></article>)}</div>}
        </div>
        {(page.offset > 0 || page.hasMore) && <nav className="marketplace-pagination" aria-label={t("市场分页")}>
          <Button variant="secondary" size="sm" disabled={busy || page.offset === 0} onClick={() => onLoad(Math.max(0, page.offset - page.limit))}>{t("上一页")}</Button>
          <span>{t("起始位置")} {page.offset + 1}</span>
          <Button variant="secondary" size="sm" disabled={busy || !page.hasMore} onClick={() => onLoad(page.offset + page.limit)}>{t("下一页")}</Button>
        </nav>}
      </section>
      <section className="marketplace-candidate" aria-label={t("候选插件详情")}>
        <Button className="marketplace-results-back" variant="quiet" size="sm" icon={<ArrowLeft size={16} />} onClick={back}>{t("返回插件结果")}</Button>
        {selected ? <MarketplaceCandidate entry={selected} busy={busy} canReport={canReport} onPreview={onPreview} onVersions={onVersions} onDownload={onDownload} onReport={onReport} />
          : <EmptyState title={t("选择一个候选插件")} description={t("在结果中选择插件，查看用途、客户端和版本资料。")} />}
      </section>
    </div>
  </div>;
}
