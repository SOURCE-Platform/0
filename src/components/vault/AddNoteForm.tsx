//! Add-note form (Phase I, §10.8 Choice A). Title and body cross to the
//! helper once via `add_item`; the body is stored only inside the record
//! ciphertext. Local state clears after the call, success or failure.

import { useState } from "react";
import { vaultErrorMessage } from "@/lib/vault";
import { vaultAddNote } from "@/lib/vaultImport";

export function AddNoteForm({ onDone, onError }: { onDone: () => void; onError: (message: string | null) => void }) {
  const [title, setTitle] = useState("");
  const [body, setBody] = useState("");
  const [busy, setBusy] = useState(false);
  const complete = title.trim() !== "";

  async function submit() {
    if (!complete || busy) return;
    setBusy(true);
    onError(null);
    try {
      await vaultAddNote(title.trim(), body);
      onDone();
    } catch (e) {
      onError(vaultErrorMessage(String(e)));
    } finally {
      setTitle("");
      setBody("");
      setBusy(false);
    }
  }

  return (
    <div className="rounded-2xl border border-border/70 bg-black/10 p-5">
      <h3 className="text-sm font-medium text-foreground">Add a synthetic secure note</h3>
      <p className="mt-1 max-w-[60ch] text-xs leading-5 text-muted-foreground">
        Only the title is shown in lists. The note itself is encrypted and shown only when
        you reveal it. Notes are never filled into websites.
      </p>
      <label className="mt-4 block">
        <span className="mb-1 block text-xs text-muted-foreground">Title</span>
        <input
          value={title}
          onChange={(e) => setTitle(e.target.value)}
          placeholder="Synthetic note"
          autoComplete="off"
          className="w-full rounded-xl border border-border/70 bg-black/20 px-3 py-2 text-sm text-foreground outline-none placeholder:text-muted-foreground/50 focus:border-white/30"
        />
      </label>
      <label className="mt-3 block">
        <span className="mb-1 block text-xs text-muted-foreground">Note</span>
        <textarea
          value={body}
          onChange={(e) => setBody(e.target.value)}
          rows={5}
          autoComplete="off"
          spellCheck={false}
          className="w-full rounded-xl border border-border/70 bg-black/20 px-3 py-2 text-sm text-foreground outline-none focus:border-white/30"
        />
      </label>
      <div className="mt-4 flex items-center gap-3">
        <button
          onClick={submit}
          disabled={!complete || busy}
          className="cursor-pointer rounded-xl bg-white px-4 py-2 text-sm font-medium text-black transition-opacity hover:opacity-90 disabled:cursor-not-allowed disabled:opacity-40"
        >
          {busy ? "Confirming…" : "Save note"}
        </button>
        <button
          onClick={onDone}
          className="cursor-pointer rounded-xl border border-border/70 px-4 py-2 text-sm text-foreground hover:bg-white/5"
        >
          Cancel
        </button>
      </div>
    </div>
  );
}

export default AddNoteForm;
