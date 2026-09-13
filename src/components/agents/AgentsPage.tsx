import { useMemo, useState } from "react";
import { AgentSessionRow } from "./AgentSessionRow";
import { useAgentSessions } from "./useAgentSessions";
import { APP_LABELS, type AgentApp } from "./types";

type Filter = AgentApp | "all";

const FILTERS: Filter[] = ["all", "codex", "claude-code", "factory", "opencode"];

/**
 * Every coding-agent conversation on this Mac in one list: Codex, Claude Code,
 * Factory and OpenCode. Read-only, so opening this page cannot disturb a
 * running session.
 */
export default function AgentsPage() {
  const { sessions, problems, error, loading } = useAgentSessions();
  const [filter, setFilter] = useState<Filter>("all");
  const [showArchived, setShowArchived] = useState(false);

  const counts = useMemo(() => {
    const tally: Record<string, number> = { all: 0 };
    for (const session of sessions) {
      if (session.archived && !showArchived) continue;
      tally.all = (tally.all ?? 0) + 1;
      tally[session.app] = (tally[session.app] ?? 0) + 1;
    }
    return tally;
  }, [sessions, showArchived]);

  const visible = useMemo(
    () =>
      sessions.filter(
        (session) =>
          (showArchived || !session.archived) && (filter === "all" || session.app === filter),
      ),
    [sessions, filter, showArchived],
  );

  const liveCount = visible.filter((session) => session.live).length;

  return (
    <div className="mx-auto w-full max-w-5xl px-4 py-2">
      <header className="mb-5">
        <h1 className="text-2xl font-semibold tracking-tight text-foreground">Agents</h1>
        <p className="mt-2 max-w-[60ch] text-sm leading-6 text-muted-foreground">
          Every coding-agent conversation on this Mac, newest first. Running sessions
          are marked with a green dot.
          {liveCount > 0 && ` ${liveCount} running now.`}
        </p>
      </header>

      <div className="mb-4 flex flex-wrap items-center gap-2">
        {FILTERS.map((value) => (
          <button
            key={value}
            onClick={() => setFilter(value)}
            className={`cursor-pointer rounded-lg border px-3 py-1.5 text-xs transition-colors ${
              filter === value
                ? "border-foreground/40 bg-foreground/10 text-foreground"
                : "border-border/60 text-muted-foreground hover:bg-card/60"
            }`}
          >
            {value === "all" ? "All" : APP_LABELS[value]}
            <span className="ml-2 text-[11px] text-muted-foreground">{counts[value] ?? 0}</span>
          </button>
        ))}
        <button
          onClick={() => setShowArchived((current) => !current)}
          className="ml-auto cursor-pointer rounded-lg border border-border/60 px-3 py-1.5 text-xs text-muted-foreground transition-colors hover:bg-card/60"
        >
          {showArchived ? "Hide archived" : "Show archived"}
        </button>
      </div>

      {error && (
        <div className="rounded-xl border border-red-500/30 bg-red-950/15 px-4 py-3 text-sm text-red-200">
          Could not read agent sessions: {error}
        </div>
      )}

      {/* An app that cannot be read must say so: a silent failure looks
          identical to an app you have simply never used. */}
      {problems.map((problem) => (
        <div
          key={problem.app}
          className="mb-2 rounded-xl border border-amber-500/30 bg-amber-950/15 px-4 py-3 text-sm text-amber-100"
        >
          {APP_LABELS[problem.app]} sessions could not be read: {problem.message}
        </div>
      ))}

      {!error && loading && visible.length === 0 && (
        <p className="text-sm text-muted-foreground">Reading sessions…</p>
      )}

      {!error && !loading && visible.length === 0 && (
        <p className="max-w-[60ch] text-sm leading-6 text-muted-foreground">
          No sessions found. SOURCE reads the records Codex, Claude Code, Factory and
          OpenCode keep in your home folder, so this fills in once you have used one of
          them on this Mac.
        </p>
      )}

      <div className="flex flex-col gap-2">
        {visible.map((session) => (
          <AgentSessionRow key={`${session.app}:${session.id}`} session={session} />
        ))}
      </div>
    </div>
  );
}
