import { Button } from "@/components/ui/button";
import { PageHeader } from "@/components/ui/page";
import { useMemo, useState } from "react";
import { AgentSessionDetail } from "./AgentSessionDetail";
import { AgentSessionRow } from "./AgentSessionRow";
import { useAgentSessions } from "./useAgentSessions";
import { APP_LABELS, type AgentApp } from "./types";

type Filter = AgentApp | "all";

const FILTERS: Filter[] = ["all", "codex", "claude-code", "factory", "opencode"];

/**
 * Every coding-agent conversation on this Mac in one list: Codex, Claude Code,
 * Factory and OpenCode. Opening a conversation shows its messages; Claude Code
 * conversations can be continued from here.
 */
export default function AgentsPage() {
  const { sessions, problems, held, error, loading } = useAgentSessions();
  const [filter, setFilter] = useState<Filter>("all");
  const [openKey, setOpenKey] = useState<string | null>(null);
  const [limit, setLimit] = useState(80);
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
  const open = sessions.find((session) => `${session.app}:${session.id}` === openKey);

  if (open) {
    return (
      <div className="w-full max-w-6xl space-y-6">
        <AgentSessionDetail
          key={openKey}
          session={open}
          held={held.includes(open.id)}
          onBack={() => setOpenKey(null)}
        />
      </div>
    );
  }

  return (
    <div className="w-full max-w-6xl space-y-6">
      <PageHeader title="Agents" description="Your conversations across Codex, Claude Code, Factory, and OpenCode."
        action={liveCount > 0 ? <span className="flex items-center gap-2 text-xs text-muted-foreground"><span className="h-1.5 w-1.5 rounded-full bg-emerald-500" />{liveCount} running</span> : undefined} />

      <div className="mb-4 flex flex-wrap items-center gap-2">
        {FILTERS.map((value) => (
          <Button
            size="sm"
            variant={filter === value ? "secondary" : "ghost"}
            aria-pressed={filter === value}
            key={value}
            onClick={() => { setFilter(value); setLimit(80); }}

          >
            {value === "all" ? "All" : APP_LABELS[value]}
            <span className="ml-2 text-[11px] text-muted-foreground">{counts[value] ?? 0}</span>
          </Button>
        ))}
        <Button size="sm" variant="ghost" aria-pressed={showArchived}
          onClick={() => { setShowArchived((current) => !current); setLimit(80); }} className="ml-auto text-muted-foreground">
          {showArchived ? "Hide archived" : "Show archived"}
        </Button>
      </div>

      {error && (
        <div className="rounded-xl border border-red-500/30 bg-red-950/15 px-4 py-3 text-sm text-destructive">
          Could not read agent sessions: {error}
        </div>
      )}

      {/* An app that cannot be read must say so: a silent failure looks
          identical to an app you have simply never used. */}
      {problems.map((problem) => (
        <div
          key={problem.app}
          className="mb-2 rounded-xl border border-amber-500/30 bg-amber-950/15 px-4 py-3 text-sm text-amber-800 dark:text-amber-200"
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

      <div className={visible.length ? "agent-list" : undefined}>
        {visible.slice(0, limit).map((session) => (
          <AgentSessionRow
            key={`${session.app}:${session.id}`}
            session={session}
            held={held.includes(session.id)}
            onOpen={() => setOpenKey(`${session.app}:${session.id}`)}
          />
        ))}
      </div>
      {visible.length > limit && <div className="flex items-center justify-between gap-3">
        <p className="text-xs text-muted-foreground">Showing {limit} of {visible.length} conversations</p>
        <Button variant="outline" size="sm" onClick={() => setLimit((current) => current + 80)}>Show more</Button>
      </div>}
    </div>
  );
}
