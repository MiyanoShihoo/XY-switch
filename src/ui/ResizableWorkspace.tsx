import { useLanguage, t } from "../i18n/language";
import { useCallback, useEffect, useId, useLayoutEffect, useRef, useState, type CSSProperties, type KeyboardEvent, type PointerEvent, type ReactNode } from "react";
import { GripVertical } from "lucide-react";
import { DEFAULT_INSPECTOR_WIDTH, inspectorWidthForKey, parseInspectorWidth, splitWorkspaceLayout } from "../lib/split-workspace";
import "./resizable-workspace.css";

interface ResizableWorkspaceProps {
  storageKey: string;
  compact: boolean;
  primary: ReactNode;
  secondary: ReactNode;
  primaryLabel: string;
  secondaryLabel: string;
  className?: string;
}
interface Drag {
  pointerId: number;
  element: HTMLDivElement;
  startX: number;
  startWidth: number;
  startPreference: number;
  lastWidth: number | null;
}

function loadWidth(key: string) {
  try { return parseInspectorWidth(window.localStorage.getItem(key)); }
  catch { return DEFAULT_INSPECTOR_WIDTH; }
}

export function ResizableWorkspace({ storageKey, compact, primary, secondary, primaryLabel, secondaryLabel, className = "" }: ResizableWorkspaceProps) {
  useLanguage();
  const id = useId();
  const rootRef = useRef<HTMLDivElement>(null);
  const dragRef = useRef<Drag | null>(null);
  const measuredWidth = useRef(0);
  const [preferredWidth, setPreferredWidth] = useState(() => loadWidth(storageKey));
  const [containerWidth, setContainerWidth] = useState(0);
  const [dragging, setDragging] = useState(false);
  const layout = splitWorkspaceLayout(containerWidth, preferredWidth);
  const canResize = !compact && layout.maximum > layout.minimum;
  const saveWidth = (width: number) => {
    setPreferredWidth(width);
    try { window.localStorage.setItem(storageKey, String(width)); }
    catch { /* Resizing still works when storage is unavailable. */ }
  };
  const finishDrag = useCallback((cancelled: boolean, updateState = true) => {
    const drag = dragRef.current;
    if (!drag) return null;
    dragRef.current = null;
    if (drag.element.hasPointerCapture(drag.pointerId)) drag.element.releasePointerCapture(drag.pointerId);
    if (updateState) {
      if (cancelled) setPreferredWidth(drag.startPreference);
      setDragging(false);
    }
    return drag;
  }, []);

  useLayoutEffect(() => {
    const element = rootRef.current;
    if (!element) return;
    const measure = () => {
      const width = element.clientWidth;
      if (width === measuredWidth.current) return;
      finishDrag(true);
      measuredWidth.current = width;
      setContainerWidth(width);
    };
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(element);
    const cancel = () => { finishDrag(true); };
    window.addEventListener("blur", cancel);
    return () => {
      observer.disconnect();
      window.removeEventListener("blur", cancel);
      finishDrag(true, false);
    };
  }, [finishDrag]);
  useEffect(() => { if (compact) finishDrag(true); }, [compact, finishDrag]);

  const beginDrag = (event: PointerEvent<HTMLDivElement>) => {
    if (!canResize || !event.isPrimary || event.button !== 0 || dragRef.current) return;
    event.preventDefault();
    event.currentTarget.focus({ preventScroll: true });
    event.currentTarget.setPointerCapture(event.pointerId);
    dragRef.current = { pointerId: event.pointerId, element: event.currentTarget, startX: event.clientX,
      startWidth: layout.inspectorWidth, startPreference: preferredWidth, lastWidth: null };
    setDragging(true);
  };
  const moveDrag = (event: PointerEvent<HTMLDivElement>) => {
    const drag = dragRef.current;
    if (!drag || drag.pointerId !== event.pointerId) return;
    const next = splitWorkspaceLayout(rootRef.current?.clientWidth ?? containerWidth, drag.startWidth + drag.startX - event.clientX).inspectorWidth;
    drag.lastWidth = next;
    setPreferredWidth(next);
  };
  const endDrag = (event: PointerEvent<HTMLDivElement>, cancelled: boolean) => {
    const active = dragRef.current;
    if (!active || active.pointerId !== event.pointerId) return;
    if (!cancelled && (active.lastWidth !== null || event.clientX !== active.startX)) moveDrag(event);
    const drag = finishDrag(cancelled);
    if (!cancelled && drag?.lastWidth != null) saveWidth(drag.lastWidth);
  };
  const reset = () => { finishDrag(true); saveWidth(DEFAULT_INSPECTOR_WIDTH); };
  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    if (event.key === "Escape" && dragRef.current) { event.preventDefault(); finishDrag(true); return; }
    if (!canResize || dragRef.current) return;
    const width = inspectorWidthForKey(event.key, layout.inspectorWidth, layout.minimum, layout.maximum, event.shiftKey);
    if (width === null) return;
    event.preventDefault();
    saveWidth(width);
  };
  const percent = (width: number) => layout.available ? Math.round(width / layout.available * 100) : 0;
  const style = {
    "--xy-split-inspector-width": (containerWidth ? layout.inspectorWidth : preferredWidth) + "px",
    "--xy-split-handle-width": (containerWidth ? layout.handleWidth : 12) + "px",
  } as CSSProperties;

  return <div ref={rootRef} className={"split-workspace xy-ui-resizable-workspace " + className} data-compact={compact} data-resizing={dragging || undefined} style={style}>
    <div id={id + "-primary"} className="split-workspace__list xy-ui-resizable-workspace__primary" role="region" aria-label={primaryLabel} tabIndex={0}>{primary}</div>
    {!compact && <>
      <div className="xy-ui-resizable-workspace__handle" role="separator" aria-label={t("调整") + primaryLabel + t("与") + secondaryLabel + t("宽度")}
        aria-orientation="vertical" aria-controls={id + "-secondary"} aria-describedby={id + "-help"}
        aria-valuemin={percent(layout.minimum)} aria-valuemax={percent(layout.maximum)} aria-valuenow={percent(layout.inspectorWidth)}
        aria-valuetext={secondaryLabel + t("宽度 ") + Math.round(layout.inspectorWidth) + t(" 像素")} aria-disabled={!canResize || undefined}
        tabIndex={canResize ? 0 : -1} title={t("左右拖动调整宽度；双击恢复默认。也可使用方向键调整，Enter 复位。")}
        onPointerDown={beginDrag} onPointerMove={moveDrag} onPointerUp={event => endDrag(event, false)}
        onPointerCancel={event => endDrag(event, true)} onLostPointerCapture={event => endDrag(event, true)}
        onKeyDown={onKeyDown} onDoubleClick={reset}>
        <GripVertical size={12} aria-hidden="true" />
      </div>
      <div id={id + "-secondary"} className="xy-ui-resizable-workspace__secondary">{secondary}</div>
      <span id={id + "-help"} className="sr-only">{t("拖动分隔线调整左右宽度。左右方向键微调，Shift 加速，Home 和 End 移到边界；双击或 Enter 恢复默认。Escape 取消本次拖动。宽度自动保存，面板内可横向滚动。")}</span>
    </>}
  </div>;
}
