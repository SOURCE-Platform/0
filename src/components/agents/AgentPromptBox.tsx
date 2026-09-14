import { useState, type KeyboardEvent } from "react";
import type { TurnStatus } from "./useAgentSessionDetail";

interface AgentPromptBoxProps {
  status: TurnStatus;
  onSend: (text: string) => Promise<void>;
}

/** Type a prompt into the conversation. Enter sends; Shift+Enter adds a line. */
export function AgentPromptBox({ status, onSend }: AgentPromptBoxProps) {
  const [text, setText] = useState("");
  const sending = status.state === "sending";

  async function submit() {
    const prompt = text.trim();
    if (!prompt || sending) return;
    try {
      await onSend(prompt);
      setText("");
    } catch {
      // The status line explains what went wrong; keep the text so nothing is lost.
    }
  }

  function onKeyDown(event: KeyboardEvent<HTMLTextAreaElement>) {
    if (event.key === "Enter" && !event.shiftKey) {
      event.preventDefault();
      void submit();
    }
  }

  return (
    <div className="rounded-xl border border-border/60 bg-card/40 p-3">
      <StatusLine status={status} />
      <div className="flex items-end gap-2">
        <textarea
          value={text}
          onChange={(event) => setText(event.target.value)}
          onKeyDown={onKeyDown}
          rows={2}
          placeholder="Send a prompt to this conversation…"
          className="min-h-[44px] flex-1 resize-y rounded-lg border border-border/60 bg-background/60 px-3 py-2 text-sm text-foreground outline-none focus:border-foreground/40"
        />
        <button
          onClick={() => void submit()}
          disabled={!text.trim() || sending}
          className="cursor-pointer rounded-lg border border-foreground/30 bg-foreground/10 px-4 py-2 text-sm text-foreground transition-colors hover:bg-foreground/20 disabled:cursor-default disabled:opacity-40"
        >
          {sending ? "Sending…" : "Send"}
        </button>
      </div>
    </div>
  );
}

function StatusLine({ status }: { status: TurnStatus }) {
  if (status.state === "idle") return null;
  const tone =
    status.state === "error"
      ? "text-red-300"
      : status.state === "done"
        ? "text-green-300"
        : "text-muted-foreground";
  const text =
    status.state === "sending"
      ? "Handing the conversation to SOURCE…"
      : status.state === "working"
        ? status.detail
        : status.state === "done"
          ? status.brief
          : status.message;
  return <p className={`mb-2 max-w-[70ch] text-xs leading-5 ${tone}`}>{text}</p>;
}
