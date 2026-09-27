//! Backup status (§11.3.2, §11.6): the worker's latest state, pushed on
//! `vault:backup`, plus "Back up now".

import { useEffect, useState } from "react";
import { Cloud, CloudOff } from "lucide-react";
import {
  type BackupStatus,
  onBackupStatus,
  vaultBackupNow,
  vaultBackupStatus,
  vaultErrorMessage,
  vaultSetupRetryHandle,
} from "@/lib/vault";
import { clearedNotice, pendingBanner } from "./remoteCopy";

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

  const bad = status && ["error", "access_lost", "conflict", "offline", "clock_skew", "handle_taken"].includes(status.state);
  return (
    <div className="space-y-2">
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
      <PendingBanner status={status} />
      {status?.state === "handle_taken" && <RetryHandle />}
    </div>
  );
}

function PendingBanner({ status }: { status: BackupStatus | null }) {
  const banner = pendingBanner(status);
  const notice = clearedNotice(status);
  if (banner) {
    const tone = banner.tone === "warn" ? "border-amber-300/50 text-amber-300" : "border-border/60 text-muted-foreground";
    return <div className={`rounded-xl border px-3 py-2 text-sm ${tone}`}>{banner.text}</div>;
  }
  if (notice) return <div className="rounded-xl border border-border/60 px-3 py-2 text-sm text-green-300">{notice}</div>;
  return null;
}

/** BK-28: choose another recovery name; a new Recovery Key follows. */
function RetryHandle() {
  const [handle, setHandle] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const submit = () => {
    setBusy(true);
    setError(null);
    vaultSetupRetryHandle(handle.trim())
      .catch((e) => setError(vaultErrorMessage(String(e))))
      .finally(() => setBusy(false));
  };
  return (
    <div className="rounded-xl border border-amber-300/40 px-3 py-3 text-sm">
      <p className="text-muted-foreground">
        Pick another recovery name. You will get a new Recovery Key — throw away the sheet you
        printed before; it opens nothing.
      </p>
      <div className="mt-2 flex gap-2">
        <input
          value={handle}
          onChange={(e) => setHandle(e.target.value)}
          placeholder="New recovery name"
          autoComplete="off"
          className="w-full max-w-sm rounded-lg border border-border/70 bg-transparent px-3 py-1 text-sm text-foreground outline-none focus:border-white/40"
        />
        <button
          onClick={submit}
          disabled={busy || handle.trim().length < 3}
          className="cursor-pointer rounded-lg bg-white px-3 py-1 text-xs font-medium text-black disabled:cursor-not-allowed disabled:opacity-50"
        >
          {busy ? "Waiting for the secure panel…" : "Use this name"}
        </button>
      </div>
      {error && <p className="mt-2 text-amber-300">{error}</p>}
    </div>
  );
}
