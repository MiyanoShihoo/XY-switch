import { useEffect, useMemo, useRef, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { localeTag, translateMessage, useLanguage } from "../../i18n/language";
import { Button, DataTable, Field, InlineError, Input, LoadingState, Modal, Select } from "../../ui";
import { deleteModelPricing, listModelPricing, saveModelPricing, syncModelsDevPricing } from "../../lib/usage-service";
import { errorText } from "../../lib/error";
import type { ModelPricing } from "../../types/usage";
import { emptyPricingDraft, parsePricingDraft, pricingDraft, type PricingDraft } from "./pricing-validation";
import { t } from "../../i18n/language";

type DraftAction = { kind: "close" } | { kind: "draft"; draft: PricingDraft };
const PAGE_SIZE = 25;
const priceLabel = (value: number | null) => value == null ? t("未知") : value.toLocaleString(localeTag(), { maximumFractionDigits: 10 });

export function ModelPricingModal({ onClose }: { onClose: () => void }) {
  useLanguage();
  const query = useQuery({ queryKey: ["model-pricing"], queryFn: listModelPricing, retry: false, refetchOnWindowFocus: false });
  const [draft, setDraft] = useState<PricingDraft>(emptyPricingDraft);
  const [savedDraft, setSavedDraft] = useState<PricingDraft>(emptyPricingDraft);
  const [search, setSearch] = useState("");
  const [source, setSource] = useState("custom");
  const [page, setPage] = useState(0);
  const [busy, setBusy] = useState(false);
  const pending = useRef(false);
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");
  const [deleting, setDeleting] = useState<ModelPricing | null>(null);
  const [draftAction, setDraftAction] = useState<DraftAction | null>(null);
  const draftConfirmation = useRef<HTMLDivElement>(null);
  const editorInput = useRef<HTMLInputElement>(null);
  const searchInput = useRef<HTMLInputElement>(null);
  const removalTrigger = useRef<HTMLButtonElement | null>(null);
  const focusTarget = useRef<HTMLElement | null>(null);
  const removalConfirmation = useRef<HTMLDivElement>(null);
  const dirty = JSON.stringify(draft) !== JSON.stringify(savedDraft);
  const filtered = useMemo(() => (query.data ?? []).filter(row => (!source || row.source === source) && row.id.toLowerCase().includes(search.trim().toLowerCase())), [query.data, source, search]);
  const lastPage = Math.max(0, Math.ceil(filtered.length / PAGE_SIZE) - 1);
  const visiblePage = Math.min(page, lastPage);
  const rows = filtered.slice(visiblePage * PAGE_SIZE, (visiblePage + 1) * PAGE_SIZE);
  const patch = (value: Partial<PricingDraft>) => setDraft(previous => ({ ...previous, ...value }));
  const blocked = busy || Boolean(draftAction) || Boolean(deleting);
  useEffect(() => { if (draftAction) draftConfirmation.current?.focus(); }, [draftAction]);
  useEffect(() => { if (deleting) removalConfirmation.current?.focus(); }, [deleting]);
  useEffect(() => {
    if (busy || !focusTarget.current) return;
    const target = focusTarget.current;
    if (blocked && target !== removalConfirmation.current) return;
    focusTarget.current = null;
    (target.isConnected ? target : searchInput.current)?.focus();
  });
  const navigate = (action: DraftAction) => {
    if (action.kind === "close") { onClose(); return; }
    focusTarget.current = editorInput.current;
    setDraft(action.draft); setSavedDraft(action.draft); setDeleting(null); setError(""); setNotice("");
  };
  const requestAction = (action: DraftAction) => {
    if (pending.current || draftAction || deleting) return;
    if (dirty) setDraftAction(action);
    else navigate(action);
  };
  const continueEditing = () => {
    if (pending.current) return;
    focusTarget.current = editorInput.current;
    setDraftAction(null);
  };
  const cancelRemoval = () => {
    if (pending.current) return;
    focusTarget.current = removalTrigger.current ?? searchInput.current;
    setDeleting(null);
  };
  const perform = async (action: () => Promise<void>) => {
    if (pending.current) return;
    pending.current = true;
    setBusy(true); setError(""); setNotice("");
    try { await action(); }
    catch (cause) { setError(errorText(cause)); }
    finally { pending.current = false; setBusy(false); }
  };
  const refresh = async () => {
    const result = await query.refetch();
    if (result.error) throw result.error;
  };
  return <Modal open size="lg" className="model-pricing-modal" title={t("模型定价")} description={t("价格单位：USD / 百万 token。这里只提供本地估算，不代表供应商账单。")} onClose={() => { if (draftAction) continueEditing(); else if (deleting) cancelRemoval(); else requestAction({ kind: "close" }); }} busy={busy}>
    <div className="model-pricing">
      <p className="model-pricing__scope">{t("自定义价格按完整模型 ID 精确匹配，对使用该 ID 的供应商共同生效；保存只影响后续请求和新导入记录，历史费用不会重算。")}</p>
      {error && <InlineError message={error} />}
      {notice && <p role="status" className="model-pricing__notice">{translateMessage(notice)}</p>}
      {draftAction && <div className="model-pricing__confirmation" role="alert" ref={draftConfirmation} tabIndex={-1}>
        <strong>{t("放弃未保存的定价？")}</strong><p>{t("关闭或切换会丢弃当前草稿，已保存的定价保持不变。")}</p>
        <div className="inline-actions"><Button variant="secondary" disabled={busy} onClick={continueEditing}>{t("继续编辑")}</Button><Button variant="danger" disabled={busy} onClick={() => { if (pending.current) return; const action = draftAction; setDraftAction(null); navigate(action); }}>{t("放弃草稿")}</Button></div>
      </div>}
      <div className="model-pricing__workspace">
        <section className="model-pricing__catalog" aria-label={t("定价目录")}>
          <div className="usage-section-heading"><h3>{t("定价目录")}</h3><Button variant="secondary" disabled={blocked} onClick={() => requestAction({ kind: "draft", draft: emptyPricingDraft() })}>{t("新建定价")}</Button></div>
          <div className="model-pricing__filters"><Field label={t("搜索定价")}><Input ref={searchInput} value={search} disabled={blocked} onChange={event => { setSearch(event.target.value); setPage(0); }} /></Field><Field label={t("价格来源")}><Select value={source} disabled={blocked} onChange={event => { setSource(event.target.value); setPage(0); }}><option value="custom">{t("自定义价格")}</option><option value="models.dev">{t("models.dev 参考价")}</option><option value="">{t("全部")}</option></Select></Field></div>
          <div className="inline-actions"><Button variant="secondary" disabled={blocked} onClick={() => void perform(async () => {
            const count = await syncModelsDevPricing();
            setNotice(t("已同步 {count} 条参考价，自定义价格保持不变。", { count }));
            setSource("models.dev"); setSearch(""); setPage(0);
            await refresh();
          })}>{t("从 models.dev 同步参考价")}</Button><Button variant="quiet" disabled={blocked || query.isFetching} onClick={() => void perform(refresh)}>{t("刷新目录")}</Button></div>
          <details className="model-pricing__help"><summary>{t("参考价同步说明")}</summary><p>{t("同步仅访问 models.dev 公开目录，不发送密钥。参考价保留供应商前缀，不自动套用到同名裸模型；采用时可修改模型 ID 再保存。不支持的媒体或阶梯价格会跳过。")}</p></details>
          {deleting && <div className="model-pricing__removal" role="alert" ref={removalConfirmation} tabIndex={-1}><p>{t("确认移除自定义定价 {id}？后续记录将使用参考价或显示费用未知，历史记录保持不变。", { id: deleting.id })}</p><div className="inline-actions"><Button variant="danger" disabled={busy} onClick={() => void perform(async () => {
            focusTarget.current = removalConfirmation.current;
            await deleteModelPricing(deleting.id); focusTarget.current = searchInput.current; setDeleting(null); setNotice(t("自定义定价已移除，历史费用未修改。")); await refresh();
          })}>{t("确认移除定价")}</Button><Button variant="quiet" disabled={busy} onClick={cancelRemoval}>{t("取消")}</Button></div></div>}
          {query.isPending && <LoadingState label={t("读取定价目录…")} />}
          {query.error && <InlineError message={errorText(query.error)} />}
          {!query.isPending && !query.error && <><div className="usage-table-region model-pricing__table" role="region" tabIndex={0} aria-label={t("定价目录")}><DataTable caption={t("定价目录")} rows={rows} rowKey={row => row.id} emptyTitle={t("没有匹配的定价")} columns={[
            { key: "id", label: t("模型 ID"), render: row => <code className="model-pricing__id" tabIndex={0}>{row.id}</code> },
            { key: "input", label: t("输入价格"), numeric: true, render: row => priceLabel(row.inputPerMillion) },
            { key: "output", label: t("输出价格"), numeric: true, render: row => priceLabel(row.outputPerMillion) },
            { key: "cache-read", label: t("缓存读取"), numeric: true, render: row => priceLabel(row.cacheReadPerMillion) },
            { key: "cache-write", label: t("缓存写入"), numeric: true, render: row => priceLabel(row.cacheCreationPerMillion) },
            { key: "actions", label: t("操作"), render: row => <div className="inline-actions"><Button size="sm" variant="quiet" disabled={blocked} onClick={() => requestAction({ kind: "draft", draft: pricingDraft(row) })}>{row.source === "custom" ? t("编辑") : t("采用参考价")}</Button>{row.source === "custom" && <Button size="sm" variant="quiet" disabled={blocked} onClick={event => { if (blocked || pending.current) return; removalTrigger.current = event.currentTarget; setDeleting(row); }}>{t("移除")}</Button>}</div> },
          ]} /></div><div className="model-pricing__pagination"><span>{t("共 {count} 条定价", { count: filtered.length })} · {visiblePage + 1} / {lastPage + 1}</span><div className="inline-actions"><Button size="sm" variant="secondary" disabled={blocked || visiblePage === 0} onClick={() => setPage(visiblePage - 1)}>{t("上一页")}</Button><Button size="sm" variant="secondary" disabled={blocked || visiblePage >= lastPage} onClick={() => setPage(visiblePage + 1)}>{t("下一页")}</Button></div></div></>}
        </section>
        <form className="model-pricing__editor" onSubmit={event => { event.preventDefault(); if (blocked) return; void perform(async () => {
          await saveModelPricing(parsePricingDraft(draft));
          setSavedDraft(draft);
          setNotice(t("已保存定价。仅影响后续记录，历史费用不会重算。"));
          setSource("custom"); setPage(0);
          await refresh();
        }); }}>
          <div className="usage-section-heading"><h3>{t("自定义价格")}</h3>{dirty && <span className="model-pricing__dirty" role="status">{t("定价草稿尚未保存")}</span>}</div>
          <Field label={t("定价模型 ID")} description={t("使用上游实际返回的模型 ID；同 ID 保存会覆盖本机自定义价格。")} required><Input ref={editorInput} value={draft.id} onChange={event => patch({ id: event.target.value })} disabled={blocked} autoComplete="off" spellCheck={false} /></Field>
          <div className="model-pricing__rates">
            <Field label={t("输入价格")} required><Input type="number" min={0} max={1_000_000} step="any" value={draft.input} onChange={event => patch({ input: event.target.value })} disabled={blocked} /></Field>
            <Field label={t("输出价格")} required><Input type="number" min={0} max={1_000_000} step="any" value={draft.output} onChange={event => patch({ output: event.target.value })} disabled={blocked} /></Field>
            <Field label={t("缓存读取价格")}><Input type="number" min={0} max={1_000_000} step="any" value={draft.cacheRead} onChange={event => patch({ cacheRead: event.target.value })} disabled={blocked} /></Field>
            <Field label={t("缓存写入价格")}><Input type="number" min={0} max={1_000_000} step="any" value={draft.cacheCreation} onChange={event => patch({ cacheCreation: event.target.value })} disabled={blocked} /></Field>
          </div>
          <p className="usage-footnote">{t("留空表示未知，0 表示免费。")}</p>
          <div className="model-pricing__save"><Button type="submit" disabled={blocked}>{t("保存定价")}</Button><small>{t("本地估算 · 仅影响后续记录")}</small></div>
        </form>
      </div>
    </div>
  </Modal>;
}
