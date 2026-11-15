import { useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { t } from "../../i18n/language";
import { applyExplicitMount, chooseMountRoot, deleteMountBackup, listMountBackups, previewExplicitMount, previewMountRollback } from "../../lib/governance-service";
import { loadPrompts } from "../../lib/prompt-service";
import { getSkillCatalog } from "../../lib/skill-service";
import type { MountBackup, MountClient, MountPreview, MountRoot } from "../../types/governance";
import { Badge, Button, Field, InlineError, Modal, Select, Tabs } from "../../ui";
import { useTaskDraft, useTaskNavigation } from "../settings/TaskNavigation";
import { Confirmation, DiffPreview, OperationMessage, stamp, useClock, useGovernanceOperation } from "./shared";

export function MountsPanel() {
  const [view, setView] = useState("configure");
  const prompts = useQuery({ queryKey: ["prompts"], queryFn: loadPrompts });
  const skills = useQuery({ queryKey: ["governance", "mount-skills"], queryFn: getSkillCatalog });
  const backups = useQuery({ queryKey: ["governance", "mount-backups"], queryFn: listMountBackups });
  const { leave } = useTaskNavigation();
  const [choosingSources, setChoosingSources] = useState(false);
  const [root, setRoot] = useState<MountRoot | null>(null), [client, setClient] = useState<MountClient>("codex");
  const [promptIds, setPromptIds] = useState<string[]>([]), [skillIds, setSkillIds] = useState<string[]>([]);
  const [preview, setPreview] = useState<MountPreview | null>(null), [confirmation, setConfirmation] = useState(""), [reviewed, setReviewed] = useState(false);
  const [deleting, setDeleting] = useState<MountBackup | null>(null), [deleteWord, setDeleteWord] = useState(""), [visibleCount, setVisibleCount] = useState(30);
  const op = useGovernanceOperation(), now = useClock(), selectedCount = promptIds.length + skillIds.length;
  useTaskDraft(Boolean(root || preview || deleting || confirmation || deleteWord) || selectedCount > 0, op.busy);
  const clearPreview = () => { setPreview(null); setConfirmation(""); setReviewed(false); };
  const chooseSource = (kind: "prompt" | "skill", id: string) => {
    clearPreview(); const update = (current: string[]) => current.includes(id) ? current.filter(v => v !== id) : [...current, id];
    if (kind === "prompt") setPromptIds(update); else setSkillIds(update);
  };
  return <div className="gov-stack">{!preview && !deleting && <OperationMessage op={op} />}
    <section className="gov-card"><h2>{t("提示词 / Skills 显式挂载")}</h2><p className="gov-warning">{t("不会自动写 CLAUDE.md、AGENTS.md 或 GEMINI.md。每次都必须选目录、选择来源、预览 diff 并明确确认；外部文件只在确认后变更。")}</p>
      <div className="gov-mount-root"><Button variant="secondary" disabled={op.busy} onClick={() => { clearPreview(); void op.run(async () => { const selected = await chooseMountRoot(); if (selected) { setRoot(selected); setChoosingSources(false); setPromptIds([]); setSkillIds([]); } }); }}>{t("选择项目目录")}</Button><code className="gov-path" tabIndex={0}>{root?.path ?? t("尚未选择目录")}</code></div>
    </section>
    <Tabs id="mounts" label={t("挂载工作区")} value={view} onChange={next => leave(() => { clearPreview(); setDeleting(null); setDeleteWord(""); setRoot(null); setChoosingSources(false); setPromptIds([]); setSkillIds([]); setView(next); })} items={[{ value: "configure", label: t("挂载配置"), disabled: op.busy || Boolean(preview || deleting) }, { value: "backups", label: t("备份历史"), disabled: op.busy || Boolean(preview || deleting) }]} />
    {view === "configure" && !root && <p role="status">{t("先选择项目目录，再指定目标文件和来源；目录授权仅用于当前任务。")}</p>}
    {view === "configure" && root && <div role="tabpanel" id="mounts-panel-configure" aria-labelledby="mounts-tab-configure"><section className="gov-card"><fieldset disabled={op.busy} className="gov-fieldset">
        <h3>{choosingSources ? t("选择要写入此文件的内容") : t("指定目标文件")}</h3>
        <Field label={t("挂载目标")}><Select value={client} onChange={e => { setClient(e.target.value as MountClient); clearPreview(); setChoosingSources(false); }}><option value="claude">CLAUDE.md</option><option value="codex">AGENTS.md</option><option value="gemini">GEMINI.md</option></Select></Field>
        {!choosingSources && <div className="gov-actions"><Button variant="secondary" onClick={() => setChoosingSources(true)}>{t("继续选择来源")}</Button></div>}
        {choosingSources && <section className="gov-source-picker"><div className="section-heading"><h3>{t("选择挂载来源")}</h3><Badge>{t("已选 {count} 项", { count: selectedCount })}</Badge></div><div className="gov-view">
        <p className="page-description">{t("最多选择 20 项。Skills 只读取已显式开启扫描的本地来源；仅挂载 SKILL.md 内容，不复制脚本、资源或安装依赖。")}</p>
        <div className="gov-grid"><fieldset className="gov-sources"><legend>{t("已保存提示词")}</legend>{prompts.error && <InlineError message={String(prompts.error)} onRetry={() => void prompts.refetch()} />}{prompts.isPending && <p role="status">{t("读取中")}</p>}{prompts.data?.map(p => <label className="gov-check" key={p.id}><input type="checkbox" checked={promptIds.includes(p.id)} disabled={!promptIds.includes(p.id) && selectedCount >= 20} onChange={() => chooseSource("prompt", p.id)} /><span>{p.name}</span></label>)}{prompts.data?.length === 0 && <p>{t("没有可挂载的提示词")}</p>}</fieldset>
          <fieldset className="gov-sources"><legend>{t("本地 Skills")}</legend>{skills.error && <InlineError message={String(skills.error)} onRetry={() => void skills.refetch()} />}{skills.isPending && <p role="status">{t("读取中")}</p>}{skills.data?.skills.map(skill => <label className="gov-check" key={skill.id}><input type="checkbox" checked={skillIds.includes(skill.id)} disabled={!skillIds.includes(skill.id) && selectedCount >= 20} onChange={() => chooseSource("skill", skill.id)} /><span>{skill.name}<small>{skill.source}</small></span></label>)}{skills.data?.skills.length === 0 && <p>{t("没有可挂载的 Skills；可先在 Skills 页面显式开启来源。")}</p>}{skills.data?.warnings.length ? <p className="gov-warning">{t("部分 Skills 来源不可用，请到 Skills 页面查看详情。")}</p> : null}</fieldset></div>
        </div></section>}
        {choosingSources && <div className="gov-actions"><Button disabled={!root || selectedCount < 1 || selectedCount > 20} onClick={() => { if (!root) return; clearPreview(); void op.run(async () => { setPreview(await previewExplicitMount(root.id, client, promptIds, skillIds)); }); }}>{t("只预览，不写入")}</Button></div>}
      </fieldset>
    </section></div>}
    {preview && <Modal open className="gov-review-modal" title={t("文件变更预览")} size="lg" busy={op.busy} onClose={clearPreview} footer={<div className="gov-actions"><Button disabled={op.busy || !reviewed || confirmation !== "WRITE FILE" || now >= preview.expiresAt} onClick={() => void op.run(async () => { await applyExplicitMount(preview.id, confirmation); clearPreview(); await op.refresh(); }, t("文件已按预览更新，原文备份已保留"))}>{t("备份并应用文件变更")}</Button><Button variant="quiet" disabled={op.busy} onClick={clearPreview}>{t("取消")}</Button></div>}><div className="gov-review"><OperationMessage op={op} /><div className="gov-review__layout"><div className="gov-review__document"><DiffPreview identity={preview.target} before={preview.before} after={preview.after} /></div>
      <aside className="gov-review__approval" aria-label={t("最终确认")}><h3>{t("最终确认")}</h3><div className="gov-confirm-subject"><span>{t("挂载目标")}</span><code className="gov-id" tabIndex={0}>{preview.target}</code></div><div className="gov-review__meta">{preview.createsFile && <Badge tone="accent">{t("将创建文件")}</Badge>}{preview.removesFile && <Badge tone="danger">{t("回滚将删除新建文件")}</Badge>}<Badge tone={now >= preview.expiresAt ? "danger" : "neutral"}>{now >= preview.expiresAt ? t("预览已过期，请重新生成") : t("预览有效至") + " " + stamp(preview.expiresAt)}</Badge></div>
      <p className="gov-warning">{t("保留管理块外的手写内容。写入前会复查原文、完整备份并原子替换；文件被外部修改后必须重新预览。备份存于本地数据库，不是跨设备同步。")}</p>
      <div className="gov-approval"><label className="gov-check"><input type="checkbox" disabled={op.busy} checked={reviewed} onChange={e => setReviewed(e.target.checked)} />{t("我已查看上述 diff")}</label><Confirmation word="WRITE FILE" value={confirmation} onChange={setConfirmation} disabled={op.busy} /></div>
      </aside></div></div></Modal>}
    {view === "backups" && <div role="tabpanel" id="mounts-panel-backups" aria-labelledby="mounts-tab-backups"><section className="gov-card"><h2>{t("外部文件备份与回滚")}</h2><p className="page-description">{t("回滚前必须重新选中原项目目录并预览。若文件之后被外部修改，不会强行覆盖。最多保留 1000 份备份，满额后不再写文件。")}</p>
      {backups.error && <InlineError message={String(backups.error)} onRetry={() => void backups.refetch()} />}
      {backups.isPending && <p role="status">{t("读取中")}</p>}
      <ul className="gov-list">{backups.data?.slice(0, visibleCount).map(backup => <li key={backup.id}><div><code className="gov-path">{backup.target}</code><p>{stamp(backup.createdAt)}</p></div><div className="gov-actions"><Button variant="secondary" disabled={!root || op.busy} onClick={() => { if (!root) return; clearPreview(); void op.run(async () => { setPreview(await previewMountRollback(root.id, backup.id)); }); }}>{t("预览文件回滚")}</Button><Button variant="danger" disabled={op.busy} onClick={() => { clearPreview(); setDeleting(backup); setDeleteWord(""); op.setError(""); op.setMessage(""); }}>{t("删除备份")}</Button></div></li>)}</ul>
      {(backups.data?.length ?? 0) > visibleCount && <Button variant="quiet" onClick={() => setVisibleCount(v => v + 50)}>{t("显示更多")}</Button>}
      {backups.data?.length === 0 && <p className="page-description">{t("尚无外部文件写入记录")}</p>}
    </section></div>}
    {deleting && <Modal open className="gov-confirm-modal" title={t("删除备份")} busy={op.busy} onClose={() => { setDeleting(null); setDeleteWord(""); }} footer={<div className="gov-actions"><Button variant="danger" disabled={op.busy || deleteWord !== "DELETE BACKUP"} onClick={() => void op.run(async () => { await deleteMountBackup(deleting.id, deleteWord); setDeleting(null); setDeleteWord(""); clearPreview(); await op.refresh(); }, t("备份已删除"))}>{t("永久删除此备份")}</Button><Button variant="quiet" disabled={op.busy} onClick={() => { setDeleting(null); setDeleteWord(""); }}>{t("取消")}</Button></div>}><div className="gov-stack"><OperationMessage op={op} /><div className="gov-confirm-subject"><strong>{t("备份历史")}</strong><code className="gov-id" tabIndex={0}>{deleting.target}</code><code className="gov-id" tabIndex={0}>{deleting.id}</code><time>{stamp(deleting.createdAt)}</time></div><p className="gov-warning">{t("只删除备份，不改动外部文件；删除后不能再用此备份恢复原文。")}</p><Confirmation word="DELETE BACKUP" value={deleteWord} onChange={setDeleteWord} disabled={op.busy} /></div></Modal>}
  </div>;
}
