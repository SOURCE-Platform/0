//! Devices section of the Vault tab (§5, §11.4).
//!
//! Enrollment is deliberately a two-screen ritual: the phone scans the QR,
//! both devices derive the same 8-character code from the same transcript,
//! and the user confirms only if they match. Nothing about that comparison
//! is automated here — a code that arrived over the network instead of
//! being derived independently would prove nothing.

import { useCallback, useEffect, useRef, useState } from "react";
import { Laptop, Smartphone, QrCode, ShieldAlert } from "lucide-react";
import {
  vaultBeginEnrollment,
  vaultCancelEnrollment,
  vaultConfirmEnrollment,
  vaultEnrollmentStatus,
  vaultErrorMessage,
  vaultListDevices,
  vaultRevokeDevice,
  type EnrollmentStart,
  type EnrollmentStatus,
  type VaultDevice,
} from "@/lib/vault";

/// "Added 14:11, 21 Sep" — the only thing distinguishing two devices
/// that report the same model name.
function added(device: VaultDevice): string {
  if (!device.enrolled_at) return `entry ${device.installed_seq}`;
  const when = new Date(device.enrolled_at * 1000);
  return `Added ${when.toLocaleString(undefined, {
    day: "numeric",
    month: "short",
    hour: "2-digit",
    minute: "2-digit",
  })}`;
}

export function DevicesPanel({ onError }: { onError: (message: string) => void }) {
  const [devices, setDevices] = useState<VaultDevice[]>([]);
  const [enrollment, setEnrollment] = useState<EnrollmentStart | null>(null);
  const [status, setStatus] = useState<EnrollmentStatus | null>(null);
  const [busy, setBusy] = useState(false);
  const poll = useRef<number | null>(null);

  const refresh = useCallback(async () => {
    try {
      setDevices(await vaultListDevices());
    } catch (e) {
      onError(vaultErrorMessage(String(e)));
    }
  }, [onError]);

  useEffect(() => {
    refresh();
    // The list only changes through actions on this Mac, so focus is the
    // moment worth re-reading it — no polling, and no device can change
    // it from outside.
    const onFocus = () => refresh();
    window.addEventListener("focus", onFocus);
    return () => window.removeEventListener("focus", onFocus);
  }, [refresh]);

  // Poll only while a QR is on screen; the helper is the source of truth.
  useEffect(() => {
    if (!enrollment) return;
    const tick = async () => {
      try {
        const next = await vaultEnrollmentStatus();
        setStatus(next);
        if (!next.active) {
          setEnrollment(null);
          refresh();
        } else if (next.acked) {
          setEnrollment(null);
          setStatus(null);
          await vaultCancelEnrollment();
          refresh();
        }
      } catch {
        /* transient; the next tick retries */
      }
    };
    poll.current = window.setInterval(tick, 1000);
    return () => {
      if (poll.current) window.clearInterval(poll.current);
    };
  }, [enrollment, refresh]);

  useEffect(() => () => void vaultCancelEnrollment().catch(() => {}), []);

  const start = async () => {
    setBusy(true);
    try {
      setEnrollment(await vaultBeginEnrollment());
      setStatus(null);
    } catch (e) {
      onError(vaultErrorMessage(String(e)));
    } finally {
      setBusy(false);
    }
  };

  const confirm = async () => {
    setBusy(true);
    try {
      await vaultConfirmEnrollment();
    } catch (e) {
      onError(vaultErrorMessage(String(e)));
    } finally {
      setBusy(false);
    }
  };

  const cancel = async () => {
    await vaultCancelEnrollment().catch(() => {});
    setEnrollment(null);
    setStatus(null);
  };

  const revoke = async (device: VaultDevice) => {
    setBusy(true);
    try {
      await vaultRevokeDevice(device.device_id);
      await refresh();
    } catch (e) {
      onError(vaultErrorMessage(String(e)));
    } finally {
      setBusy(false);
    }
  };

  // "Devices" means devices that can open this vault. A revoked one
  // cannot, but it stays visible as history: a removal you did not make
  // is something you need to be able to see.
  const active = devices.filter((d) => !d.revoked);
  const removed = devices.filter((d) => d.revoked);

  return (
    <div className="rounded-2xl border border-border/70 bg-black/10 p-5">
      <div className="flex items-center justify-between">
        <h3 className="text-sm font-medium text-foreground">Devices</h3>
        {!enrollment && (
          <button
            onClick={start}
            disabled={busy}
            className="flex cursor-pointer items-center gap-2 rounded-xl border border-border/70 px-3 py-1.5 text-sm text-foreground transition-colors hover:bg-white/5 disabled:cursor-not-allowed disabled:opacity-50"
          >
            <QrCode className="h-4 w-4" /> Add device
          </button>
        )}
      </div>

      <p className="mt-1 max-w-[60ch] text-xs text-muted-foreground">
        Devices allowed to open this vault. Removing one here is the only way to revoke
        access — a device that deletes its own copy stays on this list until you do.
      </p>

      <ul className="mt-4 space-y-2">
        {active.map((device) => (
          <li
            key={device.device_id}
            className="flex items-center justify-between rounded-xl border border-border/50 px-3 py-2"
          >
            <span className="flex items-center gap-2 text-sm text-foreground">
              {device.platform === 2 ? (
                <Smartphone className="h-4 w-4 shrink-0 text-muted-foreground" />
              ) : (
                <Laptop className="h-4 w-4 shrink-0 text-muted-foreground" />
              )}
              <span className="flex flex-col">
                <span className="flex items-center gap-2">
                  {device.device_name}
                  {device.self && (
                    <span className="text-xs text-muted-foreground">this Mac</span>
                  )}
                </span>
                {/* Two devices can share a name; when it was added is
                    what tells them apart. */}
                <span className="text-xs text-muted-foreground">{added(device)}</span>
              </span>
            </span>
            {!device.self && (
              <button
                onClick={() => revoke(device)}
                disabled={busy}
                className="cursor-pointer rounded-lg border border-border/50 px-2 py-1 text-xs text-muted-foreground transition-colors hover:bg-white/5 disabled:cursor-not-allowed disabled:opacity-50"
              >
                Remove
              </button>
            )}
          </li>
        ))}
        {active.length === 0 && (
          <li className="text-sm text-muted-foreground">No devices enrolled yet.</li>
        )}
      </ul>

      {removed.length > 0 && (
        <details className="mt-3">
          <summary className="cursor-pointer text-xs text-muted-foreground">
            Removed ({removed.length})
          </summary>
          <ul className="mt-2 space-y-2">
            {removed.map((device) => (
              <li
                key={device.device_id}
                className="flex items-center justify-between rounded-xl border border-border/40 px-3 py-2"
              >
                <span className="flex items-center gap-2 text-sm text-muted-foreground">
                  {device.platform === 2 ? (
                    <Smartphone className="h-4 w-4 shrink-0" />
                  ) : (
                    <Laptop className="h-4 w-4 shrink-0" />
                  )}
                  <span className="flex flex-col">
                    {device.device_name}
                    <span className="text-xs">{added(device)}</span>
                  </span>
                </span>
                <span className="text-xs text-muted-foreground">
                  no longer has access
                </span>
              </li>
            ))}
          </ul>
        </details>
      )}

      {enrollment && (
        <div className="mt-5 rounded-xl border border-border/50 p-4">
          {!status?.sas ? (
            <div className="flex gap-4">
              <div
                className="h-[160px] w-[160px] shrink-0 overflow-hidden rounded-lg bg-white p-1 [&>svg]:h-full [&>svg]:w-full"
                dangerouslySetInnerHTML={{ __html: enrollment.qr }}
              />
              <div className="text-sm text-muted-foreground">
                <p className="text-foreground">Scan this with Source on your iPhone.</p>
                <p className="mt-2 max-w-[48ch] leading-6">
                  The code expires in {status?.expires_in ?? enrollment.expires_in} seconds and
                  works once. Both devices will then show an eight-character code to compare.
                </p>
                <button
                  onClick={cancel}
                  className="mt-3 cursor-pointer rounded-lg border border-border/50 px-2 py-1 text-xs text-muted-foreground hover:bg-white/5"
                >
                  Cancel
                </button>
              </div>
            </div>
          ) : (
            <div>
              <p className="text-sm text-foreground">
                Compare this code with the one on your iPhone.
              </p>
              <p className="mt-3 font-mono text-2xl tracking-[0.3em] text-foreground">
                {status.sas}
              </p>
              <p className="mt-3 flex max-w-[52ch] items-start gap-2 text-sm leading-6 text-muted-foreground">
                <ShieldAlert className="mt-0.5 h-4 w-4 shrink-0 text-amber-300" />
                If the codes are different, do not continue — something is intercepting the
                connection.
              </p>
              <div className="mt-4 flex gap-2">
                <button
                  onClick={confirm}
                  disabled={busy}
                  className="cursor-pointer rounded-xl bg-white px-3 py-1.5 text-sm font-medium text-black transition-opacity hover:opacity-90 disabled:cursor-not-allowed disabled:opacity-50"
                >
                  {busy ? "Waiting for Touch ID…" : "The codes match"}
                </button>
                <button
                  onClick={cancel}
                  className="cursor-pointer rounded-xl border border-border/70 px-3 py-1.5 text-sm text-foreground transition-colors hover:bg-white/5"
                >
                  They don't match
                </button>
              </div>
            </div>
          )}
        </div>
      )}
    </div>
  );
}

export default DevicesPanel;
