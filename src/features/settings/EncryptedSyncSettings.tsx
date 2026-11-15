import { useEffect, useRef, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { t } from "../../i18n/language";
import { usePlatformCapabilities } from "../../lib/platform-service";
import { applySyncImport, applySyncRollback, cancelSyncPreview, listSyncBackups, previewSyncExport, previewSyncImport, previewSyncRollback, saveSyncExport, validSyncPassphrase } from "../../lib/encrypted-sync-service";
import { refreshNativeFavorites } from "../../lib/native-favorites-service";
import type { SyncBackup, SyncChange, SyncPayload, SyncPreview, SyncRollback } from "../../types/encrypted-sync";
import { Button, Field, InlineError, Input, Modal, Tabs } from "../../ui";
import { useTaskDraft, useTaskNavigation } from "./TaskNavigation";
import { SyncImportReview } from "./SyncImportReview";
import { Confirmation, DiffPreview, stamp, useClock } from "../governance/shared";
import { usePageOperation } from "../shared/usePageOperation";
import "../governance/governance.css";
import "./encrypted-sync.css";

type Active = { kind: "export"; data: SyncPreview<SyncPayload> } | { kind: "import"; data: SyncPreview<SyncChange[]> } | { kind: "rollback"; data: SyncPreview<SyncRollback>; backup: SyncBackup };
const json = (value: unknown) => JSON.stringify(value, null, 2);
const reviewPageSize = 8;

export function EncryptedSyncSettings({ proxyStopped, onImported }: { proxyStopped: boolean; onImported: () => Promise<void> }) {
  const platform = usePlatformCapabilities();
  const readOnly = !platform.data || platform.data.os === "browser" || platform.data.mobile;
  const { leave } = useTaskNavigation();
  const [hasSecret, setHasSecret] = useState(false);
  const [view, setView] = useState("exchange");
  const query = useQuery({ queryKey: ["encrypted-sync", "backups"], queryFn: listSyncBackups, enabled: !readOnly && view === "backups", retry: false });
  const op = usePageOperation(), now = useClock();
  const [active, setActive] = useState<Active | null>(null), [reviewed, setReviewed] = useState(false), [confirmation, setConfirmation] = useState("");
  const [choices, setChoices] = useState<Record<string, boolean | undefined>>({}), [page, setPage] = useState(0);
  const pass = useRef<HTMLInputElement>(null), repeated = useRef<HTMLInputElement>(null);
  const [validPass, setValidPass] = useState(false), [matching, setMatching] = useState(false);
  const mounted = useRef(true), current = useRef<Active | null>(null);
  const clearSecrets = () => { if (pass.current) pass.current.value = ""; if (repeated.current) repeated.current.value = ""; setValidPass(false); setMatching(false); setHasSecret(false); };
  const checkSecrets = () => { setHasSecret(Boolean(pass.current?.value || repeated.current?.value)); setValidPass(validSyncPassphrase(pass.current?.value ?? "")); setMatching(pass.current?.value === repeated.current?.value); };
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
  const localCount = changes.filter(row => choices[row.key] === false).length;
  const undecidedCount = changes.length - incomingCount - localCount;
  useTaskDraft(Boolean(active) || hasSecret, op.busy);
  const blocked = op.busy || Boolean(active) || readOnly;
  const word = active?.kind === "export" ? "EXPORT" : active?.kind === "import" ? "IMPORT" : "ROLLBACK SYNC";
  const confirmed = reviewed && confirmation === word && Boolean(active && now < active.data.expiresAt);
  return <div className="encrypted-sync-settings gov-stack">
    <div className="settings-section__heading"><h2>{t("加密同步")}</h2><p>{t("手动加密文件交换，不是自动云同步。仅合并供应商白名单配置、收藏标识和自定义价格；不传播删除。")}</p></div>
    {!readOnly && <Tabs id="encrypted-sync" label={t("同步工作区")} value={view} onChange={value => leave(() => { clear(); op.setError(""); op.setMessage(""); setView(value); })} items={[{ value: "exchange", label: t("文件交换"), disabled: op.busy || Boolean(active) }, { value: "backups", label: t("本地备份"), disabled: op.busy || Boolean(active) }]} />}
    {readOnly && <p role="status">{t("此设备为只读视图；加密文件导入、导出和回滚仅限桌面应用。不会自动连接其他设备。")}</p>}
    {view === "exchange" && <div className="gov-stack" role={readOnly ? "region" : "tabpanel"} id="encrypted-sync-panel-exchange" aria-label={readOnly ? t("文件交换") : undefined} aria-labelledby={readOnly ? undefined : "encrypted-sync-tab-exchange"}>
      {!readOnly && <div className="settings-operations">
        <article className="settings-operation"><div className="settings-operation__copy"><h3>{t("导出配置")}</h3><p>{t("先检查将导出的配置，再设置加密口令。")}</p></div><Button disabled={blocked} onClick={() => void op.run(async () => install({ kind: "export", data: await previewSyncExport() }))}>{t("预览加密导出")}</Button></article>
        <article className="settings-operation"><div className="settings-operation__copy"><h3>{t("导入配置")}</h3><p>{t("输入文件口令后选择文件；预览不会自动导入。")}</p></div><div className="settings-operation__controls">
          {!active && <Field label={t("加密口令")}><Input ref={pass} type="password" autoComplete="off" spellCheck={false} disabled={blocked} onChange={checkSecrets} /></Field>}
          <Button variant="secondary" disabled={blocked || !validPass} onClick={() => { const secret = pass.current?.value ?? ""; clearSecrets(); void op.run(async () => { const data = await previewSyncImport(secret); if (data) install({ kind: "import", data }); else op.setMessage(t("已取消文件选择")); }); }}>{t("选择加密文件并预览")}</Button>
        </div></article>
      </div>}
      <details className="workspace-disclosure"><summary>{t("交换范围与口令须知")}</summary><div className="workspace-disclosure__body gov-stack">
        <p className="gov-warning">{t("不访问系统凭据存储；同步文件不包含请求头 / 请求体覆盖、TOML、MCP、提示词、会话正文和安全策略。名称与 URL 路径是自定义文本，仍需完整预览，避免手动嵌入秘密。")}</p>
        <p className="page-description">{t("AES-256-GCM + PBKDF2-SHA256（600000 次）。口令不保存；文件与口令请通过不同渠道传递，遗失口令无法恢复。")}</p>
        <p>{t("口令至少 12 个字符、最多 1024 个 UTF-8 字节。选择文件不代表允许导入；必须先预览并逐项确认。")}</p>
      </div></details>
    </div>}
    {op.error && <InlineError message={op.error} />}{op.message && <p role="status" className="gov-success">{op.message}</p>}
    {platform.error && <InlineError message={String(platform.error)} />}
    {!readOnly && active && <Modal open className={"sync-review-modal sync-review-modal--" + active.kind} title={active.kind === "export" ? t("加密导出预览") : active.kind === "import" ? t("加密导入预览") : t("同步回滚预览")} size="lg" busy={op.busy} onClose={clear} footer={<div className="sync-review-footer">{active.kind === "import" && <p className="sync-review-footer__progress" role="status">{t("未决 {undecided} 项 · 合并 {incoming} 项 · 保留本地 {local} 项", { undecided: undecidedCount, incoming: incomingCount, local: localCount })}</p>}<div className="gov-actions">
        {active.kind === "export" && <Button disabled={op.busy || !confirmed || !validPass || !matching} onClick={() => { const secret = pass.current?.value ?? ""; clearSecrets(); void op.run(async () => { try { const result = await saveSyncExport(active.data.ticketId, secret, confirmation); op.setMessage(result.saved ? t("加密文件已保存；现有文件从不覆盖。") : t("已取消文件选择")); if (result.saved && !result.auditRecorded) op.setError(t("文件已保存，但完成审计写入失败。")); } finally { clear(); } }); }}>{t("确认并另存加密文件")}</Button>}
        {active.kind === "import" && <Button disabled={op.busy || !confirmed || !allChosen || incomingCount === 0 || !proxyStopped} onClick={() => void op.run(async () => { try { const result = await applySyncImport(active.data.ticketId, changes.map(row => ({ key: row.key, useIncoming: choices[row.key] === true })), confirmation); op.setMessage(t("已合并 {count} 项，创建 {disabled} 个禁用供应商副本，并保留本地备份。", { count: result.applied, disabled: result.disabledCopies })); await refresh(); } finally { clear(); } })}>{t("确认合并所选变更")}</Button>}
        {active.kind === "rollback" && <Button variant="danger" disabled={op.busy || !confirmed || !proxyStopped} onClick={() => void op.run(async () => { try { await applySyncRollback(active.data.ticketId, confirmation); op.setMessage(t("同步变更已回滚；新增供应商保留为禁用。")); await refresh(); } finally { clear(); } })}>{t("确认回滚同步")}</Button>}
        <Button variant="quiet" disabled={op.busy} onClick={clear}>{t("取消并清除预览")}</Button>
      </div></div>}><div className="gov-stack sync-preview">
      <InlineError message={op.error} />
      <p className="sync-expiry">{t("预览有效期至")} <time>{stamp(active.data.expiresAt)}</time></p>
      {active.kind === "export" && <>
        <dl className="sync-summary"><div><dt>{t("供应商")}</dt><dd>{active.data.content.providers.length}</dd></div><div><dt>{t("收藏")}</dt><dd>{active.data.content.favorites.length}</dd></div><div><dt>{t("自定义定价")}</dt><dd>{active.data.content.prices.length}</dd></div></dl>
        <p className="gov-warning">{t("不访问系统凭据存储；同步文件不包含请求头 / 请求体覆盖、TOML、MCP、提示词、会话正文和安全策略。名称与 URL 路径是自定义文本，仍需完整预览，避免手动嵌入秘密。")}</p>
        <p className="page-description">{t("AES-256-GCM + PBKDF2-SHA256（600000 次）。口令不保存；文件与口令请通过不同渠道传递，遗失口令无法恢复。")}</p>
        <DiffPreview identity={t("导出配置")} before="{}" after={json(active.data.content)} />
        <p className="page-description">{t("口令至少 12 个字符、最多 1024 个 UTF-8 字节。选择文件不代表允许导入；必须先预览并逐项确认。")}</p>
        <div className="sync-secret-fields"><Field label={t("加密口令")}><Input ref={pass} type="password" autoComplete="off" spellCheck={false} disabled={op.busy} onChange={checkSecrets} /></Field><Field label={t("再次输入加密口令")}><Input ref={repeated} type="password" autoComplete="off" spellCheck={false} disabled={op.busy} onChange={checkSecrets} /></Field></div>
      </>}
      {active.kind === "import" && <>
        <dl className="sync-summary"><div><dt>{t("变更")}</dt><dd>{changes.length}</dd></div><div><dt>{t("接受传入项")}</dt><dd>{incomingCount}</dd></div><div><dt>{t("保留本地 / 跳过")}</dt><dd>{localCount}</dd></div><div><dt>{t("尚未选择")}</dt><dd>{undecidedCount}</dd></div></dl>
        <p className="gov-warning">{t("目标地址或协议改变时，不会将本机旧凭据交给新目标：保留旧供应商，另建禁用副本。导入后需手动配置凭据并启用。")}</p>
      </>}
      {active.kind !== "export" && <>
        <p className="gov-warning">{t("应用变更前必须停止代理并恢复所有接管。保留最近 10 次本地备份；回滚拒绝覆盖导入后的新改动。新增供应商回滚后保留为禁用，不删除用量历史。")}</p>
        {!proxyStopped && <p className="gov-warning">{t("请先停止代理，并在设置中恢复接管。")}</p>}
      </>}
      {active.kind === "import" && <>
        <div className="gov-view"><div className="gov-actions"><Button variant="secondary" disabled={op.busy} onClick={() => { setChoices(Object.fromEntries(changes.map(row => [row.key, false]))); setReviewed(false); setConfirmation(""); }}>{t("全部保留本地")}</Button><Button variant="secondary" disabled={op.busy} onClick={() => { setChoices(Object.fromEntries(changes.map(row => [row.key, row.status === "add"]))); setReviewed(false); setConfirmation(""); }}>{t("仅接受新增项")}</Button></div><p className="page-description">{t("批量选择只更新处理方式，确认后才会应用。")}</p></div>
        <SyncImportReview changes={active.data.content.slice(page * reviewPageSize, (page + 1) * reviewPageSize)} choices={choices} disabled={op.busy} onChange={(key, value) => { setChoices(previous => ({ ...previous, [key]: value })); setReviewed(false); setConfirmation(""); }} pagination={active.data.content.length > reviewPageSize && <div className="sync-review__pagination"><Button variant="quiet" disabled={page === 0 || op.busy} onClick={() => setPage(page - 1)}>{t("上一页")}</Button><span className="gov-number">{page + 1} / {Math.ceil(active.data.content.length / reviewPageSize)}</span><Button variant="quiet" disabled={(page + 1) * reviewPageSize >= active.data.content.length || op.busy} onClick={() => setPage(page + 1)}>{t("下一页")}</Button></div>} />
        {!changes.length && <p>{t("没有需要合并的变更。")}</p>}
      </>}
      {active.kind === "rollback" && <><div className="gov-confirm-subject"><strong>{t("所选交换备份")} · {stamp(active.backup.createdAt)}</strong><code className="gov-id" tabIndex={0}>{active.backup.id}</code></div><DiffPreview identity={active.backup.id} before={json(active.data.content.before)} after={json(active.data.content.after)} /></>}
      <div className="gov-approval sync-final-confirmation">
        <div className="sync-final-confirmation__summary"><h3 className="sync-review__heading">{active.kind === "import" && <span>03</span>}{t("最终确认")}</h3>
          {active.kind === "import" && <><p>{t("将合并 {incoming} 项；{local} 项保留本地 / 跳过。", { incoming: incomingCount, local: localCount })}</p>{undecidedCount > 0 ? <p role="status">{t("还有 {count} 项未选择处理方式。", { count: undecidedCount })}</p> : allChosen && incomingCount === 0 && <p role="status">{t("全部保留本地，无需导入；可取消并清除预览。")}</p>}</>}
          {active.kind === "export" && <p>{t("确认并另存加密文件")}</p>}
          {active.kind === "rollback" && <p>{t("将回滚同步变更；新增供应商保留为禁用。")}</p>}
        </div>
        <div className="sync-final-confirmation__controls"><label className="gov-check"><input type="checkbox" disabled={op.busy} checked={reviewed} onChange={event => setReviewed(event.target.checked)} />{t("我已完整检查预览和处理方式")}</label>
        <Confirmation word={word} value={confirmation} onChange={setConfirmation} disabled={op.busy} /></div>
      </div>
    </div></Modal>}
    {view === "backups" && <div role="tabpanel" id="encrypted-sync-panel-backups" aria-labelledby="encrypted-sync-tab-backups">{!readOnly && <section className="gov-card"><div className="section-heading"><h2>{t("选择要恢复的交换备份")}</h2><Button variant="quiet" disabled={op.busy || query.isFetching} onClick={() => void query.refetch()}>{t("刷新")}</Button></div>
      <p className="page-description">{t("备份只保存在本机数据库，包含回滚所需的本机高级配置，可能含自行嵌入的敏感文本；不是加密同步文件，也不会被导出。")}</p>
      {query.error && <InlineError message={String(query.error)} />}
      {query.isPending && <p role="status">{t("读取中")}</p>}
      {query.data?.length === 0 && <p>{t("还没有同步导入备份")}</p>}
      <ul className="gov-list">{query.data?.map(backup => <li key={backup.id}><div><strong>{stamp(backup.createdAt)}</strong><p><code className="gov-id" tabIndex={0}>{backup.id}</code>{backup.used && <> · {t("已回滚")}</>}</p></div><Button variant="secondary" disabled={blocked || backup.used} onClick={() => void op.run(async () => install({ kind: "rollback", data: await previewSyncRollback(backup.id), backup }))}>{t("预览同步回滚")}</Button></li>)}</ul>
    </section>}</div>}
  </div>;
}
