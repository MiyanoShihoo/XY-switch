export interface DiffLine { kind: "same" | "remove" | "add" | "fold"; text: string; count?: number; }
/** Linear-time prefix/suffix diff. No quadratic LCS on multi-megabyte rule files. */
export function governanceDiff(before: string, after: string, context = 3, changeLimit = 200): DiffLine[] {
  if (before === after) return [];
  const oldLines = before.split("\n"), newLines = after.split("\n");
  let start = 0, end = 0;
  while (start < oldLines.length && start < newLines.length && oldLines[start] === newLines[start]) start++;
  while (end < oldLines.length - start && end < newLines.length - start && oldLines[oldLines.length - end - 1] === newLines[newLines.length - end - 1]) end++;
  const lines: DiffLine[] = [];
  const fold = (count: number) => { if (count > 0) lines.push({ kind: "fold", text: "", count }); };
  fold(start - context);
  oldLines.slice(Math.max(0, start - context), start).forEach(text => lines.push({ kind: "same", text }));
  const changed = (input: string[], kind: "remove" | "add") => {
    input.slice(0, changeLimit).forEach(text => lines.push({ kind, text }));
    fold(input.length - changeLimit);
  };
  changed(oldLines.slice(start, oldLines.length - end), "remove");
  changed(newLines.slice(start, newLines.length - end), "add");
  oldLines.slice(oldLines.length - end, oldLines.length - end + Math.min(context, end)).forEach(text => lines.push({ kind: "same", text }));
  fold(end - context);
  return lines;
}
export const validInteger = (n: number, min: number, max: number) => Number.isSafeInteger(n) && n >= min && n <= max;
