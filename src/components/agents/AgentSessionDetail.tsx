import { useEffect, useRef, useState } from "react";
import { AgentPromptBox } from "./AgentPromptBox";
import { useAgentSessionDetail } from "./useAgentSessionDetail";
import { APP_ACCENTS, APP_LABELS, relativeTime, type AgentMessage, type AgentSession } from "./types";

interface AgentSessionDetailProps {
  session: AgentSession;
  held: boolean;
  onBack: () => void;
}

/** One conversation: what was said, and (for Claude Code) a box to continue it. */
export function AgentSessionDetail({ session, held, onBack }: AgentSessionDetailProps) {
  const { messages, loadError, status, send, canSend } = useAgentSessionDetail(session);
  const bottom = useRef<HTMLDivElement>(null);

  useEffect(() => {
    bottom.current?.scrollIntoView({ block: "end" });
  }, [messages.length]);

  return (
    <div className="flex flex-col gap-4">
      <header className="flex items-start gap-3">
        <button
          onClick={onBack}
          className="cursor-pointer rounded-lg border border-border/60 px-3 py-1.5 text-xs text-muted-foreground transition-colors hover:bg-card/60"
        >
          ← All sessions
        </button>
        <div className="min-w-0 flex-1">
          <div className="flex flex-wrap items-center gap-2">
            <span className={`rounded-md border px-2 py-1 text-[11px] leading-none ${APP_ACCENTS[session.app]}`}>
              {APP_LABELS[session.app]}
            </span>
            <h2 className="truncate text-lg font-semibold text-foreground">{session.title}</h2>
            {held && (
              <span className="rounded-md border border-sky-500/30 bg-sky-500/10 px-2 py-1 text-[11px] leading-none text-sky-200">
                Held by SOURCE
              </span>
            )}
          </div>
          <p className="mt-1 text-xs text-muted-foreground">
            {session.projectName || "—"} · {relativeTime(session.updatedAtMs)}
          </p>
        </div>
      </header>

      {loadError && (
        <p className="rounded-xl border border-red-500/30 bg-red-950/15 px-4 py-3 text-sm text-red-200">{loadError}</p>
      )}

      <div className="flex flex-col gap-2">
        {messages.map((message) => (
          <MessageRow key={message.id} message={message} />
        ))}
        <div ref={bottom} />
      </div>

      {canSend ? (
        <AgentPromptBox status={status} onSend={send} />
      ) : (
        <p className="text-xs text-muted-foreground">
          Sending prompts works for Claude Code conversations so far.
        </p>
      )}
    </div>
  );
}

function MessageRow({ message }: { message: AgentMessage }) {
  if (message.role === "tool") {
    return <p className="pl-1 text-[11px] text-muted-foreground">Used {message.text}</p>;
  }
  const isUser = message.role === "user";
  const label = message.role === "peer" ? "Another session" : isUser ? "You" : "Claude";
  return (
    <div
      className={`group rounded-xl border px-4 py-3 ${
        isUser ? "ml-10 border-border/50 bg-foreground/5" : "mr-10 border-border/40 bg-card/40"
      }`}
    >
      <div className="mb-1 flex items-center justify-between gap-2">
        <span className="text-[11px] text-muted-foreground">{label}</span>
        <CopyButton text={message.text} />
      </div>
      <p className="max-w-[75ch] whitespace-pre-wrap text-sm leading-6 text-foreground">{message.text}</p>
    </div>
  );
}

function CopyButton({ text }: { text: string }) {
  const [copied, setCopied] = useState(false);
  return (
    <button
      onClick={() => {
        void navigator.clipboard.writeText(text).then(() => {
          setCopied(true);
          setTimeout(() => setCopied(false), 1500);
        });
      }}
      className="cursor-pointer rounded-md px-2 py-0.5 text-[11px] text-muted-foreground opacity-0 transition-opacity hover:bg-card/80 group-hover:opacity-100"
    >
      {copied ? "Copied" : "Copy"}
    </button>
  );
}
