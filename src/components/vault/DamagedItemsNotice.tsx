//! Owner decision 2026-10-17 (final review Q1): a key change — removing a
//! device, replacing the Recovery Key — stops before any prompt when a
//! saved item no longer decrypts. The helper names those items in a
//! `damaged_records` event; this notice says which, and what to do.

import { useEffect, useState } from "react";
import { ShieldAlert } from "lucide-react";
import { onVaultEvent, vaultListItems } from "@/lib/vault";

export function DamagedItemsNotice() {
  const [names, setNames] = useState<string[] | null>(null);

  useEffect(() => {
    let stop: (() => void) | undefined;
    onVaultEvent(async (event) => {
      if (event.event !== "damaged_records") return;
      const refs = ((event as unknown as { refs?: string[] }).refs ?? []).map(String);
      const items = await vaultListItems().catch(() => []);
      setNames(refs.map((r) => items.find((i) => i.ref === r)?.title ?? "an item whose name can't be read"));
    }).then((s) => {
      stop = s;
    });
    return () => stop?.();
  }, []);

  if (!names) return null;
  return (
    <div className="flex items-start gap-3 rounded-2xl border border-red-500/30 bg-red-950/15 px-4 py-3">
      <ShieldAlert className="mt-0.5 h-4 w-4 shrink-0 text-red-400" />
      <div className="max-w-[65ch] text-sm leading-6 text-red-100">
        <p>
          {names.length === 1 ? "One saved item is" : `${names.length} saved items are`} damaged and can't be
          decrypted: <strong>{names.join(", ")}</strong>. The security change was stopped so nothing is lost.
        </p>
        <p className="mt-1">
          Restore an earlier version of each from its history (or delete it if you no longer need it), then try
          again.
        </p>
        <button onClick={() => setNames(null)} className="mt-2 cursor-pointer text-xs text-red-200 underline">
          Dismiss
        </button>
      </div>
    </div>
  );
}

export default DamagedItemsNotice;
