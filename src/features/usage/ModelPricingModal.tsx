import { useMemo, useRef, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { localeTag, t, translateMessage, useLanguage } from "../../i18n/language";
import { Button, DataTable, Field, InlineError, Input, LoadingState, Modal, Select } from "../../ui";
import { deleteModelPricing, listModelPricing, saveModelPricing, syncModelsDevPricing } from "../../lib/usage-service";
import { errorText } from "../../lib/error";
import type { ModelPricing } from "../../types/usage";
import { emptyPricingDraft, parsePricingDraft, pricingDraft, type PricingDraft } from "./pricing-validation";

const PAGE_SIZE = 25;
const priceLabel = (value: number | null) => value == null ? t("未知") : value.toLocaleString(localeTag(), { maximumFractionDigits: 10 });

export function ModelPricingModal({ onClose }: { onClose: () => void }) {
  useLanguage();
  const query = useQuery({ queryKey: ["model-pricing"], queryFn: listModelPricing, retry: false, refetchOnWindowFocus: false });
  const [draft, setDraft] = useState<PricingDraft>(emptyPricingDraft);
  const [search, setSearch] = useState("");
  const [source, setSource] = useState("custom");
  const [page, setPage] = useState(0);
  const [busy, setBusy] = useState(false);
  const pending = useRef(false);
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");
  const [deleting, setDeleting] = useState<ModelPricing | null>(null);
  const filtered = useMemo(() => (query.data ?? []).filter(row => (!source || row.source === source) && row.id.toLowerCase().includes(search.trim().toLowerCase())), [query.data, source, search]);
  const lastPage = Math.max(0, Math.ceil(filtered.length / PAGE_SIZE) - 1);
  const visiblePage = Math.min(page, lastPage);
  const rows = filtered.slice(visiblePage * PAGE_SIZE, (visiblePage + 1) * PAGE_SIZE);
  const patch = (value: Partial<PricingDraft>) => setDraft(previous => ({ ...previous, ...value }));
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
  return <Modal open size="lg" title={t("模型定价")} description={t("价格单位：USD / 百万 token。这里只提供本地估算，不代表供应商账单。")} onClose={onClose} busy={busy}>
    <div className="form-stack model-pricing">
      <p className="muted">{t("自定义价格按完整模型 ID 精确匹配，对使用该 ID 的供应商共同生效；保存只影响后续请求和新导入记录，历史费用不会重算。")}</p>
      {error && <InlineError message={error} />}
      {notice && <p role="status">{translateMessage(notice)}</p>}
      <form className="form-stack" onSubmit={event => { event.preventDefault(); void perform(async () => {
        await saveModelPricing(parsePricingDraft(draft));
        setNotice(t("已保存定价。仅影响后续记录，历史费用不会重算。"));
        setSource("custom"); setPage(0);
        await refresh();
      }); }}>
        <Field label={t("定价模型 ID")} description={t("使用上游实际返回的模型 ID；同 ID 保存会覆盖本机自定义价格。")} required><Input value={draft.id} onChange={event => patch({ id: event.target.value })} disabled={busy} autoComplete="off" /></Field>
        <div className="model-pricing__rates">
          <Field label={t("输入价格")} required><Input type="number" min={0} max={1_000_000} step="any" value={draft.input} onChange={event => patch({ input: event.target.value })} disabled={busy} /></Field>
          <Field label={t("输出价格")} required><Input type="number" min={0} max={1_000_000} step="any" value={draft.output} onChange={event => patch({ output: event.target.value })} disabled={busy} /></Field>
          <Field label={t("缓存读取价格")} description={t("留空表示未知，0 表示免费。") }><Input type="number" min={0} max={1_000_000} step="any" value={draft.cacheRead} onChange={event => patch({ cacheRead: event.target.value })} disabled={busy} /></Field>
          <Field label={t("缓存写入价格")} description={t("留空表示未知，0 表示免费。") }><Input type="number" min={0} max={1_000_000} step="any" value={draft.cacheCreation} onChange={event => patch({ cacheCreation: event.target.value })} disabled={busy} /></Field>
        </div>
        <div className="inline-actions"><Button type="submit" disabled={busy}>{t("保存定价")}</Button><Button variant="secondary" disabled={busy} onClick={() => { setDraft(emptyPricingDraft()); setDeleting(null); }}>{t("新建定价")}</Button></div>
      </form>
      <section className="form-stack" aria-label={t("定价目录")}>
        <div className="inline-actions"><Button variant="secondary" disabled={busy} onClick={() => void perform(async () => {
          const count = await syncModelsDevPricing();
          setNotice(t("已同步 {count} 条参考价，自定义价格保持不变。", { count }));
          setSource("models.dev"); setSearch(""); setPage(0);
          await refresh();
        })}>{t("从 models.dev 同步参考价")}</Button><Button variant="quiet" disabled={busy || query.isFetching} onClick={() => void perform(refresh)}>{t("刷新目录")}</Button></div>
        <p className="muted">{t("同步仅访问 models.dev 公开目录，不发送密钥。参考价保留供应商前缀，不自动套用到同名裸模型；采用时可修改模型 ID 再保存。不支持的媒体或阶梯价格会跳过。")}</p>
        <div className="model-pricing__filters"><Field label={t("搜索定价")}><Input value={search} onChange={event => { setSearch(event.target.value); setPage(0); }} /></Field><Field label={t("价格来源")}><Select value={source} onChange={event => { setSource(event.target.value); setPage(0); }}><option value="custom">{t("自定义价格")}</option><option value="models.dev">{t("models.dev 参考价")}</option><option value="">{t("全部")}</option></Select></Field></div>
        {deleting && <div className="form-stack" role="alert"><p>{t("确认移除自定义定价 {id}？后续记录将使用参考价或显示费用未知，历史记录保持不变。", { id: deleting.id })}</p><div className="inline-actions"><Button variant="danger" disabled={busy} onClick={() => void perform(async () => {
          await deleteModelPricing(deleting.id); setDeleting(null); setNotice(t("自定义定价已移除，历史费用未修改。")); await refresh();
        })}>{t("确认移除定价")}</Button><Button variant="quiet" disabled={busy} onClick={() => setDeleting(null)}>{t("取消")}</Button></div></div>}
        {query.isPending && <LoadingState label={t("读取定价目录…")} />}
        {query.error && <InlineError message={errorText(query.error)} />}
        {!query.isPending && !query.error && <><DataTable caption={t("定价目录")} rows={rows} rowKey={row => row.id} emptyTitle={t("没有匹配的定价")} columns={[
          { key: "id", label: t("模型 ID"), render: row => <code className="model-pricing__id">{row.id}</code> },
          { key: "input", label: t("输入价格"), numeric: true, render: row => priceLabel(row.inputPerMillion) },
          { key: "output", label: t("输出价格"), numeric: true, render: row => priceLabel(row.outputPerMillion) },
          { key: "cache", label: t("缓存读取 / 写入"), render: row => priceLabel(row.cacheReadPerMillion) + " / " + priceLabel(row.cacheCreationPerMillion) },
          { key: "actions", label: t("操作"), render: row => <div className="inline-actions"><Button size="sm" variant="quiet" disabled={busy} onClick={() => { setDraft(pricingDraft(row)); setDeleting(null); }}>{row.source === "custom" ? t("编辑") : t("采用参考价")}</Button>{row.source === "custom" && <Button size="sm" variant="quiet" disabled={busy} onClick={() => setDeleting(row)}>{t("移除")}</Button>}</div> },
        ]} /><div className="inline-actions"><span className="muted">{t("共 {count} 条定价", { count: filtered.length })} · {visiblePage + 1} / {lastPage + 1}</span><Button size="sm" variant="secondary" disabled={visiblePage === 0} onClick={() => setPage(visiblePage - 1)}>{t("上一页")}</Button><Button size="sm" variant="secondary" disabled={visiblePage >= lastPage} onClick={() => setPage(visiblePage + 1)}>{t("下一页")}</Button></div></>}
      </section>
    </div>
  </Modal>;
}
