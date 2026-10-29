import { useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { t } from "../../i18n/language";
import { applyExplicitMount, chooseMountRoot, deleteMountBackup, listMountBackups, previewExplicitMount, previewMountRollback } from "../../lib/governance-service";
import { loadPrompts } from "../../lib/prompt-service";
import { getSkillCatalog } from "../../lib/skill-service";
import type { MountBackup, MountClient, MountPreview, MountRoot } from "../../types/governance";
import { Badge, Button, Card, Field, InlineError, Select } from "../../ui";
import { Confirmation, DiffPreview, OperationMessage, stamp, useClock, useGovernanceOperation } from "./shared";

export function MountsPanel() {
  const prompts = useQuery({ queryKey: ["prompts"], queryFn: loadPrompts });
  const skills = useQuery({ queryKey: ["governance", "mount-skills"], queryFn: getSkillCatalog });
  const backups = useQuery({ queryKey: ["governance", "mount-backups"], queryFn: listMountBackups });
  const [root, setRoot] = useState<MountRoot | null>(null), [client, setClient] = useState<MountClient>("codex");
  const [promptIds, setPromptIds] = useState<string[]>([]), [skillIds, setSkillIds] = useState<string[]>([]);
  const [preview, setPreview] = useState<MountPreview | null>(null), [confirmation, setConfirmation] = useState(""), [reviewed, setReviewed] = useState(false);
  const [deleting, setDeleting] = useState<MountBackup | null>(null), [deleteWord, setDeleteWord] = useState(""), [visibleCount, setVisibleCount] = useState(30);
  const op = useGovernanceOperation(), now = useClock(), selectedCount = promptIds.length + skillIds.length;
  const clearPreview = () => { setPreview(null); setConfirmation(""); setReviewed(false); };
  const chooseSource = (kind: "prompt" | "skill", id: string) => {
    clearPreview(); const update = (current: string[]) => current.includes(id) ? current.filter(v => v !== id) : [...current, id];
    if (kind === "prompt") setPromptIds(update); else setSkillIds(update);
  };
  return <div className="gov-stack"><OperationMessage op={op} />
    <Card className="gov-card"><h2>{t("提示词 / Skills 显式挂载")}</h2><p className="gov-warning">{t("不会自动写 CLAUDE.md、AGENTS.md 或 GEMINI.md。每次都必须选目录、选择来源、预览 diff 并明确确认；外部文件只在确认后变更。")}</p>
      <fieldset disabled={op.busy} className="gov-fieldset">
        <div className="gov-actions"><Button variant="secondary" onClick={() => { clearPreview(); void op.run(async () => { const selected = await chooseMountRoot(); if (selected) setRoot(selected); }); }}>{t("选择项目目录")}</Button><code className="gov-path">{root?.path ?? t("尚未选择目录")}</code></div>
        <Field label={t("挂载目标")}><Select value={client} onChange={e => { setClient(e.target.value as MountClient); clearPreview(); }}><option value="claude">CLAUDE.md</option><option value="codex">AGENTS.md</option><option value="gemini">GEMINI.md</option></Select></Field>
        <p className="page-description">{t("最多选择 20 项。Skills 只读取已显式开启扫描的本地来源；仅挂载 SKILL.md 内容，不复制脚本、资源或安装依赖。")}</p>
        <div className="gov-grid"><fieldset className="gov-sources"><legend>{t("已保存提示词")}</legend>{prompts.error && <InlineError message={String(prompts.error)} onRetry={() => void prompts.refetch()} />}{prompts.data?.map(p => <label className="gov-check" key={p.id}><input type="checkbox" checked={promptIds.includes(p.id)} disabled={!promptIds.includes(p.id) && selectedCount >= 20} onChange={() => chooseSource("prompt", p.id)} /><span>{p.name}</span></label>)}{prompts.data?.length === 0 && <p>{t("没有可挂载的提示词")}</p>}</fieldset>
          <fieldset className="gov-sources"><legend>{t("本地 Skills")}</legend>{skills.error && <InlineError message={String(skills.error)} onRetry={() => void skills.refetch()} />}{skills.data?.skills.map(skill => <label className="gov-check" key={skill.id}><input type="checkbox" checked={skillIds.includes(skill.id)} disabled={!skillIds.includes(skill.id) && selectedCount >= 20} onChange={() => chooseSource("skill", skill.id)} /><span>{skill.name}<small>{skill.source}</small></span></label>)}{skills.data?.skills.length === 0 && <p>{t("没有可挂载的 Skills；可先在 Skills 页面显式开启来源。")}</p>}{skills.data?.warnings.length ? <p className="gov-warning">{t("部分 Skills 来源不可用，请到 Skills 页面查看详情。")}</p> : null}</fieldset></div>
        <Button disabled={!root || selectedCount < 1 || selectedCount > 20} onClick={() => { if (!root) return; clearPreview(); void op.run(async () => { setPreview(await previewExplicitMount(root.id, client, promptIds, skillIds)); }); }}>{t("只预览，不写入")}</Button>
      </fieldset>
    </Card>
    {preview && <Card className="gov-card"><h2>{t("文件变更预览")}</h2><code className="gov-path">{preview.target}</code><div className="gov-actions">{preview.createsFile && <Badge tone="accent">{t("将创建文件")}</Badge>}{preview.removesFile && <Badge tone="danger">{t("回滚将删除新建文件")}</Badge>}<Badge tone={now >= preview.expiresAt ? "danger" : "neutral"}>{now >= preview.expiresAt ? t("预览已过期，请重新生成") : t("预览有效至") + " " + stamp(preview.expiresAt)}</Badge></div>
      <DiffPreview before={preview.before} after={preview.after} />
      <p className="page-description">{t("保留管理块外的手写内容。写入前会复查原文、完整备份并原子替换；文件被外部修改后必须重新预览。备份存于本地数据库，不是跨设备同步。")}</p>
      <label className="gov-check"><input type="checkbox" disabled={op.busy} checked={reviewed} onChange={e => setReviewed(e.target.checked)} />{t("我已查看上述 diff")}</label><Confirmation word="WRITE FILE" value={confirmation} onChange={setConfirmation} disabled={op.busy} />
      <div className="gov-actions"><Button disabled={op.busy || !reviewed || confirmation !== "WRITE FILE" || now >= preview.expiresAt} onClick={() => void op.run(async () => { await applyExplicitMount(preview.id, confirmation); clearPreview(); await op.refresh(); }, t("文件已按预览更新，原文备份已保留"))}>{t("备份并应用文件变更")}</Button><Button variant="quiet" disabled={op.busy} onClick={clearPreview}>{t("取消")}</Button></div>
    </Card>}
    <Card className="gov-card"><h2>{t("外部文件备份与回滚")}</h2><p className="page-description">{t("回滚前必须重新选中原项目目录并预览。若文件之后被外部修改，不会强行覆盖。最多保留 1000 份备份，满额后不再写文件。")}</p>
      {backups.error && <InlineError message={String(backups.error)} onRetry={() => void backups.refetch()} />}
      <ul className="gov-list">{backups.data?.slice(0, visibleCount).map(backup => <li key={backup.id}><div><code className="gov-path">{backup.target}</code><p>{stamp(backup.createdAt)}</p></div><div className="gov-actions"><Button variant="secondary" disabled={!root || op.busy} onClick={() => { if (!root) return; clearPreview(); void op.run(async () => { setPreview(await previewMountRollback(root.id, backup.id)); }); }}>{t("预览文件回滚")}</Button><Button variant="danger" disabled={op.busy} onClick={() => { setDeleting(backup); setDeleteWord(""); }}>{t("删除备份")}</Button></div></li>)}</ul>
      {(backups.data?.length ?? 0) > visibleCount && <Button variant="quiet" onClick={() => setVisibleCount(v => v + 50)}>{t("显示更多")}</Button>}
      {backups.data?.length === 0 && <p className="page-description">{t("尚无外部文件写入记录")}</p>}
    </Card>
    {deleting && <Card className="gov-card"><h2>{t("删除备份")}</h2><code className="gov-path">{deleting.target}</code><p className="gov-warning">{t("只删除备份，不改动外部文件；删除后不能再用此备份恢复原文。")}</p><Confirmation word="DELETE BACKUP" value={deleteWord} onChange={setDeleteWord} disabled={op.busy} /><div className="gov-actions"><Button variant="danger" disabled={op.busy || deleteWord !== "DELETE BACKUP"} onClick={() => void op.run(async () => { await deleteMountBackup(deleting.id, deleteWord); setDeleting(null); clearPreview(); await op.refresh(); }, t("备份已删除"))}>{t("永久删除此备份")}</Button><Button variant="quiet" disabled={op.busy} onClick={() => setDeleting(null)}>{t("取消")}</Button></div></Card>}
  </div>;
}
