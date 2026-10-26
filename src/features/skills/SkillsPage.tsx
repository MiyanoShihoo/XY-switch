import { translateMessage, t, useLanguage } from "../../i18n/language";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { FolderInput, RefreshCw, Search, Trash2 } from "lucide-react";
import { getSkillCatalog, loadSkillImportSettings, saveSkillImportSettings, uninstallSkill, importSkillFolder, pickAndImportSkillFolder } from "../../lib/skill-service";
import { errorText } from "../../lib/error";
import { importSkillFolders, skillImportMessage } from "../../lib/skill-import";
import type { InstalledSkill, SkillImportSettings } from "../../types/skill";
import { ConfirmActionDialog } from "../../components/ConfirmActionDialog";
import { Badge, Button, EmptyState, InlineError, Input, LoadingState, Select, Toast, Toggle } from "../../ui";
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

export function SkillsPage() {
  useLanguage();
  const client = useQueryClient();
  const op = usePageOperation();
  const [search, setSearch] = useState("");
  const [source, setSource] = useState("all");
  const [dragging, setDragging] = useState(false);
  const [importWarnings, setImportWarnings] = useState<string[]>([]);
  const [pendingRemove, setPendingRemove] = useState<InstalledSkill | null>(null);
  const permissions = useQuery({ queryKey: ["skills", "settings"], queryFn: loadSkillImportSettings, retry: false });
  const catalog = useQuery({ queryKey: ["skills", "catalog"], queryFn: getSkillCatalog, enabled: permissions.isSuccess, retry: false });
  const refresh = useCallback(async () => { await client.invalidateQueries({ queryKey: ["skills"] }); }, [client]);
  const importPaths = (paths: string[]) => op.run(async () => {
    setImportWarnings([]);
    const imported = await importSkillFolders(paths, importSkillFolder);
    setImportWarnings(imported.warnings);
    await refresh();
    op.setMessage(skillImportMessage(imported.results));
  });
  const dropRef = useRef(importPaths); dropRef.current = importPaths;
  useEffect(() => {
    if (!("__TAURI_INTERNALS__" in window)) return;
    let cancelled = false; let stop: (() => void) | undefined;
    void getCurrentWebview().onDragDropEvent(event => {
      if (event.payload.type === "enter" || event.payload.type === "over") setDragging(true);
      if (event.payload.type === "leave") setDragging(false);
      if (event.payload.type === "drop") { setDragging(false); if (event.payload.paths.length) void dropRef.current(event.payload.paths); }
    }).then(cleanup => { if (cancelled) cleanup(); else stop = cleanup; }).catch(cause => op.setError(t("拖放监听失败：") + errorText(cause)));
    return () => { cancelled = true; stop?.(); };
  }, []);
  const skills = catalog.data?.skills ?? [];
  const keyword = search.trim().toLowerCase();
  const visible = useMemo(() => skills.filter(skill => (source === "all" || source === skill.source) && [skill.name, skill.description, skill.directory].some(value => (value ?? "").toLowerCase().includes(keyword))), [skills, source, keyword]);
  const changePermission = (id: keyof SkillImportSettings, enabled: boolean) => op.run(async () => {
    if (!permissions.data) return;
    const next = { ...permissions.data, [id]: enabled };
    await saveSkillImportSettings(next);
    client.setQueryData(["skills", "settings"], next);
    await client.invalidateQueries({ queryKey: ["skills", "catalog"] });
  }, t(enabled ? "已允许读取 {source}" : "已关闭读取 {source}", { source: sourceLabel(id) }));
  const remove = async () => {
    if (!pendingRemove) return;
    if (await op.run(async () => { await uninstallSkill(pendingRemove.id); await refresh(); }, t("已移除引用，源文件保持不变"))) setPendingRemove(null);
  };
  return <div className="workspace-page skills-page" data-dragging={dragging}>
    <header className="page-heading"><div className="page-heading__copy"><h1>Skills</h1><p className="page-description">{t("读取各平台 Skills 目录，或一次拖入多个技能文件夹。自动识别子目录中的 SKILL.md（含 .system），不扫描插件缓存。")}</p></div><div className="page-actions"><Button variant="secondary" size="sm" icon={<RefreshCw size={14} />} loading={catalog.isFetching} disabled={op.busy} onClick={() => void refresh()}>{t("刷新")}</Button><Button size="sm" icon={<FolderInput size={14} />} loading={op.busy} onClick={() => void op.run(async () => { setImportWarnings([]); const imported = await pickAndImportSkillFolder(); if (imported) { setImportWarnings(imported.warnings); await refresh(); op.setMessage(skillImportMessage([imported])); } })}>{t("导入文件夹")}</Button></div></header>
    {catalog.data?.isolated && <div className="inline-notice skill-isolation-notice" role="status"><strong>{t("隔离测试窗口")}</strong><span>{t("当前来源已重定向到隔离目录；下方显示的是测试路径，不是你的真实用户目录。正式版读取实际平台目录。")}</span></div>}
    <section className="settings-section" aria-label={t("技能导入权限")}><div className="settings-section__heading"><h2>{t("允许读取平台 Skills")}</h2><p>{t("默认全部关闭。下方显示实际扫描目录和识别数量；开启后递归读取，关闭后移出列表。也可导入整个技能集合文件夹。")}</p></div>
      {permissions.isPending && <LoadingState label={t("正在读取权限设置…")} />}
      {permissions.error && <InlineError message={errorText(permissions.error)} onRetry={() => void permissions.refetch()} />}
      <div className="form-stack">{sources.map(item => {
        const scanned = catalog.data?.sources?.find(entry => entry.source === item.id);
        const enabled = permissions.data?.[item.id] ?? false;
        const path = scanned?.directory || item.location;
        const state = !enabled ? t("未开启") : catalog.isFetching ? t("正在扫描…") : scanned?.status === "missing" ? t("目录不存在") : scanned?.status === "error" ? t("读取失败") : scanned ? t("已识别 {count} 个技能", { count: scanned.discovered }) + (scanned.status === "partial" ? t(" · 部分项目未读取") : "") : t("等待扫描");
        return <div className="form-switch skill-source" key={item.id} data-source={item.id}>
          <div className="skill-source__copy"><strong>{t(item.label)}</strong><code className="skill-source__path" title={path}>{path}</code><small className="skill-source__status" aria-live="polite">{state}</small></div>
          <Toggle aria-label={t("允许读取 ") + t(item.label) + " Skills"} checked={enabled} disabled={!permissions.data || op.busy} onCheckedChange={enabled => void changePermission(item.id, enabled)} />
        </div>;
      })}</div>
    </section>
    <InlineError message={op.error || (catalog.error ? t("技能读取失败：") + errorText(catalog.error) : "")} onRetry={() => void catalog.refetch()} />
    {op.message && <Toast title={op.message} tone="success" onDismiss={() => op.setMessage("")} />}
    {Boolean(importWarnings.length) && <details className="inline-notice" open><summary>{t("导入提示（{count}）", { count: importWarnings.length })}</summary><ul>{importWarnings.map((warning, index) => <li key={index}>{translateMessage(warning)}</li>)}</ul></details>}
    {Boolean(catalog.data?.warnings.length) && <details className="inline-notice"><summary>{t("部分来源未读取（{count}）", { count: catalog.data?.warnings.length })}</summary><ul>{catalog.data?.warnings.map((warning, index) => <li key={index}>{translateMessage(warning)}</li>)}</ul></details>}
    <div className="toolbar"><div className="search-field"><Search size={15} /><Input aria-label={t("搜索技能")} placeholder={t("搜索技能、说明或目录")} value={search} onChange={event => setSearch(event.target.value)} /></div><Select aria-label={t("技能来源")} value={source} onChange={event => setSource(event.target.value)}><option value="all">{t("全部来源")}</option>{sources.map(item => <option key={item.id} value={item.id}>{t(item.label)}</option>)}<option value="manual">{t("手动导入")}</option></Select><span className="muted">{visible.length} {t("个技能")}</span></div>
    {catalog.isFetching && !catalog.data ? <LoadingState label={t("正在读取已开启的技能来源…")} /> : !visible.length ? <EmptyState title={t("暂无可显示的技能")} description={t("开启上方平台读取开关，或导入单个技能 / 包含多个技能的文件夹。")} /> : <div className="record-list" aria-label={t("已读取技能")}>{visible.map(skill => <details className="skill-record" key={skill.id}><summary><span className="skill-record__copy"><strong>{skill.name}</strong><span>{skill.description || t("未填写说明")}</span></span><span className="inline-actions"><Badge>{sourceLabel(skill.source)}</Badge></span></summary><div className="skill-record__body"><dl className="key-values"><dt>{t("目录")}</dt><dd className="mono">{skill.directory}</dd><dt>{t("内容哈希")}</dt><dd className="mono">{skill.contentHash}</dd></dl><p className="muted">{t("仅在 XY Switch 中查看，不改变原平台启用状态。")}</p>{skill.source === "manual" && <Button variant="danger" size="sm" icon={<Trash2 size={14} />} disabled={op.busy} onClick={() => setPendingRemove(skill)}>{t("移除引用")}</Button>}</div></details>)}</div>}
    {pendingRemove && <ConfirmActionDialog title={t("移除本地技能引用")} message={t("从 XY Switch 移除“") + pendingRemove.name + t("”的引用，不删除任何源文件。")} confirmLabel={t("移除引用")} busy={op.busy} error={op.error} onCancel={() => setPendingRemove(null)} onConfirm={() => void remove()} />}
  </div>;
}
