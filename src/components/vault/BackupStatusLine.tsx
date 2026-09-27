//! Backup status (§11.3.2, §11.6): the worker's latest state, pushed on
//! `vault:backup`, plus "Back up now".

import { useEffect, useState } from "react";
import { Cloud, CloudOff } from "lucide-react";
import { type BackupStatus, onBackupStatus, vaultBackupNow, vaultBackupStatus, vaultErrorMessage } from "@/lib/vault";

function describe(s: BackupStatus | null, provider: string | null): string {
  if (!provider) return "Backup isn't configured in this build yet.";
  if (!s || s.state === "idle") return "Backup will run shortly.";
  const when = s.last_success ? new Date(s.last_success * 1000).toLocaleString() : null;
  switch (s.state) {
    case "working":
      return "Backing up…";
    case "ok":
      return `Backed up${when ? ` · ${when}` : ""}.`;
    case "offline":
      return `Backup service unreachable — will retry.${when ? ` Last backup ${when}.` : ""}`;
    case "clock_skew":
      return "Your Mac's clock is off, so the backup can't be checked. Fix the date and time.";
    default:
      return vaultErrorMessage(s.last_error ?? "BACKUP_UNAVAILABLE");
  }
}

export function BackupStatusLine() {
  const [status, setStatus] = useState<BackupStatus | null>(null);
  const [provider, setProvider] = useState<string | null>(null);

  useEffect(() => {
    vaultBackupStatus()
      .then((r) => {
        setStatus(r.status);
        setProvider(r.provider);
      })
      .catch(() => {});
    let stop: (() => void) | undefined;
    onBackupStatus(setStatus).then((s) => (stop = s));
    return () => stop?.();
  }, []);

  const bad = status && ["error", "access_lost", "conflict", "offline", "clock_skew"].includes(status.state);
  return (
    <div className="flex items-center justify-between rounded-xl border border-border/60 px-3 py-2 text-sm">
      <div className={`flex items-center gap-2 ${bad || status?.stale ? "text-amber-300" : "text-muted-foreground"}`}>
        {bad ? <CloudOff className="h-4 w-4" /> : <Cloud className="h-4 w-4" />}
        <span>{describe(status, provider)}</span>
        {status?.stale && status.state !== "working" && <span>No backup in the last 48 hours.</span>}
      </div>
      <button
        onClick={() => vaultBackupNow().catch(() => {})}
        disabled={!provider || status?.state === "working"}
        className="cursor-pointer rounded-lg border border-border/70 px-3 py-1 text-xs text-foreground transition-colors hover:bg-white/5 disabled:cursor-not-allowed disabled:opacity-50"
      >
        Back up now
      </button>
    </div>
  );
}
