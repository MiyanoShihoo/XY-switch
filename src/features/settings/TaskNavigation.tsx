import { createContext, useCallback, useContext, useEffect, useId, useRef, useState, type ReactNode } from "react";
import { t } from "../../i18n/language";
import { Button, Modal } from "../../ui";
import "./task-navigation.css";

type DraftState = { dirty: boolean; busy: boolean };
type HistoryNavigationEvent = Event & { navigationType: string; destination: { key: string; sameDocument: boolean } };
type HistoryNavigation = EventTarget & { traverseTo: (key: string) => { finished: Promise<unknown> } };
type Navigation = { leave: (next: () => void) => void; busy: boolean };
const NavigationContext = createContext<Navigation>({ leave: next => next(), busy: false });
export const useTaskNavigation = () => useContext(NavigationContext);
const DraftContext = createContext<((id: string, state: DraftState | null) => void) | null>(null);

export function useTaskDraft(dirty: boolean, busy = false) {
  const register = useContext(DraftContext), id = useId();
  useEffect(() => { register?.(id, { dirty, busy }); }, [register, id, dirty, busy]);
  useEffect(() => () => register?.(id, null), [register, id]);
}

export function TaskNavigation({ children, onLeaveApproved }: { children: (navigation: Navigation) => ReactNode; onLeaveApproved?: () => void }) {
  const [drafts, setDrafts] = useState<Record<string, DraftState>>({});
  const [pending, setPending] = useState<(() => void) | null>(null);
  const [waiting, setWaiting] = useState(false);
  const bypass = useRef(false);
  const approvedHistory = useRef<string | null>(null);
  const dirty = Object.values(drafts).some(state => state.dirty);
  const busy = Object.values(drafts).some(state => state.busy);
  const register = useCallback((id: string, state: DraftState | null) => setDrafts(current => {
    if (state && current[id]?.dirty === state.dirty && current[id]?.busy === state.busy) return current;
    if (!state && !current[id]) return current;
    const next = { ...current };
    if (state) next[id] = state; else delete next[id];
    return next;
  }), []);
  const leave = useCallback((next: () => void) => {
    if (busy) { setWaiting(true); return; }
    if (dirty) setPending(() => next); else next();
  }, [dirty, busy]);
  useEffect(() => { if (!busy) setWaiting(false); }, [busy]);
  useEffect(() => {
    const unload = (event: BeforeUnloadEvent) => { if (dirty || busy) { event.preventDefault(); event.returnValue = ""; } };
    const click = (event: MouseEvent) => {
      if (bypass.current || (!dirty && !busy) || !(event.target instanceof Element)) return;
      const target = event.target.closest<HTMLElement>(".xy-shell-nav-item, .xy-shell-activity, .xy-shell-brand");
      if (!target) return;
      const currentView = document.querySelector<HTMLElement>(".xy-shell-root")?.dataset.workspaceView;
      if (target.getAttribute("aria-current") === "page" && target.dataset.view === currentView) return;
      event.preventDefault(); event.stopImmediatePropagation();
      leave(() => { bypass.current = true; try { target.click(); } finally { bypass.current = false; } });
    };
    const navigation = (window as Window & { navigation?: HistoryNavigation }).navigation;
    const historyChange = (rawEvent: Event) => {
      const event = rawEvent as HistoryNavigationEvent;
      if (approvedHistory.current === event.destination.key) { approvedHistory.current = null; return; }
      if (!navigation || bypass.current || (!dirty && !busy) || event.navigationType !== "traverse" || !event.destination.sameDocument || !event.cancelable) return;
      event.preventDefault();
      const key = event.destination.key;
      leave(() => {
        approvedHistory.current = key;
        try {
          void navigation.traverseTo(key).finished.catch(() => undefined).finally(() => {
            if (approvedHistory.current === key) approvedHistory.current = null;
          });
        } catch { approvedHistory.current = null; }
      });
    };
    window.addEventListener("beforeunload", unload);
    window.addEventListener("click", click, true);
    navigation?.addEventListener("navigate", historyChange);
    return () => {
      window.removeEventListener("beforeunload", unload);
      window.removeEventListener("click", click, true);
      navigation?.removeEventListener("navigate", historyChange);
    };
  }, [dirty, busy, leave]);
  return <DraftContext.Provider value={register}><NavigationContext.Provider value={{ leave, busy }}>
    {children({ leave, busy })}
    {waiting && <p className="task-navigation-wait" role="status">{t("操作进行中，请等待结果后离开。")}</p>}
    <Modal open={Boolean(pending)} className="task-leave-dialog" title={t("离开当前任务？")} onClose={() => setPending(null)} busy={busy} footer={<>
      <Button variant="secondary" disabled={busy} onClick={() => setPending(null)}>{t("继续编辑")}</Button>
      <Button variant="danger" disabled={busy} onClick={() => { const next = pending; setPending(null); bypass.current = true; try { onLeaveApproved?.(); next?.(); } finally { bypass.current = false; } }}>{t("丢弃并离开")}</Button>
    </>}><p>{t("未保存的草稿将丢弃，敏感输入和审核票据会清理。已完成的写入不会撤销。")}</p></Modal>
  </NavigationContext.Provider></DraftContext.Provider>;
}

export function openRoutingSettings() {
  window.history.pushState({ settingsTask: "routing" }, "", "#settings");
  window.dispatchEvent(new PopStateEvent("popstate"));
}
