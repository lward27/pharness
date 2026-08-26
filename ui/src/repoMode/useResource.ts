import { useCallback, useEffect, useRef, useState } from "react";
import { getJson } from "./api";

type ResourceState<T> = {
  data: T | null;
  status: "loading" | "ready" | "refreshing" | "error";
  error: string | null;
  updatedAt: Date | null;
};

export function useResource<T = any>(path: string | null, options: { pollMs?: number; enabled?: boolean } = {}) {
  const { pollMs = 0, enabled = true } = options;
  const [state, setState] = useState<ResourceState<T>>({ data: null, status: "loading", error: null, updatedAt: null });
  const requestRef = useRef<AbortController | null>(null);

  const refresh = useCallback(async () => {
    if (!path || !enabled) return;
    requestRef.current?.abort();
    const controller = new AbortController();
    requestRef.current = controller;
    setState(current => ({ ...current, status: current.data ? "refreshing" : "loading" }));
    try {
      const data = await getJson(path, { signal: controller.signal });
      setState({ data, status: "ready", error: null, updatedAt: new Date() });
    } catch (error) {
      if ((error as Error).name === "AbortError") return;
      setState(current => ({
        ...current,
        status: current.data ? "ready" : "error",
        error: error instanceof Error ? error.message : String(error),
        updatedAt: current.updatedAt,
      }));
    }
  }, [path, enabled]);

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

  return { ...state, refresh };
}
