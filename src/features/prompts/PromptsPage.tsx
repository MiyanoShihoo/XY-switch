import { useEffect, useMemo, useRef, useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { Copy, Pencil, Plus, RefreshCw, Search, Trash2 } from "lucide-react";
import { loadPrompts, savePrompt, deletePrompt, setPromptEnabled } from "../../lib/prompt-service";
import { normalizePromptDraft, validatePromptDraft, PROMPT_NAME_MAX_LENGTH, PROMPT_DESCRIPTION_MAX_LENGTH, PROMPT_CONTENT_MAX_LENGTH, type PromptValidationErrors } from "../../lib/prompt-validation";
import { errorText } from "../../lib/error";
import type { Prompt } from "../../types/prompt";
import { ConfirmActionDialog } from "../../components/ConfirmActionDialog";
import { Badge, Button, Drawer, EmptyState, Field, IconButton, InlineError, Input, Inspector, LoadingState, Modal, Tabs, Textarea, Toast, Toggle } from "../../ui";
import { usePageOperation } from "../shared/usePageOperation";
import { useCompact } from "../shared/useCompact";
import "./prompts.css";

export function PromptsPage() {
  const client = useQueryClient();
  const query = useQuery({ queryKey: ["prompts"], queryFn: loadPrompts });
  const op = usePageOperation();
  const compact = useCompact();
  const [search, setSearch] = useState("");
  const [filter, setFilter] = useState("all");
  const [selectedId, setSelectedId] = useState("");
  const [drawerOpen, setDrawerOpen] = useState(false);
  const [editor, setEditor] = useState<{ prompt: Prompt | null } | null>(null);
  const [pendingDelete, setPendingDelete] = useState<Prompt | null>(null);
  const list = query.data ?? [];
  const visible = useMemo(() => list.filter(prompt => (filter === "all" || prompt.enabled === (filter === "enabled")) && [prompt.name, prompt.description, prompt.content].some(value => (value ?? "").toLowerCase().includes(search.trim().toLowerCase()))), [list, search, filter]);
  const selected = visible.find(prompt => prompt.id === selectedId) ?? visible[0];
  useEffect(() => { if (!compact || !selected) setDrawerOpen(false); }, [compact, selected]);
  const refresh = async () => { await client.invalidateQueries({ queryKey: ["prompts"] }); };
  const inspect = (prompt: Prompt) => { setSelectedId(prompt.id); if (compact) setDrawerOpen(true); };
  const duplicate = (prompt: Prompt) => op.run(async () => {
    let name = prompt.name + " 副本", index = 2;
    while (list.some(item => item.name === name)) name = prompt.name + " 副本 " + index++;
    const copy: Prompt = { ...prompt, id: "prompt-" + crypto.randomUUID(), name, enabled: false };
    await savePrompt(copy); await refresh(); setSelectedId(copy.id);
  }, "已复制提示词，副本默认停用");
  const remove = async () => { if (!pendingDelete) return; const prompt = pendingDelete; if (await op.run(async () => { await deletePrompt(prompt.id); await refresh(); }, "已删除 " + prompt.name)) { setPendingDelete(null); setDrawerOpen(false); } };
  const preview = selected ? <div className="form-stack"><div className="section-heading"><h3>{selected.name}</h3><Badge tone={selected.enabled ? "accent" : "neutral"}>{selected.enabled ? "已启用" : "已停用"}</Badge></div><p className="page-description">{selected.description || "未填写说明"}</p><pre className="prompt-preview mono">{selected.content}</pre><p className="page-description">{selected.content.length.toLocaleString()} 字符 · 内容按纯文本呈现</p><div className="inline-actions"><Button variant="secondary" size="sm" icon={<Pencil size={14} />} disabled={op.busy} onClick={() => { setDrawerOpen(false); setEditor({ prompt: selected }); }}>编辑</Button><Button variant="quiet" size="sm" icon={<Copy size={14} />} disabled={op.busy} onClick={() => void op.run(() => navigator.clipboard.writeText(selected.content), "提示词内容已复制")}>复制内容</Button></div></div> : <EmptyState title="选择提示词" description="在右侧查看完整内容，避免长文撑开列表。" />;
  return <div className="workspace-page prompts-page"><header className="page-heading"><div className="page-heading__copy"><h1>提示词</h1><p className="page-description">列表管理名称与启用状态，检查面板查看完整文本。</p></div><Button size="sm" icon={<Plus size={14} />} disabled={op.busy} onClick={() => setEditor({ prompt: null })}>添加提示词</Button></header>
    <InlineError message={op.error || (query.error ? "提示词读取失败：" + errorText(query.error) : "")} onRetry={() => void refresh()} retrying={query.isFetching} />{op.message && <Toast title={op.message} tone="success" onDismiss={() => op.setMessage("")} />}
    <div className="toolbar"><Tabs label="提示词状态" value={filter} onChange={setFilter} items={[{ value: "all", label: "全部", count: list.length }, { value: "enabled", label: "已启用", count: list.filter(prompt => prompt.enabled).length }, { value: "disabled", label: "已停用" }]} /><div className="inline-actions"><Input className="toolbar__search" aria-label="搜索提示词" leading={<Search size={14} />} value={search} placeholder="名称或内容" onChange={event => setSearch(event.target.value)} /><Button variant="quiet" size="sm" icon={<RefreshCw size={14} />} loading={query.isFetching} disabled={op.busy} onClick={() => void refresh()}>刷新</Button></div></div>
    {query.isPending ? <LoadingState label="读取提示词…" /> : <div className="split-workspace prompt-workspace"><div className="split-workspace__list">{!visible.length ? <EmptyState title={list.length ? "没有匹配的提示词" : "尚未添加提示词"} description={list.length ? "清除搜索或状态筛选。" : "添加常用任务指令，保存后可随时编辑与停用。"} action={<Button variant="secondary" size="sm" onClick={() => list.length ? (setSearch(""), setFilter("all")) : setEditor({ prompt: null })}>{list.length ? "清除筛选" : "添加提示词"}</Button>} /> : <div className="record-list" role="list" aria-label="提示词列表">{visible.map(prompt => <article className="prompt-record" data-selected={prompt.id === selected?.id} role="listitem" key={prompt.id}><button className="record-select" onClick={() => inspect(prompt)} aria-label={"查看 " + prompt.name}><strong>{prompt.name}</strong><span className="record-row__detail">{prompt.description || prompt.content.slice(0, 100)}</span><small className="muted">{prompt.content.length.toLocaleString()} 字符</small></button><Toggle aria-label={"启用 " + prompt.name} checked={prompt.enabled} disabled={op.busy} onCheckedChange={enabled => void op.run(async () => { await setPromptEnabled(prompt.id, enabled); await refresh(); }, prompt.name + (enabled ? "已启用" : "已停用"))} /><div className="record-row__actions"><IconButton aria-label={"复制 " + prompt.name} disabled={op.busy} onClick={() => void duplicate(prompt)}><Copy size={14} /></IconButton><IconButton aria-label={"编辑 " + prompt.name} disabled={op.busy} onClick={() => setEditor({ prompt })}><Pencil size={14} /></IconButton><IconButton aria-label={"删除 " + prompt.name} disabled={op.busy} onClick={() => { op.setError(""); setPendingDelete(prompt); }}><Trash2 size={14} /></IconButton></div></article>)}</div>}</div><Inspector title="内容检查">{preview}</Inspector></div>}
    <Drawer open={compact && drawerOpen} title="提示词内容" busy={op.busy} onClose={() => setDrawerOpen(false)}>{preview}</Drawer>
    {editor && <PromptEditor prompt={editor.prompt} onClose={() => setEditor(null)} onSaved={refresh} />}
    {pendingDelete && <ConfirmActionDialog title="删除提示词" message={"删除“" + pendingDelete.name + "”及其完整内容。此操作无法撤销。"} busy={op.busy} error={op.error} onCancel={() => { setPendingDelete(null); op.setError(""); }} onConfirm={() => void remove()} />}
  </div>;
}
export function PromptEditor({ prompt, onClose, onSaved }: { prompt: Prompt | null; onClose: () => void; onSaved: () => Promise<void> }) {
  const [name, setName] = useState(prompt?.name ?? "");
  const [description, setDescription] = useState(prompt?.description ?? "");
  const [content, setContent] = useState(prompt?.content ?? "");
  const [enabled, setEnabled] = useState(prompt?.enabled ?? true);
  const [errors, setErrors] = useState<PromptValidationErrors>({});
  const op = usePageOperation(); const focus = useRef<HTMLInputElement>(null);
  const save = async () => {
    const draft = normalizePromptDraft({ name, description, content });
    const validation = validatePromptDraft(draft); setErrors(validation);
    if (Object.keys(validation).length || op.busy) return;
    const next: Prompt = { ...prompt, id: prompt?.id ?? "prompt-" + crypto.randomUUID(), name: draft.name, description: draft.description ?? null, content: draft.content, enabled };
    if (await op.run(async () => { await savePrompt(next); await onSaved(); })) onClose();
  };
  return <Modal open title={prompt ? "编辑提示词" : "添加提示词"} size="lg" busy={op.busy} onClose={onClose} initialFocusRef={focus} footer={<><span className="editor-character-count">{content.length.toLocaleString()} / {PROMPT_CONTENT_MAX_LENGTH.toLocaleString()} 字符</span><Button variant="secondary" disabled={op.busy} onClick={onClose}>取消</Button><Button loading={op.busy} onClick={() => void save()}>保存提示词</Button></>}><form className="form-stack" onSubmit={event => { event.preventDefault(); void save(); }} noValidate><InlineError message={op.error} /><Field label="名称" required error={errors.name}><Input ref={focus} value={name} maxLength={PROMPT_NAME_MAX_LENGTH} disabled={op.busy} onChange={event => setName(event.target.value)} /></Field><Field label="说明" error={errors.description}><Input value={description} maxLength={PROMPT_DESCRIPTION_MAX_LENGTH} disabled={op.busy} onChange={event => setDescription(event.target.value)} /></Field><Field label="提示词内容" required error={errors.content}><Textarea className="prompt-editor-content mono" value={content} disabled={op.busy} onChange={event => setContent(event.target.value)} /></Field><div className="form-switch"><strong>启用此提示词</strong><Toggle aria-label="启用此提示词" checked={enabled} disabled={op.busy} onCheckedChange={setEnabled} /></div><button className="sr-only" tabIndex={-1} disabled={op.busy} type="submit">保存提示词</button></form></Modal>;
}
