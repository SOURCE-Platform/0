//! Joining a vault from your iPhone (F.2e, spec §22.13): this Mac has no
//! vault; the SOURCE Vault app on the iPhone adds it as a replacement Mac.
//! The QR here is scanned by the iPhone; the code to compare and the
//! master password are in the helper's own "Join a Vault" window.

import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Smartphone } from "lucide-react";
import { vaultErrorMessage } from "@/lib/vault";

interface JoinStatus {
  active: boolean;
  stage?: "waiting" | "comparing" | "confirmed" | "joined" | "failed";
  error?: string | null;
  expires_in?: number;
}

export function JoinFromPhoneCard({ onDone }: { onDone: () => void }) {
  const [qr, setQr] = useState<string | null>(null);
  const [status, setStatus] = useState<JoinStatus | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!qr) return;
    const timer = setInterval(async () => {
      try {
        const s = await invoke<JoinStatus>("vault_join_status");
        setStatus(s);
        if (s.stage === "joined") {
          clearInterval(timer);
          onDone();
        } else if (!s.active || s.stage === "failed") {
          clearInterval(timer);
          setQr(null);
          setError(s.error ? vaultErrorMessage(s.error) : "The code expired. Start again.");
        }
      } catch {
        /* the next poll retries */
      }
    }, 1000);
    return () => clearInterval(timer);
  }, [qr, onDone]);

  async function start() {
    setError(null);
    try {
      const r = await invoke<{ qr: string }>("vault_join_begin");
      setQr(r.qr);
    } catch (e) {
      setError(vaultErrorMessage(String(e)));
    }
  }

  async function cancel() {
    await invoke("vault_join_cancel");
    setQr(null);
    setStatus(null);
  }

  const button =
    "cursor-pointer rounded-xl border border-border/70 px-4 py-2 text-sm text-foreground transition-colors hover:bg-white/5";
  const step =
    status?.stage === "comparing"
      ? "Compare the code in the Source Vault window with your iPhone, then enter your master password there."
      : status?.stage === "confirmed"
        ? "Finishing on your iPhone…"
        : "On your iPhone, open SOURCE Vault → Security → Add a Mac, and scan this code. Keep the iPhone next to this Mac.";
  return (
    <div className="mt-4 rounded-2xl border border-border/70 bg-black/10 p-6">
      <div className="flex items-center gap-3">
        <Smartphone className="h-5 w-5 text-foreground" />
        <h2 className="text-lg font-medium text-foreground">Join your vault from your iPhone</h2>
      </div>
      <p className="mt-2 max-w-[60ch] text-sm leading-6 text-muted-foreground">
        Replacing a Mac? Your iPhone can add this Mac to your vault. Remove the old Mac on your iPhone first.
      </p>
      {qr ? (
        <>
          {/* An image, never inline markup: the SVG cannot run anything. */}
          <img alt="Code for your iPhone" className="mt-4 w-[260px] rounded-xl bg-white p-2" src={`data:image/svg+xml;utf8,${encodeURIComponent(qr)}`} />
          <p className="mt-3 max-w-[60ch] text-sm leading-6 text-foreground">{step}</p>
          <button className={`mt-3 ${button}`} onClick={cancel}>
            Cancel
          </button>
        </>
      ) : (
        <button className={`mt-4 ${button}`} onClick={start}>
          Show code for my iPhone
        </button>
      )}
      {error && <p className="mt-3 text-sm text-amber-300">{error}</p>}
    </div>
  );
}
