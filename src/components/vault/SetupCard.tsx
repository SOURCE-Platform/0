//! Vault creation (UNINITIALIZED): the public recovery name, then the
//! helper's secure panel for the master password (§1.5 setup_vault {handle}).

import { useState } from "react";
import { KeyRound } from "lucide-react";

export function SetupCard({ busy, onCreate }: { busy: boolean; onCreate: (handle: string) => void }) {
  const [handle, setHandle] = useState("");
  const valid = handle.trim().length >= 3;
  return (
    <div className="rounded-2xl border border-border/70 bg-black/10 p-6">
      <div className="flex items-center gap-3">
        <KeyRound className="h-5 w-5 text-foreground" />
        <h2 className="text-lg font-medium text-foreground">Create your vault</h2>
      </div>
      <p className="mt-2 max-w-[60ch] text-sm leading-6 text-muted-foreground">
        First choose a recovery name. It is how you find your backup if you ever lose every
        device — it can be your email address. It is not a password: anyone who knows it can
        see that a backup exists, but it opens nothing on its own.
      </p>
      <input
        value={handle}
        onChange={(e) => setHandle(e.target.value)}
        placeholder="Recovery name (for example your email)"
        autoComplete="off"
        className="mt-4 w-full max-w-md rounded-xl border border-border/70 bg-transparent px-3 py-2 text-sm text-foreground outline-none focus:border-white/40"
      />
      <p className="mt-3 max-w-[60ch] text-sm leading-6 text-muted-foreground">
        Your master password is typed into the helper's own secure panel — never into this
        window. After creation the vault starts locked and backs itself up.
      </p>
      <button
        onClick={() => onCreate(handle.trim())}
        disabled={busy || !valid}
        className="mt-4 cursor-pointer rounded-xl bg-white px-4 py-2 text-sm font-medium text-black transition-opacity hover:opacity-90 disabled:cursor-not-allowed disabled:opacity-50"
      >
        {busy ? "Waiting for the secure panel…" : "Create vault"}
      </button>
    </div>
  );
}
