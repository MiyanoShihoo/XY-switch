import { useEffect, useId, useRef, useState } from "react";
import { History, Play } from "lucide-react";
import { t, useLanguage } from "../../i18n/language";
import { channelGroupErrorText, recordedChannelGroupEvents, simulateChannelGroup } from "../../lib/channel-group-service";
import type { AccountInfo } from "../../types/accounts";
import type { ChannelGroup, ChannelGroupRecordedEvent, ChannelGroupTrialResult } from "../../types/channel-group";
import type { Provider } from "../../types/provider";
import { Badge, Button, InlineError } from "../../ui";
import { deterministicChannelTrial } from "./channel-group-form";
import { channelMoney, channelNumber, channelReason, channelTime } from "./channel-group-presentation";
import { ChannelGroupStatus, ChannelGroupWarnings } from "./ChannelGroupStatus";

interface Props {
  group: ChannelGroup;
  snapshotAtMs: number;
  providers: Provider[];
  accounts: AccountInfo[];
  disabled?: boolean;
  onBusyChange: (busy: boolean) => void;
}

export function ChannelGroupSimulation({ group, snapshotAtMs, providers, accounts, disabled = false, onBusyChange }: Props) {
  useLanguage();
  const id = useId();
  const [events, setEvents] = useState<ChannelGroupRecordedEvent[] | null>(null);
  const [selectedIds, setSelectedIds] = useState<number[]>([]);
  const [operation, setOperation] = useState<"history" | "simulation" | null>(null);
  const [historyError, setHistoryError] = useState("");
  const [trialError, setTrialError] = useState("");
  const [result, setResult] = useState<ChannelGroupTrialResult | null>(null);
  const [inputAtMs, setInputAtMs] = useState<number | null>(null);
  const alive = useRef(true);
  const lock = useRef(false);
  const resultRef = useRef<HTMLHeadingElement>(null);
  const errorRef = useRef<HTMLDivElement>(null);
  const busy = operation !== null;
  useEffect(() => { alive.current = true; return () => { alive.current = false; onBusyChange(false); }; }, [onBusyChange]);
  useEffect(() => { if (result) resultRef.current?.focus(); }, [result]);
  useEffect(() => { if (historyError || trialError) errorRef.current?.focus(); }, [historyError, trialError]);
  const finish = () => { lock.current = false; if (alive.current) { setOperation(null); onBusyChange(false); } };
  const readHistory = async () => {
    if (disabled || lock.current) return;
    lock.current = true; setOperation("history"); onBusyChange(true); setHistoryError("");
    try {
      const next = await recordedChannelGroupEvents(group.id, 50);
      if (alive.current) { setEvents(next); setSelectedIds([]); setResult(null); }
    } catch (cause) { if (alive.current) setHistoryError(channelGroupErrorText(cause)); }
    finally { finish(); }
  };
  const run = async () => {
    if (disabled || lock.current) return;
    lock.current = true; setOperation("simulation"); onBusyChange(true); setTrialError(""); setResult(null);
    try {
      const input = deterministicChannelTrial(group.id, snapshotAtMs, (events ?? []).filter(event => selectedIds.includes(event.id)));
      const next = await simulateChannelGroup(input);
      if (alive.current) { setInputAtMs(input.steps[0].atMs); setResult(next); }
    } catch (cause) { if (alive.current) setTrialError(channelGroupErrorText(cause)); }
    finally { finish(); }
  };
  const selectEvent = (eventId: number, checked: boolean) => {
    if (busy || disabled) return;
    setSelectedIds(previous => checked ? [...previous, eventId] : previous.filter(value => value !== eventId));
    setResult(null);
  };
  return <section className="channel-group-section channel-group-simulation" aria-labelledby={id}>
    <div className="channel-group-section-heading"><h2 id={id}>{t("离线选择试验")}</h2><Badge>{t("使用已保存配置")}</Badge></div>
    <p>{t("固定输入：6 条合成预留，每秒 1 条，每条 128 Token、1,000 μUSD。预留保留至试验结束，不生成模型完成结果。")}</p>
    <p>{t("同一快照与事件选择生成相同输入；本地账本或账号元数据变化可能改变选择结果。整个试验不发送网络、模型或探测请求。")}</p>
    <div className="channel-group-trial-actions">
      <Button icon={<Play size={15} />} loading={operation === "simulation"} disabled={disabled || busy} onClick={() => void run()}>{selectedIds.length ? t("回放 {count} 条后模拟", { count: selectedIds.length }) : t("运行固定模拟")}</Button>
      <Button variant="secondary" icon={<History size={15} />} loading={operation === "history"} disabled={disabled || busy} onClick={() => void readHistory()}>{events ? t("刷新本地记录") : t("读取本地记录")}</Button>
    </div>
    <div ref={errorRef} tabIndex={-1}><InlineError message={historyError} /><InlineError message={trialError} /></div>
    {events === null ? <p className="channel-group-hint">{t("可直接运行固定模拟，也可先读取供应商级记录并选择回放事件。")}</p>
      : <details className="channel-group-history" open>
        <summary>{t("可选回放记录（{selected} / {total}）", { selected: selectedIds.length, total: events.length })}</summary>
        <p>{t("最多读取 50 条供应商级记录。选中事件会影响同供应商的多个渠道，记录本身没有渠道归属。")}</p>
        {!events.length ? <p role="status">{t("暂无可回放的本地记录。仍可运行固定模拟。")}</p> : <>
          {selectedIds.length > 0 && <Button variant="quiet" size="sm" disabled={disabled || busy} onClick={() => { setSelectedIds([]); setResult(null); }}>{t("清除回放选择")}</Button>}
          <div className="channel-group-table-scroll"><table className="channel-group-table channel-group-event-table">
            <caption className="sr-only">{t("供应商级历史事件，可选用于离线回放")}</caption>
            <thead><tr><th scope="col">{t("回放")}</th><th scope="col">{t("时间 / 记录 ID")}</th><th scope="col">{t("供应商")}</th><th scope="col">{t("HTTP 状态")}</th><th scope="col">{t("Token / 金额")}</th></tr></thead>
            <tbody>{events.map(event => <tr key={event.id}>
              <td><input type="checkbox" aria-label={t("回放记录 {id}", { id: event.id })} checked={selectedIds.includes(event.id)} disabled={disabled || busy} onChange={change => selectEvent(event.id, change.target.checked)} /></td>
              <th scope="row">{channelTime(event.atMs)}<code>#{event.id}</code></th>
              <td>{providers.find(provider => provider.id === event.providerId)?.name ?? event.providerId}<code>{event.providerId}</code></td>
              <td>{channelNumber(event.status)}{event.errorKind && <small>{event.errorKind}</small>}</td>
              <td>{channelNumber(event.tokens)}<small>{channelMoney(event.costMicrousd)}</small></td>
            </tr>)}</tbody>
          </table></div>
        </>}
      </details>}
    {result && <div className="channel-group-trial-result">
      <div className="channel-group-section-heading"><h3 ref={resultRef} tabIndex={-1}>{t("试验结果")}</h3><Badge tone="info">{channelReason(result.source)}</Badge></div>
      <p>{t("输入起点：{time}", { time: channelTime(inputAtMs) })}</p>
      <dl className="channel-group-metrics">
        <div><dt>{t("实时派发")}</dt><dd>{channelNumber(result.liveDispatches)}</dd></div>
        <div><dt>{t("账本写入")}</dt><dd>{channelNumber(result.ledgerWrites)}</dd></div>
        <div><dt>{t("选择决策")}</dt><dd>{channelNumber(result.decisions.length)}</dd></div>
        <div><dt>{t("供应商事件回放")}</dt><dd>{channelNumber(result.replayedEventIds.length)}</dd></div>
      </dl>
      {result.replayedEventIds.length > 0 && <p className="channel-group-model">{t("实际回放记录 ID：{ids}", { ids: result.replayedEventIds.join(", ") })}</p>}
      <ol className="channel-group-decisions">{result.decisions.map((decision, index) => <li key={`${decision.requestId}-${index}`}>
        <div><code>{decision.requestId}</code><strong>{channelReason(decision.reason)}</strong></div>
        {decision.channelId && <p><code>{decision.channelId}</code><span> · </span><code>{decision.upstreamModel}</code></p>}
        {decision.excluded.length > 0 && <details><summary>{t("排除 {count} 个渠道", { count: decision.excluded.length })}</summary>
          <ul>{decision.excluded.map(exclusion => <li key={exclusion.channelId}><code>{exclusion.channelId}</code><span>{channelReason(exclusion.reason)}</span>{exclusion.retryAtMs != null && <span>{t("可重试时间：{time}", { time: channelTime(exclusion.retryAtMs) })}</span>}</li>)}</ul>
        </details>}
      </li>)}</ol>
      <ChannelGroupWarnings warnings={result.warnings} />
      <details className="channel-group-trial-state"><summary>{t("查看模拟后的状态与预留")}</summary><ChannelGroupStatus group={group} channels={result.channels} providers={providers} accounts={accounts} source="simulation" /></details>
    </div>}
  </section>;
}
