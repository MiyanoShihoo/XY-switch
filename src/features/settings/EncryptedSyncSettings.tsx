import { useEffect, useRef, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { t } from "../../i18n/language";
import { usePlatformCapabilities } from "../../lib/platform-service";
import { applySyncImport, applySyncRollback, cancelSyncPreview, listSyncBackups, previewSyncExport, previewSyncImport, previewSyncRollback, saveSyncExport, validSyncPassphrase } from "../../lib/encrypted-sync-service";
import { refreshNativeFavorites } from "../../lib/native-favorites-service";
import type { SyncChange, SyncPayload, SyncPreview, SyncRollback } from "../../types/encrypted-sync";
import { Button, Card, Field, InlineError, Input, Select } from "../../ui";
import { Confirmation, DiffPreview, stamp, useClock } from "../governance/shared";
import { usePageOperation } from "../shared/usePageOperation";
import "../governance/governance.css";
import "./encrypted-sync.css";

type Active = { kind: "export"; data: SyncPreview<SyncPayload> } | { kind: "import"; data: SyncPreview<SyncChange[]> } | { kind: "rollback"; data: SyncPreview<SyncRollback> };
const json = (value: unknown) => JSON.stringify(value, null, 2);
function effect(value: string) {
  switch (value) {
    case "new_disabled_provider": return t("创建新的禁用供应商；需要在本机重新配置凭据。");
    case "keep_old_target_create_disabled_copy": return t("目标地址或协议已变化：保留旧供应商及凭据，另建禁用副本。");
    case "update_allowed_fields_only": return t("仅修改白名单字段；保留本机凭据和高级配置。");
    case "custom_price_only_history_unchanged": return t("只更新自定义价格，不重算历史费用。");
    default: return t("只合并收藏标识，不包含会话内容，也不传播删除。");
  }
}
function ChangeRow({ change, value, disabled, onChange }: { change: SyncChange; value: boolean | undefined; disabled: boolean; onChange: (value: boolean | undefined) => void }) {
  const [open, setOpen] = useState(false);
  return <article className="sync-change">
    <details open={open} onToggle={event => setOpen(event.currentTarget.open)}><summary><strong>{change.status === "add" ? t("新增") : change.status === "conflict" ? t("冲突") : t("内容没有变化")}</strong> <code>{change.key}</code></summary>
      {open && <><p>{effect(change.effect)}</p><DiffPreview before={json(change.local)} after={json(change.incoming)} /></>}
    </details>
    {change.status !== "unchanged" && <Field label={t("处理方式") + " · " + change.key}><Select value={value === undefined ? "" : value ? "incoming" : "local"} disabled={disabled} onChange={event => onChange(event.target.value === "" ? undefined : event.target.value === "incoming")}><option value="">{t("请选择")}</option><option value="local">{t("保留本地 / 跳过")}</option><option value="incoming">{t("接受传入项")}</option></Select></Field>}
  </article>;
}

export function EncryptedSyncSettings({ proxyStopped, onImported }: { proxyStopped: boolean; onImported: () => Promise<void> }) {
  const platform = usePlatformCapabilities();
  const readOnly = !platform.data || platform.data.os === "browser" || platform.data.mobile;
  const query = useQuery({ queryKey: ["encrypted-sync", "backups"], queryFn: listSyncBackups, enabled: !readOnly, retry: false });
  const op = usePageOperation(), now = useClock();
  const [active, setActive] = useState<Active | null>(null), [reviewed, setReviewed] = useState(false), [confirmation, setConfirmation] = useState("");
  const [choices, setChoices] = useState<Record<string, boolean | undefined>>({}), [page, setPage] = useState(0);
  const pass = useRef<HTMLInputElement>(null), repeated = useRef<HTMLInputElement>(null);
  const [validPass, setValidPass] = useState(false), [matching, setMatching] = useState(false);
  const mounted = useRef(true), current = useRef<Active | null>(null);
  const clearSecrets = () => { if (pass.current) pass.current.value = ""; if (repeated.current) repeated.current.value = ""; setValidPass(false); setMatching(false); };
  const checkSecrets = () => { setValidPass(validSyncPassphrase(pass.current?.value ?? "")); setMatching(pass.current?.value === repeated.current?.value); };
  const clear = () => { const previous = current.current; current.current = null; setActive(null); setReviewed(false); setConfirmation(""); setChoices({}); setPage(0); clearSecrets(); if (previous) void cancelSyncPreview(previous.data.ticketId).catch(() => undefined); };
  const install = (next: Active) => {
    if (!mounted.current) { void cancelSyncPreview(next.data.ticketId).catch(() => undefined); return; }
    current.current = next; setActive(next); setReviewed(false); setConfirmation(""); setChoices({}); setPage(0); clearSecrets();
  };
  useEffect(() => { mounted.current = true; return () => { mounted.current = false; const preview = current.current; current.current = null; if (preview) void cancelSyncPreview(preview.data.ticketId).catch(() => undefined); }; }, []);
  useEffect(() => { if (active && !op.busy && now >= active.data.expiresAt) { clear(); op.setError(t("同步预览已过期，请重新预览。")); } });
  const refresh = async () => {
    const results = await Promise.allSettled([refreshNativeFavorites(), onImported(), query.refetch({ throwOnError: true })]);
    if (results.some(result => result.status === "rejected")) op.setError(t("数据已保存，但界面刷新失败；请刷新后再操作。"));
  };
  const changes = active?.kind === "import" ? active.data.content.filter(row => row.status !== "unchanged") : [];
  const allChosen = changes.length > 0 && changes.every(row => choices[row.key] !== undefined);
  const incomingCount = changes.filter(row => choices[row.key] === true).length;
  const blocked = op.busy || Boolean(active) || readOnly;
  const word = active?.kind === "export" ? "EXPORT" : active?.kind === "import" ? "IMPORT" : "ROLLBACK SYNC";
  const confirmed = reviewed && confirmation === word && Boolean(active && now < active.data.expiresAt);
  return <div className="encrypted-sync-settings gov-stack">
    <Card className="gov-card"><h2>{t("加密同步")}</h2><p>{t("手动加密文件交换，不是自动云同步。仅合并供应商白名单配置、收藏标识和自定义价格；不传播删除。")} </p>
      <p className="gov-warning">{t("不访问系统凭据存储；同步文件不包含请求头 / 请求体覆盖、TOML、MCP、提示词、会话正文和安全策略。名称与 URL 路径是自定义文本，仍需完整预览，避免手动嵌入秘密。")}</p>
      <p className="page-description">{t("AES-256-GCM + PBKDF2-SHA256（600000 次）。口令不保存；文件与口令请通过不同渠道传递，遗失口令无法恢复。")}</p>
      {readOnly ? <p role="status">{t("此设备为只读视图；加密文件导入、导出和回滚仅限桌面应用。不会自动连接其他设备。")}</p> : <>
        <p>{t("口令至少 12 个字符、最多 1024 个 UTF-8 字节。选择文件不代表允许导入；必须先预览并逐项确认。")}</p>
        <div className="sync-secret-fields"><Field label={t("加密口令")}><Input ref={pass} type="password" autoComplete="off" spellCheck={false} disabled={op.busy || Boolean(active && active.kind !== "export")} onChange={checkSecrets} /></Field>
          {active?.kind === "export" && <Field label={t("再次输入加密口令")}><Input ref={repeated} type="password" autoComplete="off" spellCheck={false} disabled={op.busy} onChange={checkSecrets} /></Field>}
        </div><div className="gov-actions"><Button disabled={blocked} onClick={() => void op.run(async () => install({ kind: "export", data: await previewSyncExport() }))}>{t("预览加密导出")}</Button>
          <Button variant="secondary" disabled={blocked || !validPass} onClick={() => { const secret = pass.current?.value ?? ""; clearSecrets(); void op.run(async () => { const data = await previewSyncImport(secret); if (data) install({ kind: "import", data }); else op.setMessage(t("已取消文件选择")); }); }}>{t("选择加密文件并预览")}</Button></div>
      </>}
    </Card>
    {op.error && <InlineError message={op.error} />}{op.message && <p role="status" className="gov-success">{op.message}</p>}
    {platform.error && <InlineError message={String(platform.error)} />}
    {!readOnly && active && <Card className="gov-card"><h2>{active.kind === "export" ? t("加密导出预览") : active.kind === "import" ? t("加密导入预览") : t("同步回滚预览")}</h2><p>{t("预览有效期至")} {stamp(active.data.expiresAt)}</p>
      {active.kind === "export" && <DiffPreview before="{}" after={json(active.data.content)} />}
      {active.kind === "import" && <><p>{t("变更 {count} 项；将接受 {accepted} 项。", { count: changes.length, accepted: incomingCount })}</p>
        <p className="gov-warning">{t("目标地址或协议改变时，不会将本机旧凭据交给新目标：保留旧供应商，另建禁用副本。导入后需手动配置凭据并启用。")}</p>
        <div className="gov-actions"><Button variant="secondary" disabled={op.busy} onClick={() => { setChoices(Object.fromEntries(changes.map(row => [row.key, false]))); setReviewed(false); }}>{t("全部保留本地")}</Button><Button variant="secondary" disabled={op.busy} onClick={() => { setChoices(Object.fromEntries(changes.map(row => [row.key, row.status === "add"]))); setReviewed(false); }}>{t("仅接受新增项")}</Button></div>
        {active.data.content.slice(page * 50, page * 50 + 50).map(row => <ChangeRow key={row.key} change={row} value={choices[row.key]} disabled={op.busy} onChange={value => { setChoices(previous => ({ ...previous, [row.key]: value })); setReviewed(false); setConfirmation(""); }} />)}
        {active.data.content.length > 50 && <div className="gov-actions"><Button variant="quiet" disabled={page === 0 || op.busy} onClick={() => setPage(page - 1)}>{t("上一页")}</Button><span>{page + 1} / {Math.ceil(active.data.content.length / 50)}</span><Button variant="quiet" disabled={(page + 1) * 50 >= active.data.content.length || op.busy} onClick={() => setPage(page + 1)}>{t("下一页")}</Button></div>}
        {!changes.length && <p>{t("没有需要合并的变更。")}</p>}
      </>}
      {active.kind === "rollback" && <DiffPreview before={json(active.data.content.before)} after={json(active.data.content.after)} />}
      {active.kind !== "export" && <p className="gov-warning">{t("应用变更前必须停止代理并恢复所有接管。保留最近 10 次本地备份；回滚拒绝覆盖导入后的新改动。新增供应商回滚后保留为禁用，不删除用量历史。")}</p>}
      <label className="gov-check"><input type="checkbox" disabled={op.busy} checked={reviewed} onChange={event => setReviewed(event.target.checked)} />{t("我已完整检查预览和处理方式")}</label>
      <Confirmation word={word} value={confirmation} onChange={setConfirmation} disabled={op.busy} />
      <div className="gov-actions">
        {active.kind === "export" && <Button disabled={op.busy || !confirmed || !validPass || !matching} onClick={() => { const secret = pass.current?.value ?? ""; clearSecrets(); void op.run(async () => { try { const result = await saveSyncExport(active.data.ticketId, secret, confirmation); op.setMessage(result.saved ? t("加密文件已保存；现有文件从不覆盖。") : t("已取消文件选择")); if (result.saved && !result.auditRecorded) op.setError(t("文件已保存，但完成审计写入失败。")); } finally { clear(); } }); }}>{t("确认并另存加密文件")}</Button>}
        {active.kind === "import" && <Button disabled={op.busy || !confirmed || !allChosen || incomingCount === 0 || !proxyStopped} onClick={() => void op.run(async () => { try { const result = await applySyncImport(active.data.ticketId, changes.map(row => ({ key: row.key, useIncoming: choices[row.key] === true })), confirmation); op.setMessage(t("已合并 {count} 项，创建 {disabled} 个禁用供应商副本，并保留本地备份。", { count: result.applied, disabled: result.disabledCopies })); await refresh(); } finally { clear(); } })}>{t("确认合并所选变更")}</Button>}
        {active.kind === "rollback" && <Button variant="danger" disabled={op.busy || !confirmed || !proxyStopped} onClick={() => void op.run(async () => { try { await applySyncRollback(active.data.ticketId, confirmation); op.setMessage(t("同步变更已回滚；新增供应商保留为禁用。")); await refresh(); } finally { clear(); } })}>{t("确认回滚同步")}</Button>}
        <Button variant="quiet" disabled={op.busy} onClick={clear}>{t("取消并清除预览")}</Button>
      </div>
    </Card>}
    {!readOnly && <Card className="gov-card"><div className="section-heading"><h2>{t("同步本地备份")}</h2><Button variant="quiet" disabled={op.busy || query.isFetching} onClick={() => void query.refetch()}>{t("刷新")}</Button></div>
      <p className="page-description">{t("备份只保存在本机数据库，包含回滚所需的本机高级配置，可能含自行嵌入的敏感文本；不是加密同步文件，也不会被导出。")}</p>
      {query.error && <InlineError message={String(query.error)} />}
      {query.data?.length === 0 && <p>{t("还没有同步导入备份")}</p>}
      <ul className="gov-list">{query.data?.map(backup => <li key={backup.id}><div><strong>{stamp(backup.createdAt)}</strong><p><code>{backup.id}</code>{backup.used && <> · {t("已回滚")}</>}</p></div><Button variant="secondary" disabled={blocked || backup.used} onClick={() => void op.run(async () => install({ kind: "rollback", data: await previewSyncRollback(backup.id) }))}>{t("预览同步回滚")}</Button></li>)}</ul>
    </Card>}
  </div>;
}
