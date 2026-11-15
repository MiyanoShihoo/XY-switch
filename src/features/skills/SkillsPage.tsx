import { translateMessage, t, useLanguage } from "../../i18n/language";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { ArrowLeft, FolderInput, RefreshCw, Search, Settings2, Trash2, X } from "lucide-react";
import { getSkillCatalog, loadSkillImportSettings, saveSkillImportSettings, uninstallSkill, importSkillFolder, pickAndImportSkillFolder } from "../../lib/skill-service";
import { errorText } from "../../lib/error";
import { importSkillFolders, skillImportMessage } from "../../lib/skill-import";
import type { InstalledSkill, SkillImportSettings, SkillCatalog } from "../../types/skill";
import { ConfirmActionDialog } from "../../components/ConfirmActionDialog";
import { Badge, Button, EmptyState, InlineError, Input, LoadingState, Modal, Select, Toast, Toggle } from "../../ui";
import { usePageOperation } from "../shared/usePageOperation";
import "./skills.css";

const sources: Array<{ id: keyof SkillImportSettings; label: string; location: string }> = [
  { id: "codex", label: "Codex", location: ".codex/skills" },
  { id: "deepseekHarness", label: "DeepSeek Harness", location: ".dsh/skills" },
  { id: "claude", label: "Claude Code", location: ".claude/skills" },
  { id: "gemini", label: "Gemini CLI", location: ".gemini/skills" },
  { id: "agents", label: "共享 Agent Skills", location: ".agents/skills" },
];
const sourceLabel = (source: string) => t(sources.find(item => item.id === source)?.label ?? "手动导入");

export function SkillSources({ settings, catalog, busy, scanning, onChange }: {
  settings?: SkillImportSettings; catalog?: SkillCatalog; busy: boolean; scanning: boolean;
  onChange: (id: keyof SkillImportSettings, enabled: boolean) => void;
}) {
  useLanguage();
  return <section className="skill-sources" aria-label={t("技能导入权限")}>
    <p className="page-description">{t("默认全部关闭。下方显示实际扫描目录和识别数量；开启后递归读取，关闭后移出列表。也可导入整个技能集合文件夹。")}</p>
    <div className="skill-source-list">{sources.map(item => {
      const scanned = catalog?.sources?.find(entry => entry.source === item.id);
      const enabled = settings?.[item.id] ?? false;
      const path = scanned?.directory || item.location;
      const state = !enabled ? t("未开启") : scanning ? t("正在扫描…") : scanned?.status === "missing" ? t("目录不存在") : scanned?.status === "error" ? t("读取失败") : scanned ? t("已识别 {count} 个技能", { count: scanned.discovered }) + (scanned.status === "partial" ? t(" · 部分项目未读取") : "") : t("等待扫描");
      return <div className="skill-source" key={item.id} data-source={item.id}>
        <div className="skill-source__copy"><strong>{t(item.label)}</strong><code className="skill-source__path" title={path}>{path}</code><small className="skill-source__status" aria-live="polite">{state}</small></div>
        <Toggle aria-label={t("允许读取 ") + t(item.label) + " Skills"} checked={enabled} disabled={!settings || busy} onCheckedChange={enabled => onChange(item.id, enabled)} />
      </div>;
    })}</div>
  </section>;
}

function SkillWarnings({ warnings, imported = false }: { warnings?: string[]; imported?: boolean }) {
  if (!warnings?.length) return null;
  return <details className="workspace-disclosure skill-warnings" open={imported}>
    <summary>{t(imported ? "导入提示（{count}）" : "部分来源未读取（{count}）", { count: warnings.length })}</summary>
    <div className="workspace-disclosure__body"><ul>{warnings.map((warning, index) => <li key={index}>{translateMessage(warning)}</li>)}</ul></div>
  </details>;
}

export function SkillsPage() {
  useLanguage();
  const client = useQueryClient();
  const op = usePageOperation();
  const native = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
  const page = useRef<HTMLDivElement>(null);
  const searchInput = useRef<HTMLInputElement>(null);
  const resultsHeading = useRef<HTMLHeadingElement>(null);
  const sourceHeading = useRef<HTMLHeadingElement>(null);
  const directoryReturn = useRef<{ scrollTop: number; trigger: HTMLElement | null }>({ scrollTop: 0, trigger: null });
  const navigation = useRef<"library" | "sources" | null>(null);
  const [task, setTask] = useState<"library" | "sources">("library");
  const focusAfterRemoval = useRef(false);
  const [search, setSearch] = useState("");
  const [source, setSource] = useState("all");
  const [dragging, setDragging] = useState(false);
  const [importWarnings, setImportWarnings] = useState<string[]>([]);
  const [selectedId, setSelectedId] = useState("");
  const [pendingRemove, setPendingRemove] = useState<InstalledSkill | null>(null);
  const permissions = useQuery({ queryKey: ["skills", "settings"], queryFn: loadSkillImportSettings, retry: false });
  const catalog = useQuery({ queryKey: ["skills", "catalog"], queryFn: getSkillCatalog, enabled: permissions.isSuccess, retry: false });
  const refresh = useCallback(async () => { await client.invalidateQueries({ queryKey: ["skills"] }); }, [client]);
  const scroller = () => page.current?.closest<HTMLElement>(".xy-shell-main") ?? document.scrollingElement;
  const openSources = (trigger: HTMLElement | null) => {
    if (task === "library") directoryReturn.current = { scrollTop: scroller()?.scrollTop ?? 0, trigger };
    navigation.current = "sources"; setTask("sources");
  };
  const returnToLibrary = () => { navigation.current = "library"; setTask("library"); };
  useEffect(() => {
    if (!navigation.current) return;
    const root = scroller();
    if (navigation.current === "library") {
      if (root) root.scrollTop = directoryReturn.current.scrollTop;
      const trigger = directoryReturn.current.trigger;
      (trigger?.isConnected && !trigger.closest("[hidden]") ? trigger : resultsHeading.current)?.focus({ preventScroll: true });
    } else {
      if (root) root.scrollTop = 0;
      sourceHeading.current?.focus({ preventScroll: true });
    }
    navigation.current = null;
  }, [task]);
  const importPaths = (paths: string[]) => {
    if (pendingRemove || op.busy) return;
    openSources(document.activeElement instanceof HTMLElement ? document.activeElement : null);
    return op.run(async () => {
      setSelectedId("");
      setImportWarnings([]);
      const imported = await importSkillFolders(paths, importSkillFolder);
      setImportWarnings(imported.warnings);
      await refresh();
      op.setMessage(skillImportMessage(imported.results));
    });
  };
  const dropRef = useRef(importPaths);
  dropRef.current = importPaths;
  useEffect(() => {
    if (!("__TAURI_INTERNALS__" in window)) return;
    let cancelled = false;
    let stop: (() => void) | undefined;
    void getCurrentWebview().onDragDropEvent(event => {
      if (event.payload.type === "enter" || event.payload.type === "over") setDragging(true);
      if (event.payload.type === "leave") setDragging(false);
      if (event.payload.type === "drop") { setDragging(false); if (event.payload.paths.length) void dropRef.current(event.payload.paths); }
    }).then(cleanup => { if (cancelled) cleanup(); else stop = cleanup; }).catch(cause => op.setError(t("拖放监听失败：") + errorText(cause)));
    return () => { cancelled = true; stop?.(); };
  }, []);
  const skills = catalog.data?.skills ?? [];
  const availableSources = useMemo(() => [...new Set(skills.map(skill => skill.source))].sort(), [skills]);
  const keyword = search.trim().toLowerCase();
  const visible = useMemo(() => skills.filter(skill => (source === "all" || source === skill.source) && [skill.name, skill.description, skill.directory].some(value => (value ?? "").toLowerCase().includes(keyword))), [skills, source, keyword]);
  const selected = skills.find(skill => skill.id === selectedId);
  const clearFilters = () => { setSearch(""); setSource("all"); searchInput.current?.focus(); };
  useEffect(() => {
    if (catalog.isSuccess && source !== "all" && !availableSources.includes(source)) setSource("all");
    if (selectedId && !selected && !pendingRemove) { setSelectedId(""); focusAfterRemoval.current = true; }
    if (!selectedId && !pendingRemove && focusAfterRemoval.current) { focusAfterRemoval.current = false; searchInput.current?.focus(); }
  }, [catalog.isSuccess, source, availableSources, selectedId, selected, pendingRemove]);

  const changePermission = (id: keyof SkillImportSettings, enabled: boolean) => op.run(async () => {
    if (!permissions.data) return;
    const next = { ...permissions.data, [id]: enabled };
    await saveSkillImportSettings(next);
    client.setQueryData(["skills", "settings"], next);
    await client.invalidateQueries({ queryKey: ["skills", "catalog"] });
  }, t(enabled ? "已允许读取 {source}" : "已关闭读取 {source}", { source: sourceLabel(id) }));
  const pickFolder = () => op.run(async () => {
    setImportWarnings([]);
    const imported = await pickAndImportSkillFolder();
    if (imported) {
      setImportWarnings(imported.warnings);
      await refresh();
      op.setMessage(skillImportMessage([imported]));
    }
  });
  const remove = async () => {
    if (!pendingRemove) return;
    if (await op.run(async () => { await uninstallSkill(pendingRemove.id); await refresh(); }, t("已移除引用，源文件保持不变"))) {
      focusAfterRemoval.current = true;
      setPendingRemove(null);
      setSelectedId("");
    }
  };
  const permissionError = permissions.error ? t("来源设置读取失败：") + errorText(permissions.error) : "";
  const catalogError = catalog.error ? t("技能读取失败：") + errorText(catalog.error) : "";

  return <div className="workspace-page skills-page" ref={page} data-dragging={dragging} data-task={task}>
    <header className="page-heading"><div className="page-heading__copy"><h1>Skills</h1><p className="page-description">{t(task === "library" ? "查看本地技能，按需添加来源。" : "管理读取授权、扫描和文件夹引用。")}</p></div>
      <div className="page-actions"><Button hidden={task !== "library"} variant="secondary" size="sm" icon={<Settings2 size={14} />} disabled={op.busy} onClick={event => openSources(event.currentTarget)}>{t("管理来源")}</Button>
        <Button hidden={task !== "sources"} variant="secondary" size="sm" icon={<ArrowLeft size={14} />} disabled={op.busy} onClick={returnToLibrary}>{t("返回引用库")}</Button></div>
    </header>
    {catalog.data?.isolated && <div className="inline-notice skill-isolation-notice" role="status"><strong>{t("隔离测试窗口")}</strong><span>{t("当前来源已重定向到隔离目录；下方显示的是测试路径，不是你的真实用户目录。正式版读取实际平台目录。")}</span></div>}
    <InlineError message={!pendingRemove ? op.error : ""} />
    {op.message && <Toast title={op.message} tone="success" onDismiss={() => op.setMessage("")} />}
    {dragging && <div className="skill-drop-notice" role="status">{t("松开以导入技能文件夹")}</div>}
    <section className="skill-library" hidden={task !== "library"} aria-label={t("已读取技能")}>
      <header className="skill-library-heading"><h2 ref={resultsHeading} tabIndex={-1}>{t("XY Switch 引用库")}</h2><p className="skill-results-summary" role="status" aria-live="polite">{t("{visible} / {total} 个技能", { visible: visible.length, total: skills.length })}</p></header>
      {(permissionError || catalogError || importWarnings.length > 0 || (catalog.data?.warnings.length ?? 0) > 0 || catalog.data?.sources?.some(item => ["missing", "error", "partial"].includes(item.status))) && <div className="skill-source-notice" role="status"><span>{t("来源有读取或导入提示")}</span><Button variant="quiet" size="sm" onClick={event => openSources(event.currentTarget)}>{t("查看来源问题")}</Button></div>}
      <div className="skill-toolbar">
      <Input ref={searchInput} leading={<Search size={17} />} aria-label={t("搜索技能")} placeholder={t("搜索技能、说明或目录")} value={search} onChange={event => setSearch(event.target.value)}
        onKeyDown={event => { if (event.key === "Escape" && !event.nativeEvent.isComposing && search) { event.preventDefault(); event.stopPropagation(); setSearch(""); } }} />
      {(availableSources.length > 1 || source !== "all") && <Select className="skill-source-filter" aria-label={t("技能来源")} value={source} onChange={event => setSource(event.target.value)}><option value="all">{t("全部来源")}</option>{availableSources.map(value => <option key={value} value={value}>{sourceLabel(value)}</option>)}</Select>}
      {(search || source !== "all") && <Button variant="quiet" icon={<X size={16} />} onClick={clearFilters}>{t("清除筛选")}</Button>}
    </div>
    {permissions.isPending || (catalog.isPending && permissions.isSuccess) ? <LoadingState label={t("正在读取已开启的技能来源…")} /> : (permissions.isError || catalog.isError) && !skills.length ? null : !visible.length ? <EmptyState title={skills.length ? t("没有匹配的技能") : native ? t("暂无可显示的技能") : t("桌面端可读取本地技能")}
        description={skills.length ? t("清除搜索或来源筛选。") : native ? t("选择文件夹添加引用，或允许读取一个平台来源。") : t("浏览器预览不读取本地文件，也不能导入技能。")}
        action={skills.length ? <Button variant="secondary" onClick={clearFilters}>{t("清除筛选")}</Button> : <Button variant="secondary" onClick={event => openSources(event.currentTarget)}>{t("管理来源")}</Button>} />
        : <div className="record-list" role="list">{visible.map(skill => <article className="skill-record" role="listitem" key={skill.id}>
          <button type="button" className="skill-record__select" disabled={op.busy} onClick={() => setSelectedId(skill.id)} aria-label={t("查看 ") + skill.name}>
            <span className="skill-record__heading"><strong>{skill.name}</strong><span className="skill-record__source">{sourceLabel(skill.source)}<span>{skill.readOnly ? t("只读来源") : t("本地引用")}</span></span></span>
            <span className="skill-record__description">{skill.description || t("未填写说明")}</span>
            <code className="skill-record__path">{skill.directory}</code>
          </button>
        </article>)}</div>}
    </section>
    {task === "sources" && <section className="skill-source-management" aria-label={t("来源管理")}>
      <header className="skill-source-heading"><h2 ref={sourceHeading} tabIndex={-1}>{t("来源管理")}</h2><Button variant="secondary" size="sm" icon={<RefreshCw size={14} />} loading={catalog.isFetching || permissions.isFetching} disabled={op.busy || !permissions.isSuccess} onClick={() => void refresh()}>{t("刷新技能来源")}</Button></header>
      <InlineError message={permissionError} onRetry={() => void permissions.refetch()} retrying={permissions.isFetching} />
      <InlineError message={catalogError} onRetry={() => void refresh()} retrying={catalog.isFetching} />
      {permissions.isPending && <LoadingState label={t("正在读取权限设置…")} />}
      <SkillSources settings={permissions.data} catalog={catalog.data} busy={op.busy || !native} scanning={catalog.isFetching} onChange={(id, enabled) => void changePermission(id, enabled)} />
      <SkillWarnings warnings={catalog.data?.warnings} />
      <p className="page-description">{t("仅在 XY Switch 中查看，不改变原平台启用状态。")}</p>
      <section className="skill-import-panel" aria-label={t("导入文件夹")}>
        <div><h3>{t("导入文件夹")}</h3><p>{t("导入只添加引用，源文件保留在所选目录。")}</p><p className="page-description">{t("也可以将多个文件夹拖入此窗口。")}</p></div>
        <Button icon={<FolderInput size={16} />} loading={op.busy} disabled={!native} onClick={() => void pickFolder()}>{t("选择文件夹")}</Button>
      </section>
      <SkillWarnings warnings={importWarnings} imported />
      {!native && <p className="page-description">{t("浏览器预览不读取本地文件，也不能导入技能。")}</p>}
      <details className="workspace-disclosure"><summary>{t("技能信息")}</summary><div className="workspace-disclosure__body"><p className="page-description">{t("读取各平台 Skills 目录，或一次拖入多个技能文件夹。自动识别子目录中的 SKILL.md（含 .system），不扫描插件缓存。")}</p></div></details>
    </section>}
    {selected && !pendingRemove && <Modal className="skills-dialog" open size="md" title={selected.name} busy={op.busy} onClose={() => setSelectedId("")} footer={<Button variant="secondary" disabled={op.busy} onClick={() => setSelectedId("")}>{t("关闭详情")}</Button>}>
      <div className="skill-detail"><div className="skill-detail__source"><Badge>{sourceLabel(selected.source)}</Badge><span>{selected.readOnly ? t("只读来源") : t("本地引用")}</span></div><p>{selected.description || t("未填写说明")}</p>
        <p className="page-description">{t("这里只展示名称、说明和来源信息，不提供技能全文或执行入口。")}</p>
        <dl className="key-values"><dt>{t("目录")}</dt><dd className="mono">{selected.directory}</dd></dl>
        <p className="page-description">{t("仅在 XY Switch 中查看，不改变原平台启用状态。")}</p>
        {(selected.contentHash || selected.source === "manual") && <details className="workspace-disclosure"><summary>{t("技能信息")}</summary><div className="workspace-disclosure__body">
          {selected.contentHash && <dl className="key-values"><dt>{t("内容哈希")}</dt><dd className="mono">{selected.contentHash}</dd></dl>}
          {selected.source === "manual" && <Button variant="danger" icon={<Trash2 size={16} />} disabled={op.busy || !native} onClick={() => { op.setError(""); setPendingRemove(selected); }}>{t("移除引用")}</Button>}
        </div></details>}
      </div>
    </Modal>}
    {pendingRemove && <ConfirmActionDialog title={t("移除本地技能引用")} message={t("从 XY Switch 移除“") + pendingRemove.name + t("”的引用，不删除任何源文件。")}
      confirmLabel={t("移除引用")} busy={op.busy} error={op.error} onCancel={() => { setPendingRemove(null); op.setError(""); }} onConfirm={() => void remove()} />}
  </div>;
}
