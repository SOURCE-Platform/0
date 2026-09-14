export type AgentApp = "codex" | "claude-code" | "factory" | "opencode";

export interface AgentSession {
  id: string;
  app: AgentApp;
  title: string;
  projectPath: string;
  projectName: string;
  updatedAtMs: number;
  preview: string;
  live: boolean;
  archived: boolean;
}

/** An app whose records could not be read, so the UI can say so instead of
 *  showing it as an app with no sessions. */
export interface AgentProblem {
  app: AgentApp;
  message: string;
}

export interface AgentSessionsSnapshot {
  sessions: AgentSession[];
  problems: AgentProblem[];
}

export const APP_LABELS: Record<AgentApp, string> = {
  codex: "Codex",
  "claude-code": "Claude Code",
  factory: "Factory",
  opencode: "OpenCode",
};

/** One accent per app so a row is identifiable before reading it. */
export const APP_ACCENTS: Record<AgentApp, string> = {
  codex: "bg-emerald-500/15 text-emerald-300 border-emerald-500/30",
  "claude-code": "bg-orange-500/15 text-orange-300 border-orange-500/30",
  factory: "bg-sky-500/15 text-sky-300 border-sky-500/30",
  opencode: "bg-violet-500/15 text-violet-300 border-violet-500/30",
};

/** "4m ago", "3h ago", "2d ago" — enough to judge what is current. */
export function relativeTime(ms: number, now = Date.now()): string {
  if (!ms) return "unknown";
  const seconds = Math.max(0, Math.round((now - ms) / 1000));
  if (seconds < 60) return "just now";
  const minutes = Math.round(seconds / 60);
  if (minutes < 60) return `${minutes}m ago`;
  const hours = Math.round(minutes / 60);
  if (hours < 24) return `${hours}h ago`;
  const days = Math.round(hours / 24);
  if (days < 7) return `${days}d ago`;
  return new Date(ms).toLocaleDateString();
}

export type MessageRole = "user" | "assistant" | "peer" | "tool";

export interface AgentMessage {
  id: string;
  role: MessageRole;
  text: string;
  atMs: number;
}

/** What a conversation SOURCE is driving reports (mirrors `DriverEvent`). */
export type DriverEvent =
  | { kind: "working" }
  | { kind: "added_to_current_work" }
  | { kind: "progress"; detail: string }
  | { kind: "tool_use"; name: string }
  | { kind: "assistant_text"; text: string }
  | { kind: "turn_done"; text: string; summary: string | null; isError: boolean; costUsd: number; durationMs: number }
  | { kind: "usage"; status: string; weeklyUtilization: number | null }
  | { kind: "auth_expired" }
  | { kind: "released" };

export interface BridgeEvent {
  sessionId: string;
  event: DriverEvent;
  brief: string | null;
  stillWorking: boolean;
}

/** Why a prompt couldn't be sent (mirrors `SendError`). */
export type SendError =
  | { kind: "handoff"; reason: "busy_in_app" | "running_elsewhere" | "stop_failed" | "no_transcript"; entrypoint?: string }
  | { kind: "no_claude_program" }
  | { kind: "no_conversation" }
  | { kind: "driver"; message: string };

export function describeSendError(error: unknown): string {
  const value = error as Partial<SendError> | string;
  if (typeof value === "string") return value;
  switch (value?.kind) {
    case "handoff":
      switch ((value as Extract<SendError, { kind: "handoff" }>).reason) {
        case "busy_in_app":
          return "Claude is working on something in the Claude app right now. Try again when it's done.";
        case "running_elsewhere":
          return "This conversation is open in a terminal. Close it there first.";
        case "stop_failed":
          return "The Claude app didn't let go of this conversation. Try again in a moment.";
        default:
          return "This conversation's transcript couldn't be found.";
      }
    case "no_claude_program":
      return "Claude Code isn't installed where SOURCE can find it.";
    case "no_conversation":
      return "SOURCE couldn't tell which folder this conversation runs in.";
    case "driver":
      return (value as Extract<SendError, { kind: "driver" }>).message;
    default:
      return "The prompt couldn't be sent.";
  }
}
