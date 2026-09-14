import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { AgentProblem, AgentSession, AgentSessionsSnapshot } from "./types";

/**
 * The session hub's list. Refreshes when the agent apps' session files change
 * (the Mac tells us via `agent-sessions-changed`) and when the window regains
 * focus, instead of re-reading on a timer.
 */
export function useAgentSessions(limit = 40) {
  const [sessions, setSessions] = useState<AgentSession[]>([]);
  const [problems, setProblems] = useState<AgentProblem[]>([]);
  const [held, setHeld] = useState<string[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const mounted = useRef(true);

  const refresh = useCallback(async () => {
    try {
      const [data, heldIds] = await Promise.all([
        invoke<AgentSessionsSnapshot>("list_agent_sessions", { limit }),
        invoke<string[]>("agent_held_sessions").catch(() => [] as string[]),
      ]);
      if (!mounted.current) return;
      setSessions(data.sessions);
      setProblems(data.problems);
      setHeld(heldIds);
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
    const stops: Array<() => void> = [];
    listen("agent-sessions-changed", () => void refresh()).then((stop) => stops.push(stop));
    listen("agent-turn-event", () => void refresh()).then((stop) => stops.push(stop));
    const onFocus = () => void refresh();
    window.addEventListener("focus", onFocus);
    return () => {
      mounted.current = false;
      stops.forEach((stop) => stop());
      window.removeEventListener("focus", onFocus);
    };
  }, [refresh]);

  return { sessions, problems, held, error, loading, refresh };
}
