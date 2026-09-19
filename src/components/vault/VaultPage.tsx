//! Vault tab (Phase C): state-driven screen over the helper's lifecycle.
//!
//! - UNINITIALIZED → create-vault call to action (helper panel collects MP)
//! - LOCKED → unlock call to action (helper panel collects MP)
//! - UNLOCKED → item list + add form
//! - ERROR / COMPROMISED / helper-unavailable → honest status cards
//!
//! §14.2 wiring: while this tab is displayed, the whole SOURCE window is
//! a sensitive capture surface (counter up); leaving the tab drops it.
//! Helper panels bump the same counter themselves via `secure_panel_visible`.

import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { KeyRound, Lock, Plus, ShieldAlert, ShieldCheck } from "lucide-react";
import {
  onVaultEvent,
  vaultChangeMasterPassword,
  vaultErrorMessage,
  vaultLock,
  vaultSetup,
  vaultState,
  vaultUnlock,
  type VaultState,
} from "@/lib/vault";
import { ItemList } from "./ItemList";
import { AddLoginForm } from "./AddLoginForm";

export function VaultPage() {
  const [state, setState] = useState<VaultState>("unknown");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [adding, setAdding] = useState(false);
  const [refreshKey, setRefreshKey] = useState(0);

  const refreshState = useCallback(async () => {
    try {
      setState(await vaultState());
    } catch (e) {
      setError(vaultErrorMessage(String(e)));
    }
  }, []);

  useEffect(() => {
    refreshState();
  }, [refreshState]);

  // §14.2: this surface is capture-sensitive exactly while displayed.
  useEffect(() => {
    invoke("sensitive_capture_surface_changed", { open: true }).catch(() => {});
    return () => {
      invoke("sensitive_capture_surface_changed", { open: false }).catch(() => {});
    };
  }, []);

  // Helper events drive the UI state directly (state/locked).
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    onVaultEvent((event) => {
      if (event.event === "state" && event.state) {
        setState(event.state as VaultState);
      }
      if (event.event === "locked") {
        setState("locked");
        setAdding(false);
      }
      if (event.event === "capture_unsafe") {
        setError(vaultErrorMessage("CAPTURE_UNSAFE"));
      }
    }).then((stop) => {
      unlisten = stop;
    });
    return () => unlisten?.();
  }, []);

  async function run(action: () => Promise<void>) {
    setBusy(true);
    setError(null);
    try {
      await action();
      await refreshState();
      setRefreshKey((k) => k + 1);
    } catch (e) {
      setError(vaultErrorMessage(String(e)));
      await refreshState();
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="mx-auto max-w-4xl px-2 pb-16">
      <div className="flex items-center justify-between">
        <div>
          <h1 className="text-2xl font-semibold tracking-tight text-foreground">Vault</h1>
          <p className="mt-1 max-w-[60ch] text-sm leading-6 text-muted-foreground">
            Credentials live in a separate, code-signed helper process. Secrets are typed
            into the helper's own secure panel and never pass through this window's
            JavaScript. Phase C uses synthetic credentials only.
          </p>
        </div>
        {(state === "unlocked" || state === "authorizing") && (
          <div className="flex items-center gap-2">
            <button
              onClick={() => run(vaultChangeMasterPassword)}
              disabled={busy || state !== "unlocked"}
              className="flex cursor-pointer items-center gap-2 rounded-xl border border-border/70 px-4 py-2 text-sm text-foreground transition-colors hover:bg-white/5 disabled:cursor-not-allowed disabled:opacity-50"
            >
              <KeyRound className="h-4 w-4" /> Change master password
            </button>
            {/* Never disabled: Lock must work while a panel/presence op is
                in flight — the helper applies it immediately (§13.3). */}
            <button
              onClick={() => {
                vaultLock()
                  .then(refreshState)
                  .catch((e) => setError(vaultErrorMessage(String(e))));
              }}
              className="flex cursor-pointer items-center gap-2 rounded-xl border border-border/70 px-4 py-2 text-sm text-foreground transition-colors hover:bg-white/5 disabled:cursor-not-allowed disabled:opacity-50"
            >
              <Lock className="h-4 w-4" /> Lock now
            </button>
          </div>
        )}
      </div>

      {error && (
        <div className="mt-4 flex items-start gap-3 rounded-2xl border border-red-500/30 bg-red-950/15 px-4 py-3">
          <ShieldAlert className="mt-0.5 h-4 w-4 shrink-0 text-red-400" />
          <p className="max-w-[60ch] text-sm leading-6 text-red-100">{error}</p>
        </div>
      )}

      <div className="mt-6">
        {state === "helper-unavailable" && (
          <StatusCard
            icon={<ShieldAlert className="h-5 w-5 text-amber-400" />}
            title="Vault helper not available"
            body="The signed SourceVaultHelper bundle was not found or failed verification. Build it with scripts/build-helper.sh, then reopen this tab."
          />
        )}
        {state === "uninitialized" && (
          <div className="rounded-2xl border border-border/70 bg-black/10 p-6">
            <div className="flex items-center gap-3">
              <KeyRound className="h-5 w-5 text-foreground" />
              <h2 className="text-lg font-medium text-foreground">Create your vault</h2>
            </div>
            <p className="mt-2 max-w-[60ch] text-sm leading-6 text-muted-foreground">
              Choosing a master password happens in the helper's native secure panel —
              the password is never visible to this window. After creation the vault
              starts locked.
            </p>
            <button
              onClick={() => run(vaultSetup)}
              disabled={busy}
              className="mt-4 cursor-pointer rounded-xl bg-white px-4 py-2 text-sm font-medium text-black transition-opacity hover:opacity-90 disabled:cursor-not-allowed disabled:opacity-50"
            >
              {busy ? "Waiting for the secure panel…" : "Create vault"}
            </button>
          </div>
        )}
        {(state === "locked" || state === "unlocking") && (
          <div className="rounded-2xl border border-border/70 bg-black/10 p-6">
            <div className="flex items-center gap-3">
              <Lock className="h-5 w-5 text-foreground" />
              <h2 className="text-lg font-medium text-foreground">Vault is locked</h2>
            </div>
            <p className="mt-2 max-w-[60ch] text-sm leading-6 text-muted-foreground">
              Unlock happens in the helper's secure panel. The password never crosses
              this window.
            </p>
            <button
              onClick={() => run(vaultUnlock)}
              disabled={busy || state === "unlocking"}
              className="mt-4 cursor-pointer rounded-xl bg-white px-4 py-2 text-sm font-medium text-black transition-opacity hover:opacity-90 disabled:cursor-not-allowed disabled:opacity-50"
            >
              {busy || state === "unlocking" ? "Waiting for the secure panel…" : "Unlock"}
            </button>
          </div>
        )}
        {state === "error" && (
          <StatusCard
            icon={<ShieldAlert className="h-5 w-5 text-red-400" />}
            title="Vault error"
            body="The vault database failed its integrity checks. The helper never deletes vault data; restore flows land in a later phase."
          />
        )}
        {/* AUTHORIZING is the unlocked vault waiting on a Touch ID/password
            check for one op (§13.3). Keep the list mounted through it: an
            in-flight reveal resolves into ItemList's state, and swapping in
            a status card would unmount it and drop the revealed secret. */}
        {(state === "unlocked" || state === "authorizing") && (
          <div className="space-y-4">
            <div className="flex items-center justify-between">
              {state === "authorizing" ? (
                <div className="flex items-center gap-2 text-sm text-muted-foreground">
                  <KeyRound className="h-4 w-4" /> Waiting for Touch ID or your Mac password…
                </div>
              ) : (
                <div className="flex items-center gap-2 text-sm text-green-300">
                  <ShieldCheck className="h-4 w-4" /> Unlocked — auto-locks after inactivity,
                  sleep, or screen lock.
                </div>
              )}
              <button
                onClick={() => setAdding((a) => !a)}
                className="flex cursor-pointer items-center gap-2 rounded-xl border border-border/70 px-3 py-1.5 text-sm text-foreground transition-colors hover:bg-white/5"
              >
                <Plus className="h-4 w-4" /> Add login
              </button>
            </div>
            {adding && (
              <AddLoginForm
                onDone={() => {
                  setAdding(false);
                  setRefreshKey((k) => k + 1);
                }}
                onError={(message) => setError(message)}
              />
            )}
            <ItemList refreshKey={refreshKey} onError={(m) => setError(m)} />
          </div>
        )}
        {(state === "unknown" || state === "compromised") && (
          <StatusCard
            icon={<KeyRound className="h-5 w-5 text-muted-foreground" />}
            title={`Vault state: ${state}`}
            body="This state is managed by the helper; follow its prompts."
          />
        )}
      </div>
    </div>
  );
}

function StatusCard({
  icon,
  title,
  body,
}: {
  icon: React.ReactNode;
  title: string;
  body: string;
}) {
  return (
    <div className="rounded-2xl border border-border/70 bg-black/10 p-6">
      <div className="flex items-center gap-3">
        {icon}
        <h2 className="text-lg font-medium text-foreground">{title}</h2>
      </div>
      <p className="mt-2 max-w-[60ch] text-sm leading-6 text-muted-foreground">{body}</p>
    </div>
  );
}

export default VaultPage;
