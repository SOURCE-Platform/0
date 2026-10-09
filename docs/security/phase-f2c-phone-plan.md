# Phase F.2c — peer sync, iPhone side: plan

Date: 2026-10-08. Spec: v0.5 §22.5 (tiers), §22.7 (admission), §22.8
(protocol, requester order, triggers, transport), §22.9 (signed peer
status); wire annex revision 3 (A.1–A.4). The Mac side is done and
reviewed (phase-f2-verification §4–§5). Internal working plan; the code it
produces goes through the usual milestone review.

## Direction and scope

The iPhone requests and the Mac serves (§22.8 "Direction"). This milestone
gives the phone the requester:

- **in the engine** (Rust, shared, so the Mac's serving code and the
  phone's requesting code are tested against each other in-process): build
  and sign requests, verify responses in the §22.8 requester order, and
  drive the exchange;
- **in SOURCE Vault** (Swift): only the transport — pinned HTTPS to
  `POST /v1/vault/peer` with the bearer token — and the triggers.

Not in this milestone:

- the phone's own provider client, so no committed-tier movement from a
  provider and no "fresh within 15 minutes" state;
- phone authoring and publication (F.2d);
- "Remove this vault" (F.2d).

Consequences, stated:

- Without a verified provider `state_get` the phone may **push only
  provider-confirmed revisions** (§22.7 freshness). It authors nothing yet
  either, so `peer_revs_put` is wired and tested but has nothing to carry
  in normal use.
- `peer_state` answers are verified exactly as a provider state and kept
  **provisional** (§22.5). They deliver revisions at the current
  `vk_generation` only and never move the committed tier.

## Engine (vault-engine `peer/client/*`, FFI ops)

1. **Requester core.**
   - The phone builds `PeerRequest`: `vault_id`, self as sender, the Mac's
     `device_id` (the enrollment's `mac_device_id`) as receiver, the
     operation, `body_sha256`, `t` = now, and `n` from the OS RNG. It
     signs the prehash with its own SE signing key; the engine builds the
     digest, so no caller digest is ever signed.
   - **One outstanding request** at a time: its prehash, the addressed
     responder and the operation.
   - **Response order** (§22.8 requester):
     1. canonical parse;
     2. `request_prehash` = the outstanding one;
     3. `responder_device_id` = the addressed Mac, active in the phone's
        **committed** registry — `peer_status` included, since the signer
        of a status the phone believes must be active (§22.9);
     4. the signature;
     5. `SHA-256(body)` = `body_sha256`;
     6. the body decodes as its operation.
   - Any failure is "unable to verify": nothing is applied, and the
     exchange ends.
2. **Exchange driver**, a state machine behind two FFI ops:
   - `peer_sync_begin {now}` returns `{request}`, the HTTP carriage entry,
     base64url.
   - `peer_sync_step {response | refused: http}` returns the next
     `{request}`, or `{done, summary}`.

   Allowed while UNLOCKED, because admission opens revisions under the VK.

   Steps:
   1. `peer_status`. Verify the body (`vault_id`, the registry chain
      verified as §4.8 against the phone's committed head). If the chain
      revokes this phone, published or pending, apply the **revocation
      lock**: lock, record it, no further peer exchange (§22.9). A new
      `recovery_epoch` is believed only if its proof verifies under the
      phone's VK for its last accepted manifest; otherwise "unable to
      verify".
   2. `peer_hello`. Compare the heads digests bucket by bucket. If the
      Mac's committed generation is greater than the phone's, run
      `peer_state` (state mode). The Mac's provider-committed state is
      verified as a provider state and held provisionally.
   3. `peer_heads` for the differing buckets (≤ 256 per request; re-ask
      until `complete`).
   4. `peer_revs_get` for records whose heads differ: the phone's servable
      heads are declared, at most 512 records per request, re-asked until
      `complete`. Admission uses the Mac-side rules factored out of
      `admit::put` (open before admit, closure, waiting, provenance =
      the Mac's `device_id`). Unavailable items are left to the provider
      path.
   5. `peer_revs_put` with only the provider-confirmed revisions the Mac
      lacks (none in practice until F.2d).

   Caps per the annex. The summary returns counts only — never record
   content.
3. **FFI**:
   - add `peer_sync_begin` / `peer_sync_step` to `IOS_OPS` and to the
     catalogue (`phase-f2b-ffi.md`);
   - there is no new entry point; FFI-01 is unchanged;
   - the stored `peer_endpoint` is read by the engine from the Keychain
     and returned to Swift as `{host_hints, port, spki_sha256}`. The token
     travels to Swift once per exchange; it authorizes nothing in the
     vault (§22.8).

## SOURCE Vault (Swift)

- **`PeerClient`**:
  - HTTPS to each host hint in turn, then mDNS `_source-vault._tcp`.
    Hints are untrusted; the pin decides.
  - **Pin:** SHA-256 of the leaf's SubjectPublicKeyInfo DER. That is a
    P-256 key, matching `peer_tokens::spki_of_key_pem`, so the fixed
    P-256 SPKI prefix ‖ the 65-byte point. Any other key type is refused.
  - The `Authorization: Bearer` header only; no redirects; the body
    `application/octet-stream`.
  - Unsigned refusals (`401`, `403`, `413`, `429`, `503`) are passed to
    the engine as `refused`.
- **Triggers:** unlock, app foreground while unlocked, and "Sync now".
  Never in the background. One exchange at a time; cancelled on lock.
- **UI:**
  - "Last synced with your Mac" and a "Sync now" button;
  - the revocation lock screen: "This iPhone was removed from your vault"
    (wording per §22.9 — "removal pending" until corroborated), with no
    delete offer until F.2d;
  - plain messages for "unable to verify" and "Mac not reachable".

## Tests

- **Engine, in-process** (helper tests, like `phone_join.rs`): a joined
  phone engine syncs against the real Mac `peer_serve`.
  - The Mac adds and edits items; the phone syncs and sees them (PS-01
    from the requester side).
  - PA-05: a response signed by another device, mis-addressed, with a
    wrong `request_prehash` or a swapped body is refused.
  - A Mac `peer_status` revoking the phone locks it.
  - `complete = 0` paging converges.
  - An unverifiable state stays provisional and moves nothing committed.
  - Peer-only revisions carry the Mac as source.
- **FFI catalogue** test for the two ops.
- **Swift:** the SPKI pin (fixed P-256 prefix) against a test key.
- **Device run:** sync after the Mac adds an item; the revocation lock.

## Risks

- The requester reuses the Mac's admission code. It is factored, not
  copied, so both sides share one implementation of §22.7.
- The provisional `peer_state` verification must use the full provider
  order. It reuses `sync::remote::parse` and the §22.10 checks, never a
  lighter path.

## Progress

- **Engine done (2026-10-09, `b1df19b`).** `peer::client`
  (`envelope`, `status`, `removal`, the exchange), ops `peer_sync_begin`
  and `peer_sync_step`, a shared `admit_rows`, and the revocation lock.
  Tests: `tests/phone_sync.rs` (4), against the Mac's real `peer_serve`.
- **App done (2026-10-09).** `PeerClient` (SPKI pin), the `PeerSync`
  loop, sync after unlock and "Sync now", the sync bar, and the removal
  screen. Tests: `PinTests` (2).
- **Choices made in this milestone, for review:**
  1. **`peer_state` and `peer_revs_put` are not requested yet.** A
     provisional state moves nothing the phone can act on before its own
     provider path exists. Without a verified provider exchange, the
     phone may push only provider-confirmed revisions, and those came
     from the Mac. Both move to F.2d.
  2. **The removal lock is a marker file in the vault directory.**
     Unlocking is still allowed so the vault can be read; authoring and
     peer exchanges are refused; nothing is deleted (§22.9 "no automatic
     deletion"). Lifting the lock needs the provider (F.2d).
  3. **The Mac addressed is the authorizer of the phone's own enroll
     entry**, which must still be active in the committed registry.
  4. **Real-world consequence of §22.7 freshness:** the Mac serves its
     local-only edits only within 15 minutes of a verified provider
     exchange. Until a backup service is configured, the phone receives
     only what reached it at pairing, plus provider-confirmed revisions.
