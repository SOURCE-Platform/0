import { useState } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { SettingsPanel } from "@/components/ui/page";

const SIGN_IN = "https://enablebanking.com/sign-in/";
const APPLICATIONS = "https://enablebanking.com/cp/applications";
const ACTIVATION_GUIDE = "https://enablebanking.com/docs/api/linked-accounts/";

export function BankingSetup({ configured, busy, onConfigure }: {
  configured: boolean;
  busy: boolean;
  onConfigure: (applicationId: string) => Promise<void>;
}) {
  const [applicationId, setApplicationId] = useState("");
  const [linkError, setLinkError] = useState<string | null>(null);
  async function openLink(url: string) {
    setLinkError(null);
    try { await openUrl(url); }
    catch { setLinkError(`Could not open your browser. Open this address manually: ${url}`); }
  }
  return (
    <SettingsPanel title={configured ? "Replace integration credentials" : "Set up Enable Banking"}
      description="Set this up once to let SOURCE read your Millennium accounts. You will create a personal integration, not a new bank account.">
      <div className="flex flex-wrap gap-2">
        <Button type="button" asChild variant="outline" disabled={busy}>
          <a href={SIGN_IN} target="_blank" rel="noreferrer" aria-disabled={busy}
            className={busy ? "pointer-events-none cursor-not-allowed" : "cursor-pointer"}
            tabIndex={busy ? -1 : undefined}
            onClick={(event) => { event.preventDefault(); if (!busy) void openLink(SIGN_IN); }}>
            1. Register or sign in
          </a>
        </Button>
        <Button type="button" asChild variant="outline" disabled={busy}>
          <a href={APPLICATIONS} target="_blank" rel="noreferrer" aria-disabled={busy}
            className={busy ? "pointer-events-none cursor-not-allowed" : "cursor-pointer"}
            tabIndex={busy ? -1 : undefined}
            onClick={(event) => { event.preventDefault(); if (!busy) void openLink(APPLICATIONS); }}>
            2. Open API applications
          </a>
        </Button>
      </div>
      <ol className="max-w-[60ch] list-decimal space-y-2 pl-5 text-sm leading-6 text-muted-foreground">
        <li>Open the sign-in page, enter your email, and follow the sign-in link sent to you.
          Your first sign-in creates the account automatically. Then open <strong>API applications</strong>.</li>
        <li>Choose <strong>Add a new application</strong>. Name it <strong>SOURCE personal</strong>, select
          the <strong>Production</strong> environment, and keep the default option to generate a private key in your browser.</li>
        <li>In the allowed redirect URLs, enter exactly:
          <code className="mt-1 block break-all rounded-md bg-muted p-2 select-all">https://sourceovcourse.com/banking/callback</code>
        </li>
        <li>Use <code>https://sourceovcourse.com/privacy/banking</code> as the privacy URL and
          <code> https://sourceovcourse.com/privacy/terms</code> as the terms URL.
          The HTTPS return page passes authorization back to this Mac; keep SOURCE open.</li>
        <li>Click <strong>Register</strong> and keep the downloaded <code>.pem</code> file safe.
          Your application ID is also the filename without <code>.pem</code>.</li>
        <li>On your application's page, choose <strong>Activate by linking accounts</strong>.
          Select <strong>Poland → Bank Millennium</strong>, sign in on the bank's website,
          and select every account you want SOURCE to read. This is the account whitelisting step.</li>
        <li>Return here, enter the application ID, and select the downloaded <code>.pem</code> file.
          Then click <strong>Connect Millennium</strong> above to authorize SOURCE's own connection.</li>
      </ol>
      <p className="max-w-[60ch] text-sm leading-6 text-muted-foreground">
        The integration key is copied to macOS Keychain. Your original PEM file is not deleted.
        Never select a bank statement, password export, or unrelated private key.
      </p>
      <Button type="button" variant="link" className="px-0" onClick={() => openLink(ACTIVATION_GUIDE)} disabled={busy}>
        Read the account activation guide
      </Button>
      {linkError && <p role="alert" className="max-w-[60ch] break-words text-sm text-destructive">{linkError}</p>}
      <form className="max-w-md space-y-3" onSubmit={(event) => {
        event.preventDefault();
        void onConfigure(applicationId.trim());
      }}>
        <label htmlFor="banking-application-id" className="block text-sm font-medium">Enable Banking application ID</label>
        <Input id="banking-application-id" value={applicationId} onChange={(event) => setApplicationId(event.target.value)}
          placeholder="xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx" autoComplete="off" spellCheck={false} disabled={busy} required />
        <Button type="submit" disabled={busy || !/^[0-9a-f-]{36}$/i.test(applicationId.trim())}>
          Select private key and save
        </Button>
      </form>
    </SettingsPanel>
  );
}
