export const DEFAULT_INSPECTOR_WIDTH = 340;
export const MIN_INSPECTOR_WIDTH = 288;
export const MIN_LIST_WIDTH = 320;
export const SPLIT_HANDLE_WIDTH = 12;
export const PROVIDER_SPLIT_STORAGE_KEY = "xy-switch.providers.inspector-width.v1";

export function parseInspectorWidth(raw: string | null): number {
  if (!raw?.trim()) return DEFAULT_INSPECTOR_WIDTH;
  const width = Number(raw);
  return Number.isFinite(width) && width > 0 ? width : DEFAULT_INSPECTOR_WIDTH;
}

export function splitWorkspaceLayout(containerWidth: number, preferredWidth: number) {
  const width = Number.isFinite(containerWidth) ? Math.max(0, containerWidth) : 0;
  const handleWidth = Math.min(SPLIT_HANDLE_WIDTH, width);
  const available = width - handleWidth;
  // Very small containers degrade proportionally instead of overflowing either edge.
  const minimum = Math.min(MIN_INSPECTOR_WIDTH, available * MIN_INSPECTOR_WIDTH / (MIN_LIST_WIDTH + MIN_INSPECTOR_WIDTH));
  const maximum = Math.max(minimum, available - MIN_LIST_WIDTH);
  const preferred = Number.isFinite(preferredWidth) ? preferredWidth : DEFAULT_INSPECTOR_WIDTH;
  const inspectorWidth = Math.min(maximum, Math.max(minimum, preferred));
  return { inspectorWidth, listWidth: available - inspectorWidth, handleWidth, minimum, maximum, available };
}

export function inspectorWidthForKey(key: string, width: number, minimum: number, maximum: number, shift = false): number | null {
  const step = shift ? 48 : 16;
  switch (key) {
    case "ArrowLeft": return Math.min(maximum, width + step);
    case "ArrowRight": return Math.max(minimum, width - step);
    case "Home": return maximum;
    case "End": return minimum;
    case "Enter": return DEFAULT_INSPECTOR_WIDTH;
    default: return null;
  }
}
