import { useEffect, useRef, type RefObject } from "react";

const dialogs: HTMLElement[] = [];
const focusable = 'button:not(:disabled), input:not(:disabled), select:not(:disabled), textarea:not(:disabled), a[href], summary, [tabindex]:not([tabindex="-1"])';
let previousOverflow = "";

export function useDialogFocus(open: boolean, panelRef: RefObject<HTMLElement | null>, onClose: () => void, busy = false, initialFocusRef?: RefObject<HTMLElement | null>) {
  const state = useRef({ onClose, busy });
  state.current = { onClose, busy };
  useEffect(() => {
    if (!open) return;
    const panel = panelRef.current;
    if (!panel) return;
    const previous = document.activeElement;
    if (!dialogs.length) { previousOverflow = document.body.style.overflow; document.body.style.overflow = "hidden"; }
    dialogs.push(panel);
    const controls = () => Array.from(panel.querySelectorAll<HTMLElement>(focusable)).filter(el => {
      if (el.hidden || el.tabIndex < 0 || !el.getClientRects().length || getComputedStyle(el).visibility === "hidden") return false;
      // Closed details may retain layout rectangles; only their own summary is a tab stop.
      for (let ancestor = el.parentElement; ancestor && ancestor !== panel; ancestor = ancestor.parentElement) {
        if (ancestor.tagName === "DETAILS" && !ancestor.hasAttribute("open") && !ancestor.querySelector(":scope > summary")?.contains(el)) return false;
      }
      return true;
    });
    const focusInside = () => {
      if (state.current.busy) panel.focus();
      else (initialFocusRef?.current ?? controls()[0] ?? panel).focus();
    };
    focusInside();
    const onFocus = (event: FocusEvent) => {
      if (dialogs.at(-1) !== panel) return;
      if (event.target instanceof Node && !panel.contains(event.target)) focusInside();
    };
    const onKey = (event: KeyboardEvent) => {
      if (dialogs.at(-1) !== panel) return;
      if (event.key === "Escape") {
        event.preventDefault(); event.stopPropagation();
        if (!state.current.busy) state.current.onClose();
      } else if (event.key === "Tab") {
        const items = controls();
        const first = items[0], last = items.at(-1);
        if (!first || !last) { event.preventDefault(); panel.focus(); }
        else if (!panel.contains(document.activeElement) || document.activeElement === panel) { event.preventDefault(); (event.shiftKey ? last : first).focus(); }
        else if (event.shiftKey && document.activeElement === first) { event.preventDefault(); last.focus(); }
        else if (!event.shiftKey && document.activeElement === last) { event.preventDefault(); first.focus(); }
      }
    };
    document.addEventListener("keydown", onKey, true);
    document.addEventListener("focusin", onFocus, true);
    return () => {
      document.removeEventListener("keydown", onKey, true);
      document.removeEventListener("focusin", onFocus, true);
      const index = dialogs.indexOf(panel); if (index >= 0) dialogs.splice(index, 1);
      if (!dialogs.length) document.body.style.overflow = previousOverflow;
      if (previous instanceof HTMLElement && previous.isConnected) previous.focus();
    };
  }, [open, panelRef, initialFocusRef]);
  useEffect(() => { if (open && busy) panelRef.current?.focus(); }, [open, busy, panelRef]);
}
