# Phase G — iPhone remote approval (design, revision 1)

Spec v0.5 §6.5, §6.6, §7.1, §7.2; tests DA-01…DA-11. Owner decision
2026-10-16: no review during development — this design and its code are
reviewed in the final review. Synthetic data only.

## 1. What it does

When the Mac cannot ask for Touch ID itself (lid closed, §6.B), a request
that needs the user's presence — fill a password or card, reveal, unlock,
update a password — can be approved on the paired iPhone. The phone shows
exactly what it approves (site, action, which item), the user confirms
with Face ID, and the phone returns the §6.5 signed approval. The Mac
releases the request only after the §6.6 checks. A push notification only
wakes the phone ("Approval request — open Source to review"); the request
itself is always fetched over the authenticated channel.

## 2. Pieces

| Piece | Where | Notes |
|---|---|---|
| §6.5 approval TLV, its prehash, sign/verify | `vault-proto/src/approval.rs` | tags 0x01–0x0A, 0x10; `SHA-256("ov0/approval/sign/v1" ‖ tlv-without-0x10)`; a challenge is the same TLV without the signature |
| Pending requests on the Mac (helper memory only) | `vault-engine/src/approval/` | `approval_request {origin, action, credential_ref?}` → `{request_id, exp}`; `approval_status {request_id}` → `pending` / `approved` / `cancelled` / `expired`; 120 s; nonce single use |
| §6.6 verification | same | signature (iPhone active in the committed registry) → Mac id → request pending → origin/action/ref equal → nonce unused → not expired → mark consumed. Duplicate delivery of the same approval is idempotent |
| The channel (§7.1) | peer protocol, two new operations (wire annex amendment) | `peer_approvals` (7): no body → the pending challenges for this phone; `peer_approve` (8): body = a signed approval or a cancel `{request_id, cancelled}` → a one-byte result. Both are signed `PeerRequest`s answered by signed `PeerResponse`s, so the phone verifies the Mac's challenges and the Mac verifies the phone's request before the §6.6 checks |
| The phone | engine ops `approvals_fetch_begin` / `approvals_fetch_step {response}` → challenges; `approval_decide {request_id, approve}` → presence (Face ID) for an approval, then the signed TLV in a `peer_approve` request; `approval_submit_step {response}` → result | the phone signs only what it showed: the challenge it verified |
| Push (§7.2) | provider routes `/v2/push/register` (op 32: the phone's APNs token) and `/v2/push/send` (op 33: a Mac asks to wake a device of its vault); an `ApnsSender` trait | the payload is fixed (generic title and body, `rid` = request id), `apns-expiration` 120 s, alert push only. The real APNs connection needs the owner's paid Apple Developer account and its `.p8` key: a stand-in sender is used until Phase J |

## 3. Result codes (`peer_approve` body)

0 released · 1 `APPROVAL_REPLAY` · 2 `APPROVAL_EXPIRED` · 3
`APPROVAL_MISMATCH` · 4 `DEVICE_NOT_AUTHORIZED` · 5 cancelled · 6 unknown
request · 7 `SIGNATURE_INVALID` (DA-08).

## 4. Not in this phase

The Chrome side that raises requests (Phase H) — until then requests are
raised by the helper op directly (tests, and later nm-host). The §6.C
fallback UI is Phase H's extension copy. The real APNs key (Phase J).

## 5. Tests

DA-01…DA-11 against the helper through the peer channel, with a phone
engine through the C ABI where it matters (DA-01, DA-08); the push routes
against the provider core with a recording sender.
