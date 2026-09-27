//! Total-loss recovery on a Mac without a vault (§11.5, §11.8): recovery
//! name + method → the helper's panel asks for the master password or
//! Recovery Key → the backup's date and item count are shown (FR-01) →
//! the user confirms → a new key is issued and the vault opens.

import { useState } from "react";
import { LifeBuoy } from "lucide-react";
import { type RecoveryPreview, vaultErrorMessage, vaultRecoveryFinish, vaultRecoveryStart } from "@/lib/vault";

export function RecoverCard({ onDone }: { onDone: () => void }) {
  const [handle, setHandle] = useState("");
  const [busy, setBusy] = useState(false);
  const [preview, setPreview] = useState<RecoveryPreview | null>(null);
  const [error, setError] = useState<string | null>(null);

  async function start(kind: "mp" | "rk") {
    setBusy(true);
    setError(null);
    try {
      setPreview(await vaultRecoveryStart(handle.trim(), kind));
    } catch (e) {
      setError(vaultErrorMessage(String(e)));
    } finally {
      setBusy(false);
    }
  }

  async function finish() {
    setBusy(true);
    setError(null);
    try {
      await vaultRecoveryFinish();
      onDone();
    } catch (e) {
      setError(vaultErrorMessage(String(e)));
    } finally {
      setBusy(false);
    }
  }

  const button =
    "cursor-pointer rounded-xl border border-border/70 px-4 py-2 text-sm text-foreground transition-colors hover:bg-white/5 disabled:cursor-not-allowed disabled:opacity-50";
  return (
    <div className="mt-4 rounded-2xl border border-border/70 bg-black/10 p-6">
      <div className="flex items-center gap-3">
        <LifeBuoy className="h-5 w-5 text-foreground" />
        <h2 className="text-lg font-medium text-foreground">Recover a vault from its backup</h2>
      </div>
      {!preview ? (
        <>
          <p className="mt-2 max-w-[60ch] text-sm leading-6 text-muted-foreground">
            Lost every device? Enter your recovery name, then your master password or Recovery Key
            in the helper's secure panel.
          </p>
          <input
            value={handle}
            onChange={(e) => setHandle(e.target.value)}
            placeholder="Recovery name"
            autoComplete="off"
            className="mt-4 w-full max-w-md rounded-xl border border-border/70 bg-transparent px-3 py-2 text-sm text-foreground outline-none focus:border-white/40"
          />
          <div className="mt-4 flex flex-wrap gap-2">
            <button className={button} disabled={busy || handle.trim().length < 3} onClick={() => start("mp")}>
              Use master password
            </button>
            <button className={button} disabled={busy || handle.trim().length < 3} onClick={() => start("rk")}>
              Use Recovery Key
            </button>
          </div>
        </>
      ) : (
        <>
          <p className="mt-2 max-w-[60ch] text-sm leading-6 text-foreground">
            This backup is from {new Date(preview.created_at * 1000).toLocaleString()} and contains{" "}
            {preview.item_count} {preview.item_count === 1 ? "item" : "items"}.
          </p>
          <p className="mt-2 max-w-[60ch] text-sm leading-6 text-muted-foreground">
            Compare it with your printed Recovery Key sheet: version {preview.generation}, check code{" "}
            <span className="font-mono">{preview.registry_head_prefix}</span>. An older version than your sheet
            means this backup is out of date. Finishing replaces the vault key, removes every other device,
            and may show you a new Recovery Key.
          </p>
          <button className={`mt-4 ${button}`} disabled={busy} onClick={finish}>
            {busy ? "Recovering…" : "Finish recovery"}
          </button>
        </>
      )}
      {error && <p className="mt-3 max-w-[60ch] text-sm leading-6 text-red-300">{error}</p>}
    </div>
  );
}
