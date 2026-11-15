import { useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { t } from "../../i18n/language";
import { applyConfigRollback, createConfigVersion, deleteConfigVersion, listConfigVersions, previewConfigRollback } from "../../lib/governance-service";
import type { ConfigVersion, VersionPreview } from "../../types/governance";
import { Button, Field, InlineError, Input, Modal } from "../../ui";
import { openRoutingSettings, useTaskDraft, useTaskNavigation } from "../settings/TaskNavigation";
import { Confirmation, DiffPreview, OperationMessage, stamp, useGovernanceOperation } from "./shared";

export function VersionsPanel({ proxyStopped, onRestored }: { proxyStopped: boolean; onRestored: () => Promise<void> }) {
  const [task, setTask] = useState<"restore" | "create" | "manage">("restore");
  const { leave } = useTaskNavigation();
  const query = useQuery({ queryKey: ["governance", "versions"], queryFn: listConfigVersions });
  const [label, setLabel] = useState(""), [preview, setPreview] = useState<VersionPreview | null>(null), [confirmation, setConfirmation] = useState("");
  const [reviewed, setReviewed] = useState(false), [deleting, setDeleting] = useState<ConfigVersion | null>(null), [deleteWord, setDeleteWord] = useState("");
  const op = useGovernanceOperation();
  useTaskDraft(Boolean(label || preview || deleting || confirmation || deleteWord), op.busy);
  const clearPreview = () => { setPreview(null); setConfirmation(""); setReviewed(false); };
  return <div className="gov-stack">{!preview && !deleting && <OperationMessage op={op} />}
    <div className="gov-actions" role="group" aria-label={t("配置版本任务")}>{([{ value: "restore", label: t("恢复配置") }, { value: "create", label: t("保存当前版本") }, { value: "manage", label: t("管理版本") }] as const).map(item => <Button key={item.value} variant="quiet" aria-pressed={task === item.value} disabled={op.busy} onClick={() => leave(() => { clearPreview(); setDeleting(null); setDeleteWord(""); setLabel(""); setTask(item.value); })}>{item.label}</Button>)}</div>
    <section className="gov-card"><h2>{task === "restore" ? t("选择要恢复的配置版本") : task === "create" ? t("保存当前版本") : t("管理版本")}</h2>
      <p className="page-description">{t("手动快照包含供应商、提示词、自定义价格、当前供应商和代理设置；不包含凭据存储、MCP、收藏、外部文件或安全策略。不会自动为每次编辑创建版本。")}</p>
      <p className="gov-warning">{t("快照与 diff 可能包含配置文本中的敏感内容；请勿在 endpoint、提示词或自定义文本中内嵌凭据。它不是加密同步包。")}</p>
      {task === "restore" && <section className="gov-prerequisites"><h3>{t("恢复前检查")}</h3><p>{t("先选择已有版本，再比较当前配置与目标版本。确认回滚会覆盖快照范围，并自动备份当前配置。")}</p><p>{proxyStopped ? t("路由已停止；仍需在本地路由中检查并恢复客户端接管。") : t("请先停止代理，并在设置中恢复接管。")}</p><Button variant="quiet" disabled={op.busy} onClick={() => leave(openRoutingSettings)}>{t("前往本地路由检查")}</Button></section>}
      {task === "create" && <section className="gov-group" aria-label={t("创建配置快照")}>
      <div className="gov-inline-form"><Field label={t("版本说明")}><Input maxLength={160} value={label} onChange={e => setLabel(e.target.value)} disabled={op.busy} /></Field><Button disabled={op.busy || !label.trim()} onClick={() => void op.run(async () => { await createConfigVersion(label); setLabel(""); await op.refresh(); }, t("配置版本已创建"))}>{t("创建配置快照")}</Button></div>
      <p className="page-description">{t("最多保留 200 个版本，单个不超过 8 MiB；空间满时先明确删除旧版本。回滚会在同一事务中自动备份当前配置。")}</p>
      </section>}
      {query.error && <InlineError message={String(query.error)} onRetry={() => void query.refetch()} />}
      {query.isPending && <p role="status">{t("读取中")}</p>}
      {task !== "create" && <ul className="gov-list">{query.data?.map(version => <li key={version.id}><div><strong>{version.label}</strong><p>{stamp(version.createdAt)} · <code>{version.hash.slice(0, 12)}</code></p></div><div className="gov-actions">{task === "restore" && <Button variant="secondary" disabled={op.busy} onClick={() => { clearPreview(); setDeleting(null); void op.run(async () => { setPreview(await previewConfigRollback(version.id)); }); }}>{t("预览回滚差异")}</Button>}{task === "manage" && <Button variant="danger" disabled={op.busy} onClick={() => { clearPreview(); setDeleting(version); setDeleteWord(""); op.setError(""); op.setMessage(""); }}>{t("删除版本")}</Button>}</div></li>)}</ul>}
      {query.data?.length === 0 && <p className="page-description">{t("还没有配置快照")}</p>}
    </section>
    {preview && <Modal open className="gov-review-modal" title={t("回滚预览")} size="lg" busy={op.busy} onClose={clearPreview} footer={<div className="gov-actions"><Button variant="danger" disabled={op.busy || !reviewed || confirmation !== "ROLLBACK" || !proxyStopped} onClick={() => void op.run(async () => { await applyConfigRollback(preview, confirmation); clearPreview(); await onRestored(); await op.refresh(); }, t("已回滚配置，并创建了回滚前备份"))}>{t("确认回滚配置")}</Button><Button variant="quiet" disabled={op.busy} onClick={clearPreview}>{t("取消")}</Button></div>}><div className="gov-review"><OperationMessage op={op} /><div className="gov-review__layout">
      <div className="gov-review__document"><DiffPreview identity={preview.versionId} before={preview.before} after={preview.after} /></div>
      <aside className="gov-review__approval" aria-label={t("最终确认")}><h3>{t("最终确认")}</h3><div className="gov-confirm-subject"><span>{t("配置版本")}</span><strong>{query.data?.find(v => v.id === preview.versionId)?.label}</strong><code className="gov-id" tabIndex={0}>{preview.versionId}</code></div>
      <p className="page-description">{t("回滚会在同一事务中自动备份当前配置。")}</p><p className="gov-warning">{t("新增加的供应商不会被删除，而是保留并停用，避免丢失历史。代理恢复后保持停止。预览后配置变化会阻止回滚。")}</p>{!proxyStopped && <p className="gov-warning">{t("请先停止代理，并在设置中恢复接管。")}</p>}
      <div className="gov-approval"><label className="gov-check"><input type="checkbox" disabled={op.busy} checked={reviewed} onChange={e => setReviewed(e.target.checked)} />{t("我已查看上述 diff")}</label><Confirmation word="ROLLBACK" value={confirmation} onChange={setConfirmation} disabled={op.busy} /></div>
      </aside></div></div></Modal>}
    {deleting && <Modal open className="gov-confirm-modal" title={t("删除版本")} busy={op.busy} onClose={() => { setDeleting(null); setDeleteWord(""); }} footer={<div className="gov-actions"><Button variant="danger" disabled={op.busy || deleteWord !== "DELETE VERSION"} onClick={() => void op.run(async () => { await deleteConfigVersion(deleting.id, deleteWord); if (preview?.versionId === deleting.id) clearPreview(); setDeleting(null); setDeleteWord(""); await op.refresh(); }, t("版本已删除"))}>{t("永久删除此版本")}</Button><Button variant="quiet" disabled={op.busy} onClick={() => { setDeleting(null); setDeleteWord(""); }}>{t("取消")}</Button></div>}><div className="gov-stack"><OperationMessage op={op} /><div className="gov-confirm-subject"><strong>{deleting.label}</strong><code className="gov-id" tabIndex={0}>{deleting.id}</code><time>{stamp(deleting.createdAt)}</time></div><p className="gov-warning">{t("仅删除此快照，不改变当前配置；删除后无法再从此版本回滚。")}</p><Confirmation word="DELETE VERSION" value={deleteWord} onChange={setDeleteWord} disabled={op.busy} /></div></Modal>}
  </div>;
}
