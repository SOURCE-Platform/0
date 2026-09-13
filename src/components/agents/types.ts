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
