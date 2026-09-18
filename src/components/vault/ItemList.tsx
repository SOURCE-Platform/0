//! Vault item list (Phase C): metadata rows with one-shot reveal and
//! delete. Revealed secrets live only in local state, auto-hide after
//! 15 s, and clear on unmount (leaving the tab drops the surface).

import { useCallback, useEffect, useState } from "react";
import { Eye, Trash2 } from "lucide-react";
import {
  vaultDeleteItem,
  vaultErrorMessage,
  vaultListItems,
  vaultReveal,
  type VaultItemMeta,
} from "@/lib/vault";

const REVEAL_TTL_MS = 15_000;

export function ItemList({
  refreshKey,
  onError,
}: {
  refreshKey: number;
  onError: (message: string | null) => void;
}) {
  const [items, setItems] = useState<VaultItemMeta[]>([]);
  const [revealed, setRevealed] = useState<Record<string, Record<string, string>>>({});
  const [confirmingDelete, setConfirmingDelete] = useState<string | null>(null);
  const [busyRef, setBusyRef] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    try {
      setItems(await vaultListItems());
    } catch (e) {
      onError(vaultErrorMessage(String(e)));
    }
  }, [onError]);

  useEffect(() => {
    refresh();
  }, [refresh, refreshKey]);

  // Reveal TTL: wipe revealed secrets from state (and thus the DOM).
  useEffect(() => {
    const refs = Object.keys(revealed);
    if (refs.length === 0) return;
    const timer = setTimeout(() => setRevealed({}), REVEAL_TTL_MS);
    return () => clearTimeout(timer);
  }, [revealed]);

  // Never carry revealed secrets across unmount/tab switches.
  useEffect(() => () => setRevealed({}), []);

  async function reveal(item: VaultItemMeta) {
    setBusyRef(item.ref);
    onError(null);
    try {
      const secret = await vaultReveal(item.ref);
      setRevealed({ [item.ref]: secret });
    } catch (e) {
      onError(vaultErrorMessage(String(e)));
    } finally {
      setBusyRef(null);
    }
  }

  async function remove(item: VaultItemMeta) {
    setBusyRef(item.ref);
    onError(null);
    try {
      await vaultDeleteItem(item.ref);
      setConfirmingDelete(null);
      await refresh();
    } catch (e) {
      onError(vaultErrorMessage(String(e)));
    } finally {
      setBusyRef(null);
    }
  }

  if (items.length === 0) {
    return (
      <div className="rounded-2xl border border-border/70 bg-black/10 p-6">
        <p className="max-w-[60ch] text-sm leading-6 text-muted-foreground">
          No items yet. Add a synthetic login to exercise the vault end to end.
        </p>
      </div>
    );
  }

  return (
    <div className="overflow-hidden rounded-2xl border border-border/70">
      {items.map((item) => (
        <div
          key={item.ref}
          className="flex items-center justify-between gap-4 border-b border-border/40 bg-black/10 px-5 py-4 last:border-b-0"
        >
          <div className="min-w-0">
            <div className="flex items-center gap-2">
              <span className="truncate text-sm font-medium text-foreground">
                {item.title}
              </span>
              {item.conflicted && (
                <span className="rounded-md bg-amber-500/15 px-1.5 py-0.5 text-[10px] font-medium text-amber-300">
                  conflicted
                </span>
              )}
              {item.corrupt && (
                <span className="rounded-md bg-red-500/15 px-1.5 py-0.5 text-[10px] font-medium text-red-300">
                  damaged
                </span>
              )}
            </div>
            <div className="mt-0.5 truncate text-xs text-muted-foreground">
              {item.username ?? "—"} · {item.hosts.join(", ") || "no host"}
            </div>
            {revealed[item.ref] && (
              <div className="mt-2 rounded-lg border border-green-500/30 bg-green-950/20 px-3 py-2">
                {Object.entries(revealed[item.ref]).map(([field, value]) => (
                  <div key={field} className="flex gap-2 text-xs">
                    <span className="w-20 shrink-0 text-muted-foreground">{field}</span>
                    <span className="select-all break-all font-mono text-green-100">
                      {value}
                    </span>
                  </div>
                ))}
                <p className="mt-1 text-[10px] text-muted-foreground">
                  Hides automatically after 15 seconds.
                </p>
              </div>
            )}
          </div>
          <div className="flex shrink-0 items-center gap-2">
            {confirmingDelete === item.ref ? (
              <>
                <button
                  onClick={() => remove(item)}
                  disabled={busyRef === item.ref}
                  className="cursor-pointer rounded-lg bg-red-500/80 px-3 py-1.5 text-xs font-medium text-white hover:bg-red-500 disabled:cursor-not-allowed disabled:opacity-50"
                >
                  Confirm delete
                </button>
                <button
                  onClick={() => setConfirmingDelete(null)}
                  className="cursor-pointer rounded-lg border border-border/70 px-3 py-1.5 text-xs text-foreground hover:bg-white/5"
                >
                  Cancel
                </button>
              </>
            ) : (
              <>
                <button
                  onClick={() => reveal(item)}
                  disabled={busyRef === item.ref}
                  className="flex cursor-pointer items-center gap-1.5 rounded-lg border border-border/70 px-3 py-1.5 text-xs text-foreground hover:bg-white/5 disabled:cursor-not-allowed disabled:opacity-50"
                >
                  <Eye className="h-3.5 w-3.5" />
                  {busyRef === item.ref ? "Verifying…" : "Reveal"}
                </button>
                <button
                  onClick={() => setConfirmingDelete(item.ref)}
                  disabled={busyRef === item.ref}
                  aria-label={`Delete ${item.title}`}
                  className="cursor-pointer rounded-lg border border-border/70 p-1.5 text-muted-foreground hover:bg-white/5 hover:text-red-300 disabled:cursor-not-allowed disabled:opacity-50"
                >
                  <Trash2 className="h-3.5 w-3.5" />
                </button>
              </>
            )}
          </div>
        </div>
      ))}
    </div>
  );
}

export default ItemList;
