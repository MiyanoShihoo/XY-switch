import { useId, useRef } from "react";
import { cx } from "./utils";
export interface TabItem { value: string; label: string; count?: number; disabled?: boolean; }
interface TabsProps { items: TabItem[]; value: string; onChange: (value: string) => void; className?: string; label?: string; id?: string; }
export function Tabs({ items, value, onChange, className, label = "页面选项", id }: TabsProps) {
  const generated = useId(), listRef = useRef<HTMLDivElement>(null), prefix = id ?? generated;
  const choose = (item: TabItem) => { onChange(item.value); listRef.current?.querySelector<HTMLButtonElement>('[data-value="' + item.value + '"]')?.focus(); };
  return <div ref={listRef} className={cx("xy-ui-tabs", className)} role="tablist" aria-label={label}>
    {items.map(item => <button key={item.value} id={prefix + "-tab-" + item.value} type="button" role="tab" aria-selected={item.value === value} aria-controls={id ? prefix + "-panel-" + item.value : undefined} className="xy-ui-tabs__trigger" data-active={item.value === value} data-value={item.value} disabled={item.disabled} tabIndex={item.value === value ? 0 : -1} onClick={() => onChange(item.value)} onKeyDown={event => {
      const enabled = items.filter(i => !i.disabled), index = enabled.findIndex(i => i.value === item.value);
      if (!enabled.length) return;
      const forward = ["ArrowRight", "ArrowDown"].includes(event.key), backward = ["ArrowLeft", "ArrowUp"].includes(event.key);
      if (forward || backward || event.key === "Home" || event.key === "End") {
        event.preventDefault(); choose(event.key === "Home" ? enabled[0] : event.key === "End" ? enabled[enabled.length - 1] : enabled[(index + (forward ? 1 : -1) + enabled.length) % enabled.length]);
      }
    }}>{item.label}{typeof item.count === "number" && <span className="xy-ui-tabs__count">{item.count}</span>}</button>)}
  </div>;
}
