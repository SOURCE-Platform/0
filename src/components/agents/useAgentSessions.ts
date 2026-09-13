import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { AgentSession } from "./types";

const REFRESH_MS = 10_000;

/**
 * Poll the read-only session hub. The backend reads files the agent apps
 * already write, so refreshing costs nothing but a few file reads.
 */
export function useAgentSessions(limit = 40) {
  const [sessions, setSessions] = useState<AgentSession[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const mounted = useRef(true);

  const refresh = useCallback(async () => {
    try {
      const data = await invoke<AgentSession[]>("list_agent_sessions", { limit });
      if (!mounted.current) return;
      setSessions(data);
      setError(null);
    } catch (err) {
      if (mounted.current) setError(String(err));
    } finally {
      if (mounted.current) setLoading(false);
    }
  }, [limit]);

  useEffect(() => {
    mounted.current = true;
    void refresh();
    const timer = setInterval(() => void refresh(), REFRESH_MS);
    return () => {
      mounted.current = false;
      clearInterval(timer);
    };
  }, [refresh]);

  return { sessions, error, loading, refresh };
}
