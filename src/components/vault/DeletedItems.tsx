//! "Recently deleted" (spec §22.4): tombstoned records with a Restore
//! button. A restore needs presence and brings the item back as a new
//! item; nothing secret is shown here.

import { useCallback, useEffect, useState } from "react";
import { RotateCcw, Trash2 } from "lucide-react";
import { vaultErrorMessage } from "@/lib/vault";
import { vaultListDeleted, vaultRestoreRevision, type DeletedItem } from "@/lib/vaultHistory";

export function DeletedItems({
  refreshKey,
  onRestored,
  onError,
}: {
  refreshKey: number;
  onRestored: () => void;
  onError: (message: string | null) => void;
}) {
  const [items, setItems] = useState<DeletedItem[]>([]);
  const [open, setOpen] = useState(false);
  const [busyRef, setBusyRef] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    try {
      setItems(await vaultListDeleted());
    } catch (e) {
      onError(vaultErrorMessage(String(e)));
    }
  }, [onError]);

  useEffect(() => {
    refresh();
  }, [refresh, refreshKey]);

  async function restore(item: DeletedItem) {
    setBusyRef(item.ref);
    onError(null);
    try {
      await vaultRestoreRevision(item.ref, item.revision_id);
      await refresh();
      onRestored();
    } catch (e) {
      onError(vaultErrorMessage(String(e)));
    } finally {
      setBusyRef(null);
    }
  }

  if (items.length === 0) return null;
  return (
    <div className="rounded-xl border border-border/70">
      <button
        onClick={() => setOpen((o) => !o)}
        className="flex w-full cursor-pointer items-center gap-2 px-4 py-3 text-sm text-muted-foreground hover:text-foreground"
      >
        <Trash2 className="h-4 w-4" /> Recently deleted ({items.length})
      </button>
      {open && (
        <ul className="divide-y divide-border/50 border-t border-border/50">
          {items.map((item) => (
            <li key={item.ref} className="flex items-center justify-between px-4 py-2 text-sm">
              <span className="truncate text-foreground">{item.title ?? "Untitled item"}</span>
              <button
                onClick={() => restore(item)}
                disabled={busyRef !== null}
                className="flex cursor-pointer items-center gap-1 rounded-lg px-2 py-1 text-xs text-foreground hover:bg-white/5 disabled:cursor-not-allowed disabled:opacity-50"
              >
                <RotateCcw className="h-3.5 w-3.5" /> Restore
              </button>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}

export default DeletedItems;
