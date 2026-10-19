# Bank-originated updates for SOURCE

Research date: 19 October 2026. No private messages, phone notifications,
bank login secrets or private-key contents were accessed for this research.
No bank settings, subscriptions, notification permissions or providers changed.

## Required outcome

A bank-side event for any account activity must propagate automatically into
SOURCE's Banking table. The request includes physical restaurant/store card
payments, online/app/delivery payments, incoming transfers and cash withdrawals.
Periodic polling and fetching when a window opens are not equivalent to this
requirement. Fees, refunds, reversals and adjustments also matter if claiming
complete account coverage.

## Finding

No supported public account-change event feed for this Millennium personal
account has been verified. Enable Banking's documented webhook is a payment
status notification for payments created through its payment API, so it does
not cover arbitrary account activity. The account-information integration
already working in SOURCE remains useful for reconciliation and bank records.

Millennium does have bank-originated transaction notifications for its own
mobile app, and an SMS alert service. Relaying those is a plausible event source
for SOURCE, subject to the phone platform, actual notification contents,
coverage and delivery tests. It is not yet a verified universal solution.

## Public API and provider comparison

| Route | What the documentation establishes | Does it prove the requested behavior? |
| --- | --- | --- |
| Enable Banking | Webhooks track payments initiated through its payment API. Account information is retrieved through read endpoints. | No account-wide event subscription documented. |
| Plaid Transactions | Transaction webhooks exist, but Plaid normally checks the institution one to four times a day. | No. A webhook downstream of polling is not a bank-originated immediate event. Millennium-specific event delivery is unverified. |
| TrueLayer Data API | A webhook can announce completion of an asynchronous data request initiated by the client. | No. Request completion is not an unsolicited account-change event. Millennium coverage was not established. |
| Salt Edge | Callbacks accompany fetching workflows; automatic refreshes use scheduled time slots and limits. | No bank-originated immediate feed established. Millennium coverage was not established. |
| Tink Events | Documents account transaction modification events, including unbooked records, and refresh-completed events. | Candidate only. Documentation inspected does not establish immediate bank-originated events for Millennium. Do not recommend migration on event names alone. |
| Millennium direct API | Public portal describes PolishAPI account-information services; detailed specifications redirect to developer sign-in. | No public subscription for every account change verified. Absence from public docs does not establish that no private agreement exists. |

Sources: [Enable Banking webhooks][enable], [Plaid transaction webhooks][plaid],
[TrueLayer asynchronous Data API][truelayer], [Salt Edge API][salt],
[Tink events][tink], [Millennium developer portal][millennium-api].

## Millennium's existing notification channels

The bank's current transaction-notification page lists free mobile push alerts
for incoming/outgoing transfers, card transactions, failed operations and
reminders, with the owner choosing categories in the bank's app. It does not
establish a public third-party endpoint or delivery of every ledger adjustment.
Source: [Millennium transaction notifications][millennium-push].

The bank's MilleSMS description lists incoming/outgoing transfers, debit-card
authorizations and withdrawals, and credit-card authorizations, settlement and
withdrawals. Notification types and minimum amounts are configurable. Retail
Millenet's functionality page also lists MilleSMS settings. Account-specific
eligibility, present-day charges, delivery windows, BLIK coverage and exact
message contents must be checked before relying on it. Do not enable a paid
service without the owner's approval. Sources: [MilleSMS][millesms],
[retail Millenet capabilities][retail].

The bank's current linked card terms explicitly distinguish online-authorized
card transactions from offline card transactions, for which those SMS alerts
are not sent. An older 2005 press release also describes different delivery
hours for card and other alerts; it is not evidence of today's schedule.
The live account's current terms/settings must determine the actual coverage.
Source: [bank-hosted card terms][card-terms].

## Device-dependent relay candidates

### Android

Android provides a NotificationListenerService callback when apps post
notifications, including their source package. With explicit owner permission,
a SOURCE companion could process only Millennium financial activity alerts and
forward them to paired SOURCE devices. Device battery restrictions, background
delivery, hidden/redacted content and duplicates need real testing. This
requires building an Android component; the current repo does not contain one
for this purpose. Source: [Android notification listener][android].

### iPhone

A normal notification service extension handles notifications for its own app;
it does not grant SOURCE access to Millennium's notifications.
Source: [Apple notification service extensions][apple-nse].

Apple now documents EU accessory notification forwarding with user-selectable
source apps. It requires companion/accessory integration, relevant entitlements
and compatible devices/software. Its documented transport is accessory-oriented;
it is not evidence that a general iPhone app may forward arbitrary bank alerts
over the Internet to SOURCE while the owner is away from the Mac. Do not promise
this route without verifying eligibility, distribution rules and transport.
Source: [Apple accessory notifications][apple-accessory].

An alternative candidate is MilleSMS plus an owner-created Shortcuts Message
automation. Apple documents sender/content message triggers and automatic
execution. This establishes a possible trigger, not the complete secure relay:
locked-device execution, useful message input, retry/outbox behavior, fees and
all required event categories still need testing. Mac Shortcuts also gained
Message automations; that alone does not verify reliable forwarding of bank
SMS from an iPhone. Sources: [message triggers][apple-message],
[automatic execution][apple-auto], [Shortcuts 26 changes][apple-shortcuts].

## Candidate SOURCE architecture

This is an engineering proposal, not an implemented or proven integration:

1. The bank emits an activity notification through a supported channel.
2. An owner-authorized relay selects financial activity events, excluding login
   approvals, one-time codes, passwords and unrelated notifications.
3. The paired device sends a signed/authenticated, encrypted event to SOURCE.
   A device outbox queues during disconnection. Remote delivery while the Mac
   is elsewhere requires an explicit supported network path; local Bluetooth
   alone cannot provide it. Cloud queueing needs a separate approved design.
4. SOURCE immediately shows an alert-derived provisional activity row when
   sufficient structured details exist. Missing amount/currency must not be
   invented. It must be clearly distinguishable from a bank-confirmed record.
5. The existing account-information connection later reconciles bank records,
   pending/booked transitions, corrections and full account totals within
   provider limits. Do not treat each notification as permission to bypass
   unattended fetching limits, and do not invent a balance from partial alerts.
6. An internal change event refreshes the protected Banking view. No financial
   contents should be broadcast on general application event channels.

An alert receipt alone cannot certify a complete ledger. Missing notifications,
offline transactions, refunds, fee/interest entries and reversals require
explicit coverage tests and reconciliation. A phone or Mac being offline also
precludes immediate display until delivery resumes.

## Next evidence needed

- Which phone and OS receive the owner's bank activity alerts. A question has
  been presented to the owner; no phone access or notification grant requested.
- Verify enabled bank categories and whether payment alerts include enough
  fields to display a provisional row. The owner handles bank settings and
  login; do not inspect unrelated messages or authentication codes.
- Verify at least card payment, incoming transfer, withdrawal, BLIK if used,
  reversal/refund, and fee/adjustment coverage before claiming every activity.
- Test a supported relay locally with synthetic messages, then observe normal
  future activity with the owner. Do not ask the owner to spend money for a test.
- If the owner requires full bank-confirmed records at every event rather than
  provisional notification rows, seek a documented Millennium/aggregator
  subscription with explicit coverage and latency guarantees before building.

## Provider questions prepared for owner-approved contact

No messages have been sent. Questions for Enable Banking/Millennium:

- Is a personal Millennium Poland account-change subscription available that
  originates at the bank, rather than periodic aggregation?
- Does it cover debit-card authorizations and settlement, online purchases,
  incoming/outgoing transfers, BLIK, ATM withdrawals, refunds, reversals,
  charges and interest? Which are pending versus booked?
- Can a third-party personal desktop integration subscribe? What agreements,
  costs, delivery guarantees, replay/catch-up and signature validation apply?
- If unavailable, can the existing transactional notifications be delivered
  through an owner-authorized machine-readable channel?

[enable]: https://enablebanking.com/docs/api/webhooks/
[plaid]: https://plaid.com/docs/transactions/webhooks/
[truelayer]: https://docs.truelayer.com/docs/asynchronous-calls-and-webhooks
[salt]: https://docs.saltedge.com/v6/api_reference/
[tink]: https://docs.tink.com/resources/api-setup/webhooks
[millennium-api]: https://openapi.bankmillennium.pl/en/home/
[millennium-push]: https://www.bankmillennium.pl/prestige/bankowosc-elektroniczna/bankowosc-mobilna/aplikacja-mobilna-klienci-indywidualni-biznes/powiadomienia-transakcyjne
[millesms]: https://www.bankmillennium.pl/bankowosc-prywatna/bankowosc-elektroniczna/bankowosc-mobilna/millesms
[retail]: https://www.bankmillennium.pl/bankowosc-elektroniczna/millenet-dla-klientow-indywidualnych-biznes/funkcjonalnosc
[card-terms]: https://www.bankmillennium.pl/delegate/managedfiles/63/latest?portalId=202
[android]: https://developer.android.com/reference/android/service/notification/NotificationListenerService
[apple-nse]: https://developer.apple.com/documentation/usernotifications/unnotificationserviceextension
[apple-accessory]: https://developer.apple.com/documentation/accessorynotifications
[apple-message]: https://support.apple.com/en-il/guide/shortcuts/apdd711f9dff/ios
[apple-auto]: https://support.apple.com/en-mide/guide/shortcuts/apd602971e63/ios
[apple-shortcuts]: https://support.apple.com/en-tj/125148
