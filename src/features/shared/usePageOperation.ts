import { useCallback, useEffect, useRef, useState } from "react";
import { errorText } from "../../lib/error";

/** Serializes writes and only announces success after the operation resolves. */
export function usePageOperation() {
  const lock = useRef(false);
  const mounted = useRef(true);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [message, setMessage] = useState("");
  useEffect(() => { mounted.current = true; return () => { mounted.current = false; }; }, []);
  const run = useCallback(async (work: () => Promise<void>, success = ""): Promise<boolean> => {
    if (lock.current) return false;
    lock.current = true;
    setBusy(true); setError(""); setMessage("");
    try {
      await work();
      if (mounted.current && success) setMessage(success);
      return true;
    } catch (cause) {
      if (mounted.current) setError(errorText(cause));
      return false;
    } finally {
      lock.current = false;
      if (mounted.current) setBusy(false);
    }
  }, []);
  return { busy, error, message, run, setError, setMessage };
}
