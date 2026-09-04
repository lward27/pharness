import { useCallback, useEffect, useRef, useState } from "react";
import { getJson } from "./api";

type ResourceState<T> = {
  key: string | null;
  data: T | null;
  status: "loading" | "ready" | "refreshing" | "error";
  error: string | null;
  updatedAt: Date | null;
};
const empty = <T,>(key: string | null): ResourceState<T> => ({ key, data: null, status: key ? "loading" : "ready", error: null, updatedAt: null });

export function useResource<T = any>(path: string | null, options: { pollMs?: number; enabled?: boolean } = {}) {
  const { pollMs = 0, enabled = true } = options;
  const key = enabled ? path : null;
  const [state, setState] = useState<ResourceState<T>>(() => empty(key));
  const requestRef = useRef<AbortController | null>(null);

  const refresh = useCallback(async () => {
    if (!path || !enabled) return;
    requestRef.current?.abort();
    const controller = new AbortController();
    requestRef.current = controller;
    setState(current => current.key === key ? ({ ...current, status: current.data ? "refreshing" : "loading" }) : empty(key));
    try {
      const data = await getJson(path, { signal: controller.signal });
      if (controller.signal.aborted || requestRef.current !== controller) return;
      setState({ key, data, status: "ready", error: null, updatedAt: new Date() });
    } catch (error) {
      if (controller.signal.aborted || requestRef.current !== controller || (error as Error).name === "AbortError") return;
      setState(current => ({
        ...current,
        status: current.data ? "ready" : "error",
        error: error instanceof Error ? error.message : String(error),
        updatedAt: current.updatedAt,
      }));
    }
  }, [path, enabled, key]);

  useEffect(() => {
    refresh();
    if (!pollMs || !path || !enabled) return () => requestRef.current?.abort();
    const tick = () => { if (document.visibilityState === "visible") refresh(); };
    const timer = window.setInterval(tick, pollMs);
    document.addEventListener("visibilitychange", tick);
    return () => {
      requestRef.current?.abort();
      window.clearInterval(timer);
      document.removeEventListener("visibilitychange", tick);
    };
  }, [refresh, pollMs, path, enabled]);

  return { ...(state.key === key ? state : empty<T>(key)), refresh };
}
