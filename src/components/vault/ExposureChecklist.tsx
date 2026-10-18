//! §12 scenario 8 (RC-08): after a device is removed, the breach checklist.
//! A Mac or a SOURCE Vault iPhone holds the whole vault (v0.5 §22.10), so
//! everything it could read is exposed: the user is asked to change the
//! password of every saved login at its website. The list shows titles and
//! sites only, from the metadata list; ticks are this screen's memory only.

import { useEffect, useState } from "react";
import { ShieldAlert } from "lucide-react";
import { vaultListItems, type VaultItemMeta } from "@/lib/vault";

export function ExposureChecklist({ deviceName, onClose }: { deviceName: string; onClose: () => void }) {
  const [logins, setLogins] = useState<VaultItemMeta[]>([]);
  const [done, setDone] = useState<Set<string>>(new Set());

  useEffect(() => {
    vaultListItems()
      .then((items) => setLogins(items.filter((i) => i.kind === "login" && !i.conflicted && !i.corrupt)))
      .catch(() => setLogins([]));
  }, []);

  const toggle = (ref: string) =>
    setDone((d) => {
      const next = new Set(d);
      if (next.has(ref)) next.delete(ref);
      else next.add(ref);
      return next;
    });

  return (
    <div className="mt-4 rounded-xl border border-amber-300/40 bg-amber-950/10 p-4">
      <div className="flex items-start gap-2">
        <ShieldAlert className="mt-0.5 h-4 w-4 shrink-0 text-amber-300" />
        <div className="max-w-[65ch] text-sm leading-6 text-amber-100">
          <p>
            <strong>{deviceName}</strong> was removed and can no longer open the vault or reach
            your backup. But if it was stolen or misused while it was unlocked, everything in the
            vault could have been read. Change these passwords at each website — start with email,
            banking and anything that can reset other accounts.
          </p>
          <p className="mt-1 text-xs text-amber-200/80">
            {done.size} of {logins.length} done. Ticks are not saved.
          </p>
        </div>
      </div>
      <ul className="mt-3 max-h-64 space-y-1 overflow-y-auto">
        {logins.map((l) => (
          <li key={l.ref}>
            <label className="flex cursor-pointer items-center gap-2 text-sm text-foreground">
              <input type="checkbox" checked={done.has(l.ref)} onChange={() => toggle(l.ref)} />
              <span className={done.has(l.ref) ? "line-through opacity-60" : ""}>
                {l.title}
                <span className="text-muted-foreground"> · {l.hosts.join(", ") || "no site"}</span>
              </span>
            </label>
          </li>
        ))}
      </ul>
      <button
        onClick={onClose}
        className="mt-3 cursor-pointer rounded-lg border border-border/70 px-3 py-1.5 text-xs text-foreground hover:bg-white/5"
      >
        Close checklist
      </button>
    </div>
  );
}

export default ExposureChecklist;
