import { invoke } from "@tauri-apps/api/core";
import { maxPluginPackageBytes } from "../types/plugins";

const maximumBytes = maxPluginPackageBytes + 64 * 1024;
const safeErrors = new Set(["invalid_origin", "invalid_request", "invalid_response", "network", "session_changed", "rate_limited"]);
const requestIdPattern = /^[a-f0-9]{8}-[a-f0-9]{4}-4[a-f0-9]{3}-[89ab][a-f0-9]{3}-[a-f0-9]{12}$/;

export class MarketplaceTransportError extends Error {
  readonly code: string;
  constructor(code: unknown) {
    const safeCode = typeof code === "string" && safeErrors.has(code) ? code : "network";
    super(safeCode);
    this.name = "MarketplaceTransportError";
    this.code = safeCode;
  }
}

interface TransportOptions {
  origin: string;
  allowDevelopmentHttp: boolean;
  timeoutMs: number;
}

function responseValue(value: unknown): Response {
  if (!value || typeof value !== "object" || Array.isArray(value)) throw new MarketplaceTransportError("invalid_response");
  const record = value as Record<string, unknown>;
  if (Object.keys(record).length !== 3 || !Object.hasOwn(record, "status") || !Object.hasOwn(record, "body") || !Object.hasOwn(record, "contentType")
    || typeof record.status !== "number" || !Number.isInteger(record.status) || record.status < 200 || record.status > 599
    || record.status >= 300 && record.status < 400 || typeof record.body !== "string"
    || new TextEncoder().encode(record.body).length > maximumBytes) throw new MarketplaceTransportError("invalid_response");
  if (record.status === 204) {
    if (record.body !== "" || record.contentType !== null) throw new MarketplaceTransportError("invalid_response");
    return new Response(null, { status: 204 });
  }
  if (typeof record.contentType !== "string" || record.contentType.length > 256
    || !/^application\/json(?:\s*;|$)/i.test(record.contentType)) throw new MarketplaceTransportError("invalid_response");
  try {
    return new Response(record.body, { status: record.status, headers: { "Content-Type": record.contentType } });
  } catch { throw new MarketplaceTransportError("invalid_response"); }
}

export function createMarketplaceFetch(options: TransportOptions): typeof globalThis.fetch {
  if (typeof window === "undefined" || !("__TAURI_INTERNALS__" in window)) return globalThis.fetch.bind(globalThis);
  return async (input, init = {}) => {
    let requestId: string | undefined;
    let cancelled = false;
    let cancellationSent = false;
    let finished = false;
    let rejectAbort!: (reason: MarketplaceTransportError) => void;
    const aborted = new Promise<never>((_, reject) => { rejectAbort = reject; });
    const cancelKnownRequest = () => {
      if (requestId && !cancellationSent) {
        cancellationSent = true;
        void invoke("marketplace_cancel_request", { requestId }).catch(() => {});
      }
    };
    const onAbort = () => {
      cancelled = true;
      cancelKnownRequest();
      rejectAbort(new MarketplaceTransportError("network"));
    };
    init.signal?.addEventListener("abort", onAbort, { once: true });
    try {
      if (init.signal?.aborted) throw new MarketplaceTransportError("network");
      if (typeof input !== "string") throw new MarketplaceTransportError("invalid_request");
      const url = new URL(input);
      const headers = new Headers(init.headers);
      const method = init.method ?? "GET";
      if (url.origin !== options.origin || !url.pathname.startsWith("/v1/") || url.hash
        || url.username || url.password || !["GET", "POST"].includes(method)
        || init.body !== undefined && typeof init.body !== "string"
        || method === "GET" && init.body !== undefined || method === "POST" && typeof init.body !== "string"
        || [...headers.keys()].some(key => !["accept", "content-type", "authorization"].includes(key))) {
        throw new MarketplaceTransportError("invalid_request");
      }
      const authorization = headers.get("authorization");
      if (authorization && !/^Bearer [A-Za-z0-9._~+/=-]{1,4096}$/.test(authorization)) throw new MarketplaceTransportError("invalid_request");
      const begin = invoke<unknown>("marketplace_begin_request", { options }).then(value => {
        if (typeof value !== "string" || !requestIdPattern.test(value)) throw new MarketplaceTransportError("invalid_response");
        requestId = value;
        if (cancelled) {
          cancelKnownRequest();
          throw new MarketplaceTransportError("network");
        }
        return value;
      });
      await Promise.race([begin, aborted]);
      if (cancelled) throw new MarketplaceTransportError("network");
      const response = await Promise.race([invoke<unknown>("marketplace_request", { requestId, request: {
        path: `${url.pathname}${url.search}`, method, body: init.body ?? null, bearer: authorization?.slice(7) ?? null,
      } }), aborted]);
      finished = true;
      if (cancelled) throw new MarketplaceTransportError("network");
      return responseValue(response);
    } catch (error) {
      throw error instanceof MarketplaceTransportError ? error : new MarketplaceTransportError(error);
    } finally {
      init.signal?.removeEventListener("abort", onAbort);
      if (!finished) cancelKnownRequest();
    }
  };
}
