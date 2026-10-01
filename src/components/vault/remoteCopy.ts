//! §11.3.2 UI rules for a change still on its way to the backup. A cutoff
//! ("no longer works") is claimed only after the backup accepted it;
//! security-driven work keeps a persistent warning until then.

import type { BackupStatus } from "@/lib/vault";

export type Banner = { tone: "warn" | "info"; text: string };

export function pendingBanner(s: BackupStatus | null): Banner | null {
  const p = s?.pending;
  // §22.14: a security change lost with a restored older copy.
  const lost = p?.lost_change ?? [];
  if (lost.length > 0) {
    const what = lost.includes("revocation")
      ? "a device removal"
      : lost.includes("rk_replacement")
        ? "a Recovery Key replacement"
        : lost.includes("mp_change")
          ? "a master password change"
          : "a security change";
    return { tone: "warn", text: `An older copy of this vault was restored, and ${what} made on this Mac was lost. Please do it again.` };
  }
  if (!p?.pending) return null;
  const ops = p.ops ?? [];
  const redo = p.needs_user
    ? "Another device changed your vault's security settings first. Your change was not applied — please make it again."
    : null;
  // Security-driven work keeps its warning throughout (§11.3), also while
  // the user is asked to redo it.
  const security = p.revocation_failed
    ? "Not yet cut off at your backup: the removed device — keep this Mac online."
    : p.security_driven
      ? "Not yet cut off at your backup — keep this Mac online."
      : null;
  if (security || redo) {
    return { tone: "warn", text: [security, redo].filter(Boolean).join(" ") };
  }
  if (ops.includes("vault_create")) return { tone: "info", text: "Your vault hasn't reached the backup yet." };
  if (ops.includes("mp_change")) {
    return { tone: "info", text: "Your backup still accepts your previous master password until this Mac reaches it." };
  }
  if (ops.includes("rk_replacement")) {
    return { tone: "info", text: "Your backup still accepts your previous Recovery Key until this Mac reaches it." };
  }
  if (ops.includes("enrollment")) {
    return { tone: "info", text: "Your new device can use the backup once this Mac reaches it." };
  }
  return null;
}

/** Post-commit copy (§11.3.2): shown once the backup accepted the change. */
export function clearedNotice(s: BackupStatus | null): string | null {
  const c = s?.cleared ?? [];
  if (c.includes("mp_change")) return "Your backup now uses the new master password.";
  if (c.includes("rk_replacement")) return "The previous Recovery Key no longer works.";
  return null;
}
