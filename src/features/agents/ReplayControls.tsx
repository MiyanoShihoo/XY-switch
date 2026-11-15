import { useEffect, useRef, useState, type ReactNode } from "react";
import { History, Pause, Play, Radio } from "lucide-react";
import { Button } from "../../ui";
import type { AgentSnapshot } from "../../types/agents";
import { isValidTimestamp, localDateInput, parseDateInput, timestamp } from "./format";
import { useAgentText } from "./i18n";

export function ReplayControls({ navigation, at, displayedAt, history, ready, loading, failed, onChange }: {
  navigation?: ReactNode;
  at: number | null; displayedAt: number | null; history: AgentSnapshot["history"] | null; ready: boolean; loading: boolean; failed: boolean; onChange: (at: number | null) => void;
}) {
  const { t, locale } = useAgentText();
  const [open, setOpen] = useState(false), [playing, setPlaying] = useState(false), [speed, setSpeed] = useState(10);
  const [draft, setDraft] = useState(at ?? 0), [dateInput, setDateInput] = useState(localDateInput(at));
  const debounce = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  const pendingAt = useRef<number | null>(null), editingDate = useRef(false);
  const oldest = history?.oldestAt ?? null, latest = history?.latestAt ?? null;
  const available = isValidTimestamp(oldest) && isValidTimestamp(latest) && latest >= oldest && !!localDateInput(oldest) && !!localDateInput(latest);
  useEffect(() => {
    if (at == null) return;
    editingDate.current = false; setDraft(at); setDateInput(localDateInput(at));
  }, [at]);
  useEffect(() => {
    if (at != null || editingDate.current || pendingAt.current != null) return;
    setDraft(latest ?? 0); setDateInput(localDateInput(latest));
  }, [at, latest]);
  useEffect(() => () => { if (debounce.current) clearTimeout(debounce.current); }, []);
  useEffect(() => { if (failed) setPlaying(false); }, [failed]);
  useEffect(() => {
    if (!playing || at == null || !available || loading || !ready || failed || at !== displayedAt) return;
    if (at >= latest!) { setPlaying(false); return; }
    const timer = setTimeout(() => onChange(Math.min(latest!, at + speed * 1000)), 1000);
    return () => clearTimeout(timer);
  }, [playing, at, available, latest, loading, ready, failed, displayedAt, speed, onChange]);
  const seek = (value: number, delayed = false) => {
    if (!available || !isValidTimestamp(value)) return;
    if (debounce.current) clearTimeout(debounce.current);
    debounce.current = undefined; pendingAt.current = null; editingDate.current = false;
    const next = Math.max(oldest!, Math.min(latest!, value));
    setPlaying(false); setDraft(next); setDateInput(localDateInput(next));
    if (delayed) {
      pendingAt.current = next;
      debounce.current = setTimeout(() => { debounce.current = undefined; pendingAt.current = null; onChange(next); }, 240);
    }
    else onChange(next);
  };
  const live = () => {
    if (debounce.current) clearTimeout(debounce.current);
    debounce.current = undefined; pendingAt.current = null; editingDate.current = false;
    setDraft(latest ?? 0); setDateInput(localDateInput(latest));
    setPlaying(false); setOpen(false); onChange(null);
  };
  const validDate = parseDateInput(dateInput);
  return <section className="ag-replay" data-open={open || at != null} aria-label={t("历史回放")}>
    <div className="ag-replay__navigation">{navigation}<div className="ag-replay__bar"><span className="ag-live-label" data-live={at == null} title={at == null ? t("刷新间隔 30 秒；事件通知触发增量刷新。") : undefined}>{at == null ? <Radio size={14} /> : <History size={14} />}{t(at == null ? "实时观测" : "历史回放")}</span>
      {at != null && <span className="ag-replay__time">{t("显示时刻")} <strong>{timestamp(displayedAt, locale)}</strong></span>}
      {at != null ? <Button variant="quiet" size="sm" onClick={live}>{t("返回实时")}</Button> : <Button variant="quiet" size="sm" icon={<History size={14} />} aria-expanded={open} onClick={() => setOpen(value => !value)}>{t("历史回放")}</Button>}</div></div>
    {(open || at != null) && <div className="ag-replay__controls"><p className="ag-note">{t("刷新间隔 30 秒；事件通知触发增量刷新。")}</p>{available ? <><div className="ag-replay__inputs"><label><span>{t("选择历史时刻")} · {t("本地时间")}</span><input type="datetime-local" step="1" min={localDateInput(oldest)} max={localDateInput(latest)} value={dateInput} onChange={event => { editingDate.current = true; setPlaying(false); setDateInput(event.target.value); }} onKeyDown={event => { if (event.key === "Enter" && validDate != null) seek(validDate); }} /></label>
      <Button variant="secondary" size="sm" disabled={validDate == null} onClick={() => { if (validDate != null) seek(validDate); }}>{t("在此时刻回放")}</Button>
      <Button variant="secondary" size="sm" disabled={failed || oldest === latest} icon={playing ? <Pause size={14} /> : <Play size={14} />} onClick={() => {
        if (debounce.current) clearTimeout(debounce.current);
        debounce.current = undefined;
        if (playing) { setPlaying(false); return; }
        if (pendingAt.current != null) { onChange(pendingAt.current); pendingAt.current = null; }
        else if (at == null || at >= latest!) onChange(oldest!);
        setPlaying(true);
      }}>{t(playing ? "暂停" : "播放")}</Button><label className="ag-inline-field"><span>{t("回放速度")}</span><select value={speed} onChange={event => setSpeed(Number(event.target.value))}><option value="1">1×</option><option value="10">10×</option><option value="60">60×</option></select></label></div>
      <input className="ag-replay__range" type="range" min={oldest!} max={latest!} step="1" value={Math.max(oldest!, Math.min(latest!, draft))} disabled={oldest === latest} aria-label={t("请求时刻")} aria-valuetext={timestamp(draft, locale)} onChange={event => seek(Number(event.target.value), true)} />
      <div className="ag-replay__bounds"><time>{timestamp(oldest, locale)}</time><span>{t("请求时刻")} · {timestamp(draft, locale)}</span><time>{timestamp(latest, locale)}</time></div><p className="ag-note">{t("每一帧均由后端按事件投影；等待当前快照后继续播放。")}{history?.truncated && ` ${t("历史已截断")}`}</p></> : <p className="ag-note">{t("暂无可回放历史")}</p>}</div>}
  </section>;
}
