import { useLanguage, t, localeTag } from "../../i18n/language";
import { useEffect, useMemo, useRef, useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { ArrowLeft, Plus, RefreshCw, Search, X } from "lucide-react";
import { loadPrompts, savePrompt, deletePrompt, setPromptEnabled } from "../../lib/prompt-service";
import { normalizePromptDraft, validatePromptDraft, PROMPT_NAME_MAX_LENGTH, PROMPT_DESCRIPTION_MAX_LENGTH, PROMPT_CONTENT_MAX_LENGTH, type PromptValidationErrors } from "../../lib/prompt-validation";
import { errorText } from "../../lib/error";
import type { Prompt } from "../../types/prompt";
import { ConfirmActionDialog } from "../../components/ConfirmActionDialog";
import { Badge, Button, EmptyState, Field, InlineError, Input, LoadingState, Tabs, Textarea, Toast, Toggle } from "../../ui";
import { usePageOperation } from "../shared/usePageOperation";
import { PromptReader } from "./PromptReader";
import "./prompts.css";

export function PromptsPage() {
  useLanguage();
  const client = useQueryClient();
  const query = useQuery({ queryKey: ["prompts"], queryFn: loadPrompts });
  const op = usePageOperation();
  const native = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
  const page = useRef<HTMLDivElement>(null);
  const searchInput = useRef<HTMLInputElement>(null);
  const resultsHeading = useRef<HTMLHeadingElement>(null);
  const readerHeading = useRef<HTMLHeadingElement>(null);
  const editButton = useRef<HTMLButtonElement>(null);
  const directoryReturn = useRef<{ scrollTop: number; trigger: HTMLElement | null }>({ scrollTop: 0, trigger: null });
  const readingPosition = useRef(0);
  const navigation = useRef<"library" | "reader" | "reader-return" | "editor" | null>(null);
  const [view, setView] = useState<"library" | "reader" | "editor">("library");
  const [search, setSearch] = useState("");
  const [filter, setFilter] = useState("all");
  const [selectedId, setSelectedId] = useState("");
  const [editor, setEditor] = useState<{ prompt: Prompt | null } | null>(null);
  const [pendingDelete, setPendingDelete] = useState<Prompt | null>(null);
  const list = query.data ?? [];
  const visible = useMemo(() => list.filter(prompt => (filter === "all" || prompt.enabled === (filter === "enabled")) && [prompt.name, prompt.description, prompt.content].some(value => (value ?? "").toLowerCase().includes(search.trim().toLowerCase()))), [list, search, filter]);
  const selected = list.find(prompt => prompt.id === selectedId);
  const directoryBusy = op.busy || view !== "library";
  const scroller = () => page.current?.closest<HTMLElement>(".xy-shell-main") ?? document.scrollingElement;
  const clearFilters = () => { setSearch(""); setFilter("all"); searchInput.current?.focus(); };
  const refresh = async () => { await client.invalidateQueries({ queryKey: ["prompts"] }); };
  const rememberDirectory = (trigger: HTMLElement) => { directoryReturn.current = { scrollTop: scroller()?.scrollTop ?? 0, trigger }; };
  const openPrompt = (prompt: Prompt, trigger: HTMLElement) => {
    rememberDirectory(trigger); op.setError(""); setSelectedId(prompt.id); navigation.current = "reader"; setView("reader");
  };
  const createPrompt = (trigger: HTMLElement) => {
    rememberDirectory(trigger); op.setError(""); setEditor({ prompt: null }); navigation.current = "editor"; setView("editor");
  };
  const returnToLibrary = () => { navigation.current = "library"; setEditor(null); setView("library"); };
  const closeEditor = () => {
    if (editor?.prompt && selected) { navigation.current = "reader-return"; setEditor(null); setView("reader"); }
    else returnToLibrary();
  };
  useEffect(() => {
    const destination = navigation.current;
    if (!destination) return;
    navigation.current = null;
    const root = scroller();
    if (destination === "library") {
      if (root) root.scrollTop = directoryReturn.current.scrollTop;
      const trigger = directoryReturn.current.trigger;
      (trigger?.isConnected && !trigger.closest("[hidden]") ? trigger : resultsHeading.current)?.focus({ preventScroll: true });
    } else {
      if (root) root.scrollTop = destination === "reader-return" ? readingPosition.current : 0;
      if (destination !== "editor") (destination === "reader-return" ? editButton.current : readerHeading.current)?.focus({ preventScroll: true });
    }
  }, [view, selectedId, editor]);
  useEffect(() => {
    if (view === "reader" && query.isSuccess && !selected && !pendingDelete) returnToLibrary();
  }, [view, query.isSuccess, selected, pendingDelete]);

  const duplicate = (prompt: Prompt) => op.run(async () => {
    let name = prompt.name + t(" 副本"), index = 2;
    while (list.some(item => item.name === name)) name = prompt.name + t(" 副本 ") + index++;
    const copy: Prompt = { ...prompt, id: "prompt-" + crypto.randomUUID(), name, enabled: false };
    await savePrompt(copy);
    await refresh();
    navigation.current = "reader";
    setSelectedId(copy.id);
  }, t("已复制提示词，副本默认停用"));
  const remove = async () => {
    if (!pendingDelete) return;
    const prompt = pendingDelete;
    if (await op.run(async () => { await deletePrompt(prompt.id); await refresh(); }, t("已删除 ") + prompt.name)) {
      setPendingDelete(null); setSelectedId(""); returnToLibrary();
    }
  };

  return <div className="workspace-page prompts-page" ref={page} data-view={view}>
    <header className="page-heading">
      <div className="page-heading__copy"><h1>{t("提示词")}</h1><p className="page-description">{t("为常用任务准备好指令。")}</p></div>
      <Button hidden={view !== "library"} size="sm" icon={<Plus size={14} />} disabled={directoryBusy || !native} onClick={event => createPrompt(event.currentTarget)}>{t("添加提示词")}</Button>
      {view === "reader" && <Button variant="secondary" size="sm" icon={<ArrowLeft size={14} />} disabled={op.busy} onClick={returnToLibrary}>{t("返回提示词目录")}</Button>}
    </header>
    {!native && <p className="page-description">{t("浏览器仅预览示例提示词；编辑与保存请使用桌面端。")}</p>}
    <InlineError message={(!pendingDelete ? op.error : "") || (query.error ? t("提示词读取失败：") + errorText(query.error) : "")} onRetry={editor ? undefined : () => void refresh()} retrying={query.isFetching} />
    {op.message && <Toast title={op.message} tone="success" onDismiss={() => op.setMessage("")} />}
    <section className="prompt-library" hidden={view !== "library"} aria-label={t("提示词列表")}>
      <header className="prompt-directory-heading"><h2 ref={resultsHeading} tabIndex={-1}>{t("提示词目录")}</h2><Button variant="quiet" size="sm" icon={<RefreshCw size={14} />} loading={query.isFetching} disabled={directoryBusy} onClick={() => void refresh()}>{t("刷新")}</Button></header>
      <div className="prompt-toolbar">
        <Input ref={searchInput} aria-label={t("搜索提示词")} leading={<Search size={14} />} value={search} placeholder={t("名称或内容")} disabled={directoryBusy}
          onChange={event => setSearch(event.target.value)} onKeyDown={event => { if (event.key === "Escape" && !event.nativeEvent.isComposing && search) { event.preventDefault(); event.stopPropagation(); setSearch(""); } }} />
        {(search || filter !== "all") && <Button variant="quiet" size="sm" icon={<X size={14} />} disabled={directoryBusy} onClick={clearFilters}>{t("清除筛选")}</Button>}
      </div>
      <div className="prompt-filters" role="group" aria-label={t("状态筛选与刷新")}>
        <Tabs label={t("提示词状态")} value={filter} onChange={setFilter} items={[{ value: "all", label: t("全部"), disabled: directoryBusy }, { value: "enabled", label: t("已启用"), disabled: directoryBusy }, { value: "disabled", label: t("已停用"), disabled: directoryBusy }]} />
      </div>
      {query.isPending ? <LoadingState label={t("读取提示词…")} /> : query.isError && !list.length ? null : <>
        <p className="prompt-results-summary" role="status" aria-live="polite">{t("{visible} / {total} 个提示词", { visible: visible.length, total: list.length })}</p>
        {!visible.length ? <EmptyState title={list.length ? t("没有匹配的提示词") : t("尚未添加提示词")}
          description={list.length ? t("清除搜索或状态筛选。") : native ? t("添加常用任务指令，保存后可随时编辑与停用。") : t("浏览器仅预览示例提示词；编辑与保存请使用桌面端。")}
          action={<Button variant="secondary" disabled={directoryBusy || !list.length && !native} onClick={event => list.length ? clearFilters() : createPrompt(event.currentTarget)}>{list.length ? t("清除筛选") : t("添加提示词")}</Button>} />
          : <div className="record-list" role="list">{visible.map(prompt => <article className="prompt-record" role="listitem" key={prompt.id} data-selected={selectedId === prompt.id}>
            <button className="prompt-record-select" type="button" disabled={directoryBusy} onClick={event => openPrompt(prompt, event.currentTarget)} aria-label={t("查看 ") + prompt.name} aria-pressed={selectedId === prompt.id}>
              <span className="prompt-record__copy"><strong>{prompt.name}</strong><span className="prompt-record__description">{prompt.description || prompt.content.slice(0, 100)}</span><span className="prompt-record__summary"><span className="prompt-record__status" data-enabled={prompt.enabled}>{prompt.enabled ? t("已启用") : t("已停用")}</span><span>{prompt.content.length.toLocaleString(localeTag())} {t("字符")}</span></span></span>
            </button>
          </article>)}</div>}
      </>}
    </section>
    {view === "editor" && editor && <section className="prompt-document" aria-label={t("编辑提示词")}><PromptEditor key={editor.prompt?.id ?? "new"} prompt={editor.prompt} onClose={closeEditor} onSaved={async prompt => {
      await refresh(); navigation.current = "reader"; setSelectedId(prompt.id); setEditor(null); setView("reader"); op.setMessage(t("提示词已保存"));
    }} /></section>}
    {view === "reader" && selected && <PromptReader prompt={selected} busy={op.busy} editable={native} headingRef={readerHeading} editRef={editButton}
      onCopy={() => void op.run(() => navigator.clipboard.writeText(selected.content), t("提示词内容已复制"))}
      onEdit={() => { readingPosition.current = scroller()?.scrollTop ?? 0; op.setError(""); navigation.current = "editor"; setEditor({ prompt: selected }); setView("editor"); }}
      onToggle={enabled => void op.run(async () => { await setPromptEnabled(selected.id, enabled); await refresh(); }, selected.name + (enabled ? t("已启用") : t("已停用")))}
      onDuplicate={() => void duplicate(selected)} onDelete={() => { op.setError(""); setPendingDelete(selected); }} />}
    {pendingDelete && <ConfirmActionDialog title={t("删除提示词")} message={t("删除“") + pendingDelete.name + t("”及其完整内容。此操作无法撤销。")}
      busy={op.busy} error={op.error} onCancel={() => { setPendingDelete(null); op.setError(""); }} onConfirm={() => void remove()} />}
  </div>;
}

export function PromptEditor({ prompt, onClose, onSaved }: { prompt: Prompt | null; onClose: () => void; onSaved: (prompt: Prompt) => Promise<void> }) {
  useLanguage();
  const [name, setName] = useState(prompt?.name ?? "");
  const [description, setDescription] = useState(prompt?.description ?? "");
  const [content, setContent] = useState(prompt?.content ?? "");
  const [enabled, setEnabled] = useState(prompt?.enabled ?? true);
  const [errors, setErrors] = useState<PromptValidationErrors>({});
  const [discardOpen, setDiscardOpen] = useState(false);
  const dirty = name !== (prompt?.name ?? "") || description !== (prompt?.description ?? "") || content !== (prompt?.content ?? "") || enabled !== (prompt?.enabled ?? true);
  const cancel = () => { if (dirty) setDiscardOpen(true); else onClose(); };
  const op = usePageOperation();
  const focus = useRef<HTMLInputElement>(null);
  const descriptionInput = useRef<HTMLInputElement>(null);
  const contentInput = useRef<HTMLTextAreaElement>(null);
  useEffect(() => { focus.current?.focus(); }, []);
  const save = async () => {
    const draft = normalizePromptDraft({ name, description, content });
    const validation = validatePromptDraft(draft);
    setErrors(validation);
    if (validation.name) focus.current?.focus();
    else if (validation.description) descriptionInput.current?.focus();
    else if (validation.content) contentInput.current?.focus();
    if (Object.keys(validation).length || op.busy) return;
    const next: Prompt = { ...prompt, id: prompt?.id ?? "prompt-" + crypto.randomUUID(), name: draft.name, description: draft.description ?? null, content: draft.content, enabled };
    await op.run(async () => { await savePrompt(next); await onSaved(next); });
  };
  return <><form className="prompt-editor" aria-label={prompt ? t("编辑提示词") : t("添加提示词")} aria-busy={op.busy} onSubmit={event => { event.preventDefault(); void save(); }} noValidate>
      <header className="prompt-document-heading"><h2>{prompt ? t("编辑提示词") : t("添加提示词")}</h2><Badge tone={dirty ? "warning" : "neutral"}>{t(dirty ? "有未保存的修改" : "编辑中")}</Badge></header>
      <p className="page-description">{t("保存后返回阅读；取消可返回原位置。")}</p>
      <InlineError message={op.error} />
      <Field label={t("名称")} required error={errors.name}><Input ref={focus} value={name} maxLength={PROMPT_NAME_MAX_LENGTH} disabled={op.busy} onChange={event => setName(event.target.value)} /></Field>
      <Field label={t("说明")} error={errors.description}><Input ref={descriptionInput} value={description} maxLength={PROMPT_DESCRIPTION_MAX_LENGTH} disabled={op.busy} onChange={event => setDescription(event.target.value)} /></Field>
      <Field label={t("提示词内容")} required error={errors.content}><Textarea ref={contentInput} className="prompt-editor-content" value={content} disabled={op.busy} onChange={event => setContent(event.target.value)} /></Field>
      <div className="prompt-editor-settings">
        <div className="form-switch"><strong>{t("启用此提示词")}</strong><Toggle aria-label={t("启用此提示词")} checked={enabled} disabled={op.busy} onCheckedChange={setEnabled} /></div>
        <span className="editor-character-count" aria-live="polite">{content.length.toLocaleString(localeTag())} / {PROMPT_CONTENT_MAX_LENGTH.toLocaleString(localeTag())} {t("字符")}</span>
      </div>
      <footer className="prompt-editor-actions"><Button variant="secondary" disabled={op.busy} onClick={cancel}>{t("取消")}</Button><Button type="submit" loading={op.busy}>{t("保存提示词")}</Button></footer>
    </form>{discardOpen && <ConfirmActionDialog title={t("放弃本次修改？")} message={t("尚未保存的修改将丢失，已保存的提示词保持不变。")} confirmLabel={t("放弃修改")} busy={op.busy} onCancel={() => setDiscardOpen(false)} onConfirm={onClose} />}</>;
}
