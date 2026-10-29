import { useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { t } from "../../i18n/language";
import { applyConfigRollback, createConfigVersion, deleteConfigVersion, listConfigVersions, previewConfigRollback } from "../../lib/governance-service";
import type { ConfigVersion, VersionPreview } from "../../types/governance";
import { Button, Card, Field, InlineError, Input } from "../../ui";
import { Confirmation, DiffPreview, OperationMessage, stamp, useGovernanceOperation } from "./shared";

export function VersionsPanel({ proxyStopped, onRestored }: { proxyStopped: boolean; onRestored: () => Promise<void> }) {
  const query = useQuery({ queryKey: ["governance", "versions"], queryFn: listConfigVersions });
  const [label, setLabel] = useState(""), [preview, setPreview] = useState<VersionPreview | null>(null), [confirmation, setConfirmation] = useState("");
  const [reviewed, setReviewed] = useState(false), [deleting, setDeleting] = useState<ConfigVersion | null>(null), [deleteWord, setDeleteWord] = useState("");
  const op = useGovernanceOperation();
  const clearPreview = () => { setPreview(null); setConfirmation(""); setReviewed(false); };
  return <div className="gov-stack"><OperationMessage op={op} />
    <Card className="gov-card"><h2>{t("配置版本与回滚")}</h2>
      <p className="page-description">{t("手动快照包含供应商、提示词、自定义价格、当前供应商和代理设置；不包含凭据存储、MCP、收藏、外部文件或安全策略。不会自动为每次编辑创建版本。")}</p>
      <p className="gov-warning">{t("快照与 diff 可能包含配置文本中的敏感内容；请勿在 endpoint、提示词或自定义文本中内嵌凭据。它不是加密同步包。")}</p>
      <div className="gov-inline-form"><Field label={t("版本说明")}><Input maxLength={160} value={label} onChange={e => setLabel(e.target.value)} disabled={op.busy} /></Field><Button disabled={op.busy || !label.trim()} onClick={() => void op.run(async () => { await createConfigVersion(label); setLabel(""); await op.refresh(); }, t("配置版本已创建"))}>{t("创建配置快照")}</Button></div>
      <p className="page-description">{t("最多保留 200 个版本，单个不超过 8 MiB；空间满时先明确删除旧版本。回滚会在同一事务中自动备份当前配置。")}</p>
      {query.error && <InlineError message={String(query.error)} onRetry={() => void query.refetch()} />}
      <ul className="gov-list">{query.data?.map(version => <li key={version.id}><div><strong>{version.label}</strong><p>{stamp(version.createdAt)} · <code>{version.hash.slice(0, 12)}</code></p></div><div className="gov-actions"><Button variant="secondary" disabled={op.busy} onClick={() => { clearPreview(); void op.run(async () => { setPreview(await previewConfigRollback(version.id)); }); }}>{t("预览回滚差异")}</Button><Button variant="danger" disabled={op.busy} onClick={() => { setDeleting(version); setDeleteWord(""); }}>{t("删除版本")}</Button></div></li>)}</ul>
      {query.data?.length === 0 && <p className="page-description">{t("还没有配置快照")}</p>}
    </Card>
    {preview && <Card className="gov-card"><h2>{t("回滚预览")}</h2><p>{query.data?.find(v => v.id === preview.versionId)?.label}</p><p className="page-description">{t("新增加的供应商不会被删除，而是保留并停用，避免丢失历史。代理恢复后保持停止。预览后配置变化会阻止回滚。")}</p><DiffPreview before={preview.before} after={preview.after} />
      <label className="gov-check"><input type="checkbox" disabled={op.busy} checked={reviewed} onChange={e => setReviewed(e.target.checked)} />{t("我已查看上述 diff")}</label><Confirmation word="ROLLBACK" value={confirmation} onChange={setConfirmation} disabled={op.busy} />
      <div className="gov-actions"><Button variant="danger" disabled={op.busy || !reviewed || confirmation !== "ROLLBACK" || !proxyStopped} onClick={() => void op.run(async () => { await applyConfigRollback(preview, confirmation); clearPreview(); await onRestored(); await op.refresh(); }, t("已回滚配置，并创建了回滚前备份"))}>{t("确认回滚配置")}</Button><Button variant="quiet" disabled={op.busy} onClick={clearPreview}>{t("取消")}</Button></div>
      {!proxyStopped && <p className="gov-warning">{t("请先停止代理，并在设置中恢复接管。")}</p>}
    </Card>}
    {deleting && <Card className="gov-card"><h2>{t("删除版本")}</h2><strong>{deleting.label}</strong><p className="gov-warning">{t("仅删除此快照，不改变当前配置；删除后无法再从此版本回滚。")}</p><Confirmation word="DELETE VERSION" value={deleteWord} onChange={setDeleteWord} disabled={op.busy} /><div className="gov-actions"><Button variant="danger" disabled={op.busy || deleteWord !== "DELETE VERSION"} onClick={() => void op.run(async () => { await deleteConfigVersion(deleting.id, deleteWord); if (preview?.versionId === deleting.id) clearPreview(); setDeleting(null); await op.refresh(); }, t("版本已删除"))}>{t("永久删除此版本")}</Button><Button variant="quiet" disabled={op.busy} onClick={() => setDeleting(null)}>{t("取消")}</Button></div></Card>}
  </div>;
}
