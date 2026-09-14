import { APP_ACCENTS, APP_LABELS, relativeTime, type AgentSession } from "./types";

interface AgentSessionRowProps {
  session: AgentSession;
  held: boolean;
  onOpen: () => void;
}

/** One conversation: which app it lives in, what it is about, how fresh it is. */
export function AgentSessionRow({ session, held, onOpen }: AgentSessionRowProps) {
  return (
    <button
      onClick={onOpen}
      className="flex w-full cursor-pointer items-start gap-4 rounded-xl border border-border/60 bg-card/40 px-4 py-3 text-left transition-colors hover:bg-card/70"
    >
      <span
        className={`mt-0.5 shrink-0 rounded-md border px-2 py-1 text-[11px] font-medium leading-none ${APP_ACCENTS[session.app]}`}
      >
        {APP_LABELS[session.app]}
      </span>

      <div className="min-w-0 flex-1">
        <div className="flex items-center gap-2">
          {session.live && (
            <span
              className="h-2 w-2 shrink-0 rounded-full bg-green-400"
              title="A process is running this session now"
            />
          )}
          <p className="truncate text-sm font-medium text-foreground">{session.title}</p>
          {held && (
            <span className="shrink-0 rounded-md border border-sky-500/30 bg-sky-500/10 px-1.5 py-0.5 text-[10px] leading-none text-sky-200">
              Held by SOURCE
            </span>
          )}
          {session.archived && (
            <span className="shrink-0 text-[11px] text-muted-foreground">archived</span>
          )}
        </div>
        {session.preview && (
          <p className="mt-1 line-clamp-2 max-w-[70ch] text-xs leading-5 text-muted-foreground">
            {session.preview}
          </p>
        )}
      </div>

      <div className="shrink-0 text-right">
        <p className="truncate text-xs text-foreground/80" title={session.projectPath}>
          {session.projectName || "—"}
        </p>
        <p className="mt-1 text-[11px] text-muted-foreground">
          {relativeTime(session.updatedAtMs)}
        </p>
      </div>
    </button>
  );
}
