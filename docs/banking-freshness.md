# Automatic banking updates

## Required behavior

When the owner makes a purchase or another account change occurs, SOURCE
should import it automatically at the earliest supported opportunity and
update the Banking tab without a manual refresh. Pending purchases should
appear before booking when Millennium exposes them. Pending records must
reconcile with booked records without counting the purchase twice.

The owner clarified that a bank-side event must initiate the update. A timer
or a fetch on opening Banking does not satisfy this requirement. This covers
restaurant, store, online and delivery payments, incoming money and cash
withdrawals, regardless of where they were initiated. The investigation is
recorded in [banking-push-research.md](banking-push-research.md).

An update cannot precede the bank knowing about the activity. Do not promise
universal coverage or a latency in minutes until the event source is verified.
The polling work below is a fallback/reconciliation layer, not completion of
the push requirement; do not implement it as a substitute for that requirement.

## Verified findings, 2026-10-19

- The live Enable Banking connection and initial Millennium import succeeded.
  This confirms connectivity, not the timeliness of future purchases.
- Enable Banking's documented webhooks cover payments initiated through its
  payment API. No general account-change subscription is documented for the
  account-information connection used here.
- Enable Banking distinguishes unattended fetching from fetching while an
  end user is present. Many banks limit unattended fetching to four times a
  day. This is not yet a verified Millennium-specific quota.
- User-present requests require accurate PSU context. Do not label a hidden
  window, an unattended timer, or an idle user as active to evade quotas.
  The current client does not supply PSU headers.
- SOURCE already hides its main window to the menu bar when closed. Its
  process and background scheduler remain alive. Explicitly quitting SOURCE
  or shutting down the Mac stops local fetching; sleep delays it.
- The existing background timer wakes hourly and checks a six-hour minimum
  between attempts. Automatic sync is currently opt-in.
- The Banking hook fetches the overview on mount and after its own commands.
  It does not subscribe to background import completion, so the visible
  account and transaction data can remain stale after a successful import.
- The ledger already models pending transactions separately and contains
  reconciliation tests. Millennium's live pending-purchase availability and
  exposure delay have not been verified.
- Millennium's detailed service definitions, consent documentation, API
  specification and FAQ currently redirect to its developer sign-in page.

## Implementation sequence

1. Establish the provider constraints. Inspect Millennium's supported
   capabilities through the existing integration and authenticated provider
   documentation where available. Confirm unattended quota, requirements for
   user-present requests, pending-card coverage, and retry behavior. Never
   inspect or print private keys, authorization tokens, or transaction bodies
   for diagnostics. Do not create a developer account or contact support
   without owner authorization.
2. Add a scheduler with separate unattended and user-present modes. It must
   refresh automatically when due on launch/resume, handle sleep and network
   loss, avoid overlapping requests, persist attempts and backoff, and stop
   on expired consent. Active refresh requires genuine user presence and the
   bank's required context. Choose its interval from verified capabilities.
3. Emit an import-completed notification inside SOURCE and refresh the open
   Banking view. Send only an invalidation signal, not financial records, in
   application-wide events. Reload data through the protected main-window
   banking commands. Preserve filters and prevent stale asynchronous results
   from overwriting newer data.
4. Show freshness clearly: last successful fetch, pending status, waiting for
   the next allowed fetch, offline state, provider limits, and expired consent.
   Do not present a successful fetch as proof that the bank has exposed every
   recent purchase. Keep background operation local unless a separate hosting
   arrangement is explicitly authorized.
5. Test scheduling, retries and pending-to-booked reconciliation with synthetic
   records. Then observe an ordinary future purchase with the owner and compare
   its first appearance in the bank with its first appearance in SOURCE.
   Record only the necessary timing and outcome, not purchase details in repo
   documentation. Build, sign, install and verify the changed desktop app.

## Acceptance checks

- New available bank data appears without pressing Sync now.
- The open Banking tab updates after a background import.
- Closing the window leaves permitted background updates running.
- Reopening or waking SOURCE catches up when permitted without bursting requests.
- User-present fetching never runs on an unattended timer with invented context.
- Errors retain the last successful data and respect provider backoff.
- Pending-to-booked changes do not duplicate totals.
- Unavailable push support and actual bank limits remain visible constraints.
- Fully quit or sleeping computers are not described as continuously online.

## Sources

- [Enable Banking webhooks](https://enablebanking.com/docs/api/webhooks/)
- [Enable Banking FAQ: PSU context and fetching limits](https://enablebanking.com/docs/faq/)
- [Enable Banking knowledge base: fetching frequency](https://enablebanking.com/knowledge-base)
- [Enable Banking API: capabilities and pending status](https://enablebanking.com/docs/api/reference/)
- [Millennium developer portal](https://openapi.bankmillennium.pl/en/home/)

Code inspected: `src-tauri/banking/src/service.rs`,
`src-tauri/src/app/commands/banking.rs`, `src-tauri/src/app/run.rs`,
`src-tauri/src/app/tray.rs`, `src/components/banking/useBanking.ts`.
