import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { describeSendError, type AgentMessage, type AgentSession, type BridgeEvent } from "./types";

export type TurnStatus =
  | { state: "idle" }
  | { state: "sending" }
  | { state: "working"; detail: string }
  | { state: "done"; brief: string; handedBack: boolean }
  | { state: "error"; message: string };

/**
 * One conversation: its messages, and — for Claude Code — sending a prompt and
 * following the turn it starts. Messages reload when the transcript changes.
 */
export function useAgentSessionDetail(session: AgentSession) {
  const [messages, setMessages] = useState<AgentMessage[]>([]);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [status, setStatus] = useState<TurnStatus>({ state: "idle" });
  const mounted = useRef(true);
  const canSend = session.app === "claude-code";

  const reload = useCallback(async () => {
    try {
      const loaded = await invoke<AgentMessage[]>("agent_session_messages", {
        app: session.app,
        sessionId: session.id,
        limit: 60,
      });
      if (mounted.current) {
        setMessages(loaded);
        setLoadError(null);
      }
    } catch (err) {
      if (mounted.current) setLoadError(String(err));
    }
  }, [session.app, session.id]);

  useEffect(() => {
    mounted.current = true;
    void reload();
    const stops: Array<() => void> = [];
    listen("agent-sessions-changed", () => void reload()).then((stop) => stops.push(stop));
    listen<BridgeEvent>("agent-turn-event", ({ payload }) => {
      if (payload.sessionId !== session.id || !mounted.current) return;
      setStatus((current) => nextStatus(current, payload));
    }).then((stop) => stops.push(stop));
    return () => {
      mounted.current = false;
      stops.forEach((stop) => stop());
    };
  }, [reload, session.id]);

  const send = useCallback(
    async (text: string) => {
      setStatus({ state: "sending" });
      try {
        await invoke("agent_send_prompt", { sessionId: session.id, text });
        setStatus((current) => (current.state === "sending" ? { state: "working", detail: "Working" } : current));
      } catch (err) {
        setStatus({ state: "error", message: describeSendError(err) });
        throw err;
      }
    },
    [session.id],
  );

  return { messages, loadError, status, send, canSend, reload };
}

function nextStatus(current: TurnStatus, { event, brief, stillWorking }: BridgeEvent): TurnStatus {
  switch (event.kind) {
    case "working":
    case "added_to_current_work":
      return { state: "working", detail: event.kind === "working" ? "Working" : "Added to current work" };
    case "progress":
      return { state: "working", detail: stillWorking ? "Still working…" : event.detail };
    case "tool_use":
      return { state: "working", detail: `Using ${event.name}` };
    case "turn_done":
      return event.isError
        ? { state: "error", message: brief ?? "Claude reported an error." }
        : { state: "done", brief: brief ?? "Done.", handedBack: false };
    case "released":
      // SOURCE let go of the conversation: the Claude app can use it again.
      return current.state === "done" ? { ...current, handedBack: true } : current;
    case "auth_expired":
      return { state: "error", message: "Claude Code's sign-in has expired. Run `claude auth login` in Terminal." };
    default:
      return current;
  }
}
