# Personal banking in SOURCE

## Current scope

The first provider is **Enable Banking**, for the owner's Bank Millennium
personal accounts in Poland. This is a **personal-use preview**, not a public
banking product and not a released financial-advice system.

Verified on 2026-10-18:

- [Enable Banking's Millennium connector](https://enablebanking.com/open-banking-apis/PL00116)
  lists production account information and personal-account usage.
- [Restricted production applications](https://enablebanking.com/docs/api/linked-accounts/)
  can access whitelisted own accounts for individual non-commercial use.
- [Plaid's official coverage CSV](https://plaid.com/documents/eu_institution_coverage.csv),
  generated 2026-10-01, also lists Millennium Poland with personal data access.

Coverage is not proof that a particular account type, bank authorization, or
history request works. No real bank account was connected during development.

## Setup on macOS

1. [Register or sign in to Enable Banking](https://enablebanking.com/sign-in/).
   Enter your email and follow the sign-in link sent to you. Your first
   sign-in creates an account automatically. Then
   open [API applications](https://enablebanking.com/cp/applications).
   Choose **Add a new application**, name it **SOURCE personal**, and select
   **Production** (not Sandbox). Keep the default browser-generated private
   key option. The provider's general quick-start uses Sandbox; this
   personal, real-account connection needs Production.
2. Register exactly `https://sourceovcourse.com/banking/callback` as the
   application's redirect URL. Production registration rejected the former HTTP
   loopback URL with an unsupported-scheme error. Use
   `https://sourceovcourse.com/privacy/banking` for the privacy URL and
   `https://sourceovcourse.com/privacy/terms` for the terms URL. The current data
   protection contact is `adam@sourceovcourse.com`.
3. Click **Register** and keep the downloaded `.pem` file safe. Its filename
   without `.pem` is the application's ID. Keep the private key out of Git,
   chats, logs, and screenshots.
4. On the application's page, choose **Activate by linking accounts**,
   select **Poland → Bank Millennium**, and authorize **every** account you
   want to import. This is whitelisting: it activates restricted access but
   does not authorize SOURCE's own session.
5. Open **Settings → Banking** in SOURCE (the top-level **Banking** tab also
   works). Enter the application ID, then choose the downloaded PEM file
   in the native picker.
6. Click **Connect Millennium** and authorize in the system browser. Never
   enter a bank login or password into SOURCE. Allow the initial import to
   finish before navigating away.
7. Compare imported records with a statement before using them for decisions.
   Only then opt into background sync if desired.

The original PEM file is **not deleted**. Protect it independently. SOURCE
copies it to macOS Keychain, not the unfinished SOURCE Vault.

The registered HTTPS address returns the browser to
`http://127.0.0.1:1431/banking/callback` on the same Mac. Port 1431 is reserved
in `~/.agents/ports.md`. The local callback binds **127.0.0.1 only**, only
during connection. If occupied, connection fails; it does not change ports
or terminate another service. Rebuild and restart SOURCE after changing its
registered callback constant; changing the registration form alone is insufficient.

## HTTPS authorization return

The public site's `src/app/banking/callback/route.ts` delegates to
`src/lib/banking-callback.ts`. It checks the state format, bounded code/error
values, and duplicate/unknown parameters before returning HTTP 303 to the fixed
loopback destination. It never fetches localhost, calls a bank API, stores a
code, or accepts a caller-supplied host or port. Error descriptions are discarded.
No external assets, analytics, or application logging run on this route. Responses
use `no-store` for browser and CDN caches, `no-referrer`, restrictive CSP, and
`noindex`. Native errors use fixed messages and the local page clears the current
URL's query parameters with a CSP-hashed script.
The site's Wrangler configuration explicitly disables Worker invocation logs,
which would otherwise include the incoming request URL when logging is enabled.

The authorization code and random state necessarily pass through the SOURCE
website and Cloudflare. Request metadata may still appear in hosting, security,
browser, or network records; no application-level promise can exclude those logs.
Do not enable request-URL logging or live log tails during real authorization.
Test the development return route with synthetic values only. Account records and
the private key do not pass through the website. Only the Mac validates the exact
one-use state and exchanges the code with Enable Banking using its Keychain key.

## What is implemented

- Discovery of Millennium's personal AIS connector and bank-specific maximum
  consent duration; system-browser authorization with a random, one-use state.
- A ten-minute callback timeout, cancellation, exact callback host/path,
  provider HTTPS URL validation, and sanitized errors.
- Accounts matched across sessions by `identification_hash`, not ephemeral
  provider account UIDs. Account numbers are masked in the UI.
- Longest-available history immediately after authorization, all pages with
  continuation-loop and size limits, then a 90-day overlap on later imports.
- Booked transactions, current unbooked observations, balance types and
  timestamps, raw evidence, and transaction revision history.
- Bank references scoped to accounts. Without stable references, matching is
  explicitly provisional; identical purchases retain their multiplicity.
- Failed imports do not replace committed transactions or balances. The
  authorized session is saved before history import so failures are retryable.
- Exact decimal money strings; no floating-point financial arithmetic and no
  adding balances across currencies or balance types.
- Account/direction/date/text filters and bounded transaction queries.
- Opt-in background attempts approximately every six hours while SOURCE runs.
  The worker checks hourly, never retries in a tight loop, and honors a
  one-minute minimum between manual attempts. Opening SOURCE does not imply
  the Mac stays awake.
- Disconnect deletes every stored provider session before marking accounts
  inactive. Failed deletion remains retryable. Imported history stays local.
  Provider documentation says bank consent is closed automatically **if possible**;
  inspect/revoke remaining consent in the bank or Enable Banking if necessary.

## Data and security boundaries

Code lives in `src-tauri/banking/`, with thin Tauri commands and a separate
`src/components/banking/` view. No banking records enter the general activity
SQLite database, OCR index, mobile API, agent prompts, analytics, or a remote
SOURCE record store. The provider necessarily processes data to deliver the feed;
the authorization return also passes through the public site as described above.

The provider private key and a separate random 256-bit ledger key are stored
in macOS Keychain under service `com.racker.zero.banking.v1`. Windows/Linux
fail closed until they have an implemented secure-store adapter.

All ledger data, session identifiers, raw evidence, descriptions and amounts
are stored in `<SOURCE data directory>/banking/ledger.enc`, separately from
recordings. On macOS this is under `~/.observer_data/banking/`.
The format uses XChaCha20-Poly1305, fresh random nonces and versioned AAD.
Writes use owner-only temporary files, atomic replacement and file/directory
sync. Authentication failures and missing keys never reset existing data.
There is a 128 MiB encrypted-ledger cap; this first personal implementation
rewrites a whole encrypted snapshot, not a high-volume ledger database.

Keychain protects secrets at rest, not against a compromised running SOURCE
process, an unlocked user session, or same-user malware. Losing its ledger
key makes the local history unreadable. No recovery or encrypted backup is
implemented. Do not assume OS backups contain a recoverable matching key.
Authenticated encryption does not detect restoration of an older, otherwise
valid ledger snapshot; rollback-resistant storage is not implemented.

While the Banking surface is open, existing capture exclusion pauses screen
capture/OCR and excludes keystrokes in SOURCE. The native authorization guard
also pauses **all** keyboard capture while the external bank flow is active,
even if the user navigates to another SOURCE tab. The guard unwinds on errors.
This does not block third-party screen recorders or OS/browser history.

No payment initiation, cancellation of subscriptions, categorization,
identity inference, or automatic external AI processing is implemented.
Financial objects and evidence links are later layers, separate from records.

## Validation

```sh
cd src-tauri
CARGO_PROFILE_TEST_DEBUG=0 timeout 180 cargo test -p source-banking --lib
CARGO_PROFILE_TEST_DEBUG=0 timeout 180 cargo test -p source-banking --test capture -- --test-threads=1
cd ..
node scripts/test-banking.mjs
npm run build
npm run check:file-lengths
```

To repeat the synthetic handoff with the public site's dev server already running
on its assigned port 3001 (the test briefly binds reserved callback port 1431):

```sh
cd src-tauri
CARGO_PROFILE_TEST_DEBUG=0 cargo test -p source-banking public_return_round_trip_with_synthetic_data -- --ignored
```

Development results, 2026-10-18:

- 18 banking-core tests and 9 tests of the actual app capture policy passed.
  Tests use synthetic records, in-memory secrets and temporary files, never
  the owner's Keychain or bank account.
- `cargo check -p SOURCE --lib`, the frontend build, banking UI checks,
  file-length audit and `git diff --check` passed. The native check still
  reports existing Objective-C/dead-code warnings.
- A synthetic browser bridge verified exact large PLN amounts, interactive
  and disabled cursors, no horizontal overflow, and React StrictMode's
  delayed capture registration/unmount cleanup (one active guard, then zero).
- Full browser-only navigation is blocked by an existing app startup issue:
  `App.tsx` registers Tauri event listeners when the Tauri bridge is absent.
  The Banking view was tested separately. No signed-app launch, real Keychain
  prompt, or live provider flow was tested.

HTTPS return validation, 2026-10-19:

- 20 banking-core tests, 18 public return-route tests, 9 capture-policy tests,
  and the UI checks passed. A synthetic test traversed the local Next.js route
  and the real Rust listener. A separate Chrome test traversed the deployed HTTPS
  route, delivered the synthetic code once, and verified the query-free local URL.
- Frontend build, native compile check, file-length audit, and signed macOS app
  packaging passed. The rebuilt app was installed and restarted; its Banking
  setup shows the HTTPS registration URL. The previous app bundle was retained
  under `~/Library/Application Support/SOURCE/app-backups/`.
- The public route and updated privacy notice were deployed from public-site
  revision `1e58d29`. No real account authorization or banking-key access was used
  in these checks. Production application registration and Millennium import
  still need the owner's live approval and verification.

Before considering the feed trustworthy:

Authorization host compatibility, 2026-10-19:

- Production registration and account whitelisting succeeded; Enable Banking
  shows the application as Active with access restricted to linked accounts.
  SOURCE saved the integration credentials, but its first connection attempt
  rejected the authorization URL before opening the browser.
- The authorization allowlist now accepts exactly `auth.enablebanking.com` and
  `tilisy.enablebanking.com` over HTTPS on port 443, without URL credentials.
  Enable Banking's [May 2026 migration notice](https://enablebanking.com/blog/2026/06/12/enable-banking-changelog-may-2026)
  describes the transition between these hosts. Arbitrary subdomains and
  lookalike domains remain rejected. No authorization URL or query is logged.
- All 20 banking-core tests and 9 capture-policy tests passed, along with UI
  checks, the file-length audit, frontend build and signed macOS packaging.
  The verified bundle was installed with the same application and team identity;
  existing integration credentials survived the restart.
- The live retry opened `tilisy.enablebanking.com` and displayed SOURCE's
  end-user consent page, confirming the host mismatch. The owner completed
  consent and bank authorization, the HTTPS return reached SOURCE, and the
  first live import completed successfully. Automatic sync remains off.

Remaining live validation:

- Validate ongoing updates, bank-specific request limits and Millennium's PSU
  headers. Registration, the HTTPS-to-loopback return and configured Keychain
  access across a signed-app restart have now worked on the owner's Mac.
  The desktop adapter does not invent an end-user public IP address; banks
  requiring additional headers fail with a clear error.
- Verify account selection and multiple PLN/foreign-currency accounts.
- Compare a complete statement period, including duplicate-looking purchases,
  corrections, transfers, fees, pending/booked transitions and reversals.
- Measure history depth immediately after consent. It is **not guaranteed to
  be all history**. Enable Banking documents a short post-authorization window
  for deep history at some banks. Statement import is not built yet.
- Repeat sync and reauthorize; confirm no extra accounts or transactions.
  Without stable references, edited bank records may appear as new provisional
  records. Old corrections outside the 90-day overlap need a fresh deep import.
- Exercise expiry, refused consent, network failure, pagination failure,
  cancellation and remote revocation failures.
- Verify capture exclusion on a real Mac during the bank browser flow.

Not yet built: a financial object graph, receipt/email matching, deterministic
spending/forecast tools, voice-agent tool registration, statement import,
selective deletion/export, recovery/backup, or a multi-user credential broker.
Do not use this preview to calculate a guaranteed "safe to spend" figure.
