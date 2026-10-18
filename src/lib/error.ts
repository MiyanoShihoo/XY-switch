export function errorText(cause: unknown): string {
  return cause instanceof Error ? cause.message : typeof cause === "string" ? cause : String(cause);
}
