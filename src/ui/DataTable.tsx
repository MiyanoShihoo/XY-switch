import type { ReactNode } from "react";
import { EmptyState } from "./EmptyState";
export interface DataColumn<T> { key: string; label: ReactNode; render: (row: T) => ReactNode; numeric?: boolean; }
export function DataTable<T>({ columns, rows, rowKey, caption, emptyTitle = "暂无记录" }: { columns: DataColumn<T>[]; rows: T[]; rowKey: (row: T) => string; caption: string; emptyTitle?: string }) {
  if (!rows.length) return <EmptyState title={emptyTitle} />;
  return <div className="xy-ui-table-scroll"><table className="xy-ui-table"><caption className="sr-only">{caption}</caption><thead><tr>{columns.map(c => <th key={c.key} scope="col" className={c.numeric ? "xy-ui-table__number" : undefined}>{c.label}</th>)}</tr></thead><tbody>{rows.map(row => <tr key={rowKey(row)}>{columns.map(c => <td key={c.key} className={c.numeric ? "xy-ui-table__number" : undefined}>{c.render(row)}</td>)}</tr>)}</tbody></table></div>;
}
