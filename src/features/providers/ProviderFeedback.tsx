import { InlineError, Toast } from "../../ui";
import type { ProvidersController } from "./useProviders";
export function ProviderFeedback({ controller: c }: { controller: ProvidersController }) {
  return <><InlineError message={c.error} onRetry={() => void c.refresh()} retrying={c.loading} />{c.message && <Toast tone="success" title={c.message} onDismiss={c.dismissMessage} />}</>;
}
