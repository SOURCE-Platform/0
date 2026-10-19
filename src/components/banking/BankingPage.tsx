import { useState } from "react";
import { RefreshCw, ShieldCheck } from "lucide-react";
import { Button } from "@/components/ui/button";
import { PageHeader } from "@/components/ui/page";
import { BankingSetup } from "./BankingSetup";
import { BankAccounts } from "./BankAccounts";
import { BankTransactions } from "./BankTransactions";
import { formatTimestamp } from "./format";
import { useBanking } from "./useBanking";

export default function BankingPage({ embedded = false }: { embedded?: boolean }) {
  const banking = useBanking();
  const { overview, busy, error, run } = banking;
  const [confirmDisconnect, setConfirmDisconnect] = useState(false);
  const working = busy !== null;
  const expired = overview?.consentValidUntil
    ? new Date(overview.consentValidUntil).getTime() <= Date.now() : false;
  return (
    <div className={embedded ? "space-y-6" : "mx-auto w-full max-w-6xl space-y-6 pb-12"}>
      {!embedded && <PageHeader title="Banking" description={
        <span className="block max-w-[60ch]">Your financial activity, connected to SOURCE. Start with a read-only Bank Millennium feed, not a budget or an AI guess.</span>
      } />}
      <div className="ui-panel flex items-start gap-3">
        <ShieldCheck aria-hidden="true" className="mt-0.5 h-4 w-4 shrink-0 text-muted-foreground" />
        <p className="max-w-[60ch] text-sm leading-6 text-muted-foreground">
          Personal-use preview. Bank records are encrypted locally; integration keys stay in macOS Keychain.
          No payments, cloud backup, or automatic sharing with AI models.
          Live Millennium access and history completeness still need validation.
        </p>
      </div>
      {!banking.desktop && <p role="status" className="max-w-[60ch] text-sm text-muted-foreground">Open SOURCE’s desktop app to configure banking. This browser view cannot access Keychain or bank records.</p>}
      {busy === "loading" && <p role="status" className="text-sm text-muted-foreground">Loading banking connection…</p>}
      {error && <div role="alert" className="max-w-[60ch] rounded-lg border border-destructive/30 p-4 text-sm leading-6 text-destructive">{error}</div>}
      {error && banking.protectedSurface && <Button type="button" variant="outline" disabled={working}
        onClick={() => run("banking_overview")}>Refresh connection status</Button>}
      {overview && !overview.supported && <p className="max-w-[60ch] text-sm text-muted-foreground">Secure banking storage currently requires macOS. Windows and Linux are not enabled yet.</p>}
      {overview?.supported && banking.protectedSurface && <>
        <section className="ui-panel space-y-4" aria-label="Bank connection">
          <div className="flex flex-wrap items-start justify-between gap-4">
            <div className="space-y-1">
              <h2 className="text-sm font-medium">Bank Millennium · Poland</h2>
              <p className="text-sm text-muted-foreground">{!overview.configured ? "Not configured" : !overview.connected ? "Not connected" : expired ? "Consent expired. Reconnect to resume imports." : "Connected through Enable Banking"}</p>
              <p className="text-xs text-muted-foreground">Last successful sync: {formatTimestamp(overview.lastSyncedAt)}</p>
              {overview.consentValidUntil && <p className="text-xs text-muted-foreground">Consent expires: {formatTimestamp(overview.consentValidUntil)}</p>}
            </div>
            <div className="flex flex-wrap gap-2">
              {overview.connected && <Button type="button" variant="outline" disabled={working || expired} onClick={() => run("banking_sync")}>
                <RefreshCw aria-hidden="true" /> Sync now
              </Button>}
              <Button type="button" disabled={working || !overview.configured} onClick={() => run("banking_connect")}>
                {busy === "banking_connect" ? "Authorizing and importing…" : overview.connected ? "Reconnect bank" : "Connect Millennium"}
              </Button>
              {busy === "banking_connect" && <Button type="button" variant="outline" onClick={banking.cancel}>Cancel authorization</Button>}
            </div>
          </div>
          <p className="max-w-[60ch] text-sm leading-6 text-muted-foreground">
            Sign in only on your bank’s page in the system browser. SOURCE imports the longest available history
            after authorization. Updates are periodic, not instant, and older history may require statement import.
            Screen capture and OCR pause while this view or bank authorization is open.
          </p>
          {overview.lastError && overview.lastError !== error && <p role="alert" className="max-w-[60ch] text-sm text-destructive">{overview.lastError}</p>}
          {overview.connected && <>
            <label className="flex max-w-[60ch] cursor-pointer items-start gap-3 text-sm leading-6 has-[:disabled]:cursor-not-allowed">
              <input type="checkbox" className="mt-1.5 cursor-pointer disabled:cursor-not-allowed" checked={overview.autoSync} disabled={working}
                onChange={(e) => run("banking_set_auto_sync", { enabled: e.target.checked })} />
              <span>Sync in the background about every six hours while SOURCE is running. Bank limits and reauthorization still apply.</span>
            </label>
            {confirmDisconnect ? <div className="space-y-3">
              <p className="max-w-[60ch] text-sm leading-6 text-muted-foreground">Revoke all stored bank sessions and stop syncing? Imported records stay encrypted on this Mac. This does not delete your bank account.</p>
              <div className="flex gap-2">
                <Button type="button" variant="destructive" disabled={working} onClick={async () => {
                  await run("banking_disconnect"); setConfirmDisconnect(false);
                }}>Confirm disconnect</Button>
                <Button type="button" variant="outline" disabled={working} onClick={() => setConfirmDisconnect(false)}>Keep connected</Button>
              </div>
            </div> : <Button type="button" variant="outline" disabled={working} onClick={() => setConfirmDisconnect(true)}>Disconnect bank</Button>}
          </>}
        </section>
        {!overview.connected && <BankingSetup configured={overview.configured} busy={working}
          onConfigure={(applicationId) => run("banking_configure", { applicationId })} />}
        <BankAccounts accounts={overview.accounts} />
        {overview.configured && <BankTransactions page={banking.page} accounts={overview.accounts} query={banking.query}
          onFilter={banking.filter} busy={working} />}
      </>}
    </div>
  );
}
