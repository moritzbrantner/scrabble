import { useEffect, useRef } from "react";

/**
 * Retries an interrupted, safely repeatable step once the page is back in the foreground (#209).
 * Background tabs and switched-away phone apps are hidden, frozen, offline or put in bfcache, so
 * an HTTP request started there fails although nothing is wrong with the request itself.
 */
export function useForegroundRetry(enabled: boolean, retry: () => void): void {
  const latest = useRef(retry);
  latest.current = retry;
  useEffect(() => {
    if (!enabled) {
      return;
    }
    const lifecycle = new AbortController();
    const listen = { signal: lifecycle.signal };
    const returned = () => {
      if (document.visibilityState !== "hidden" && navigator.onLine !== false) {
        lifecycle.abort();
        latest.current();
      }
    };
    document.addEventListener("visibilitychange", returned, listen);
    document.addEventListener("resume", returned, listen);
    window.addEventListener("online", returned, listen);
    window.addEventListener("pageshow", returned, listen);
    return () => lifecycle.abort();
  }, [enabled]);
}
