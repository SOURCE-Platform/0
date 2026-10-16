# Phase F.2e — reverse enrollment: the iPhone adds a replacement Mac (design, revision 2)

Owner decision F2-D4 (2026-09-29): the iPhone may authorize enrolling a
**replacement** Mac. Spec v0.5 §22.13 reserves the roles for this design.
Revision 1 (`6415ef7`) was reviewed by the security and spec reviewers
(§12); revision 2 applies every blocker and important finding. By owner
decision 2026-10-16 there are no further reviews during development: this
design and its code are reviewed together in the final review, before the
first real credential. Synthetic data only.

## 1. Scope

- **Replacement only.** The iPhone adds a Mac when no other Mac is an
  active vault device: the lost or stolen Mac has been revoked and that
  revocation has reached the provider (`REMOTE_COMMITTED`). This is
  spec §12 scenario 1's order — revoke, then enroll the replacement
  (architecture recovery table) — and it keeps the §22.7 cutoff whole: the
  revoked Mac's deliveries are refused on the iPhone before any bundle is
  built (reviews SEC-B2 / SPEC-B2). A second simultaneous Mac stays out of
  scope (§22.1 non-goal; architecture C8; reviews SEC-I7 / SPEC-I4).
- **The phone-recovered vault** (F.2d chunk 3) gets a Mac back this way.
- Unchanged: forward §5; a phone never authorizes a phone.

## 2. Roles (the §22.13 questions)

| Question | Decision |
|---|---|
| Who serves TLS and shows the QR | **The new Mac**: its main process runs the existing ephemeral enrollment server (rcgen cert, port 0, the session only); the QR is on the Mac's screen |
| Who assigns `device_id` | **The iPhone** (authorizer), in its hello |
| Who commits first | **The new Mac's helper**, in the QR: `commit = SHA-256("ov0/renroll/commit/v1" ‖ nonce_n ‖ sign_pub ‖ agree_pub ‖ name ‖ platform)`. `nonce_n` and the new keys never leave the helper before its `renroll_hello` reply — `renroll_begin` returns only `{secret, commit, expires_in}` — so the Mac's own main process cannot search for matching codes (SEC-I1 / SPEC-I2) |
| Who shows the SAS | **Both**: the iPhone on its screen; the Mac **only in the helper's own panel** "Join a Vault" (§1.7), never main |
| Who signs, seals, publishes | **The iPhone**, after presence and the **master password** (§22.4): the `enroll` entry, the envelope (`ov0_hpke_seal`, already a phone crossing), and the provider-activation publication |
| What the new Mac anchors on | The SAS-confirmed transcript, which binds `authorizer_key` = SHA-256(the iPhone's signing key). A stated §22.1 exception: the Mac anchors on a key the other party chose, confirmed only by the user's 40-bit comparison — the same strength as forward §5's SAS |

## 3. Sequence

1. **Mac, Vault tab → "Join from your iPhone"** (UNINITIALIZED only;
   one session at a time; at most six begins per minute, as forward).
   Helper `renroll_begin`: fresh SE keys (Touch ID-bound agreement key, or
   discarded on a Mac without Touch ID), secret, `nonce_n`, commit; the
   attempt lives in a **staging directory** (`renroll/`), never the vault
   root. Main starts the server and shows QR `{v:3, host, port, fp,
   secret, commit}` (`host` a private or link-local literal).
2. **iPhone, Security → "Add a Mac" → scan.** `authorize_begin {qr}`
   checks: QR `v:3`; host private or link-local (SPEC-I8); preconditions
   (§6). Hello over TLS pinned to `fp`.
3. **`POST /v1/vault/renroll/hello`** `{v:3, secret, nonce_e,
   authorizer_device_id, authorizer_sign_pub, new_device_id, vault_id}` →
   helper `renroll_hello` (constant-time secret, single use, 5 tries; a
   second hello with the correct secret ends the session) → reply `{nonce_n,
   sign_pub, agree_pub, name, platform}`, which must open the commit; the
   iPhone requires `platform` = macOS and runs the forward key checks.
4. **Both compute transcript and SAS.** Mac: helper panel "Join a Vault"
   shows the code — "Continue only if your iPhone shows the same code".
   iPhone: shows the code; "They match".
5. **iPhone `authorize_confirm`**: presence, master password (kind 1, §15
   backoff); signs the `enroll` entry; seals the envelope (bound to
   `nonce_e`, as forward binds its nonce); builds the bundle; and
   **prepares everything the finish needs** — the entry, the envelope
   file and a fully staged §22.11 publication — so the finish needs no
   key and works while LOCKED (SEC-I5). Syncs that would move the
   registry are held until the session ends.
6. **`POST /v1/vault/renroll/bundle`** (route proof; main checks the proof
   before reading the body; refused before the panel's Continue). Helper
   `renroll_complete` runs §5's checks; on a Mac without Touch ID the
   panel asks for the master password here (§5 step 4). The answer to the
   POST is `{ack, peer_endpoint}`; the per-message limit for this POST is
   the session's remaining time (it may wait on the panel).
7. **iPhone `authorize_finish {ack, peer_endpoint}`**: verifies the ACK,
   appends the prepared entry, stores the envelope, records the pending
   `Enrollment`, keeps `peer_endpoint` bound to `new_device_id`, and
   uploads the staged publication (or at the next foreground, F.2d).
8. **`POST /v1/vault/renroll/done`** (route proof + registry head) →
   helper `renroll_finish`: the staging moves into the vault root; the
   Mac's floor is set (§5 step 6). This is the only trigger; until it, the
   Mac stays without a vault (nothing to unlock), and a session that ends
   first discards the staging (SEC-I4 / SPEC-I1). An entry the iPhone
   committed whose `done` never arrived is the forward VER-O1 case: the
   Mac's keys are wiped, and the entry is revocable from the iPhone.

## 4. Strings and messages

| Item | Value |
|---|---|
| QR | `{v:3, host, port, fp, secret, commit}`; forward readers refuse `v:3`, the reverse reader refuses `v:2` |
| Transcript | `SHA-256("ov0/renroll/transcript/v1" ‖ fp ‖ authorizer_key ‖ secret ‖ nonce_e ‖ nonce_n ‖ authorizer_device_id ‖ new_device_id ‖ sign_pub ‖ agree_pub ‖ vault_id)` |
| SAS | §5.2's construction over that transcript (shares the SAS info string with forward; harmless — the transcripts are domain-separated) |
| Route proof | `HMAC-SHA256(secret, "ov0/renroll/proof/v1" ‖ nonce_e)`, checked by helper op `renroll_proof` |
| ACK | §5.2's construction (`"ov0/enroll/ack/v1"`, head, `authorizer_device_id`), unchanged |
| Errors | preconditions: `BAD_STATE`, `VAULT_BEHIND`, `PEER_NOT_PERMITTED` (removed), `ROTATION_PENDING`; `REPLACEMENT_ONLY` (another Mac active or its revocation not committed); `NOT_FRESH` (no verified provider exchange in 15 min); plus the forward enrollment codes |

## 5. The new Mac's checks (before anything is kept)

1. The panel's Continue (the SAS compared on the Mac).
2. Registry chain, §4.4 rules 1–5, 7, 8 (`CheckpointAnchored`), ending in
   one `enroll` entry that installs exactly this Mac's keys under
   `new_device_id`, signed by the active device `authorizer_device_id`
   whose signing key hashes to `authorizer_key`; `vault_id` as in the hello.
3. The manifest signature under a device active in that registry.
4. The VK: from this Mac's envelope (as the signed index names it, bound
   to `nonce_e`) with its agreement key — or, on a Mac without Touch ID,
   from the bundle's `password.wrap` (hash fixed by the signed index, KDF
   block equal to the header's and on the §2.3 allowlist) with the master
   password typed in the "Join a Vault" panel; a wrong password keeps
   nothing and drives the §15 backoff (SEC-B1 / SPEC-B1).
5. The checkpoint under that VK, bound to the manifest and registry head.
   Never skipped.
6. The floor: from the bundle's signed provider state (F.2b SEC-B2). If
   the Keychain floor already names this `vault_id`, it is kept and never
   lowered, its `lost` / `unpublished` lists intact (SEC-I3).

Then the VK commitment is signed (§22.4: "a verified first
materialization" joins the keys a device trusts), the ACK signed — all in
the staging directory until `done`.

## 6. The iPhone's preconditions (before any prompt)

UNLOCKED; not behind (§22.14); no removal lock (§22.9); not COMPROMISED;
no rotation waiting to publish (`ROTATION_PENDING`, owner decision
2026-10-12); **no other active Mac, and no Mac revocation still waiting
to publish** (`REPLACEMENT_ONLY`); a verified provider exchange in the
last 15 minutes (`NOT_FRESH`, §22.7).

## 7. Peer sync afterwards

The iPhone stores the Mac's `peer_endpoint` together with
`new_device_id` (Keychain, A.4) and addresses that device; the peer client
uses the stored id when present, else the authorizer of its own entry
(today's rule). The token is bound to the iPhone's id
(`authorizer_device_id`) on the Mac. Amends §22.8 ("the pin also arrives
in the reverse ACK over the QR-pinned channel") and annex A.4 (SEC-I6 /
SPEC-I3).

## 8. Threats and answers

| Threat | Answer |
|---|---|
| Someone else scans the Mac's QR first | single-use secret; a second correct hello ends the session; the Mac panel shows their code; the user cancels on the Mac |
| A QR relayed from far away (screen share, web page) | the iPhone accepts `v:3` only with a private or link-local host; the copy says "the Mac must be next to you"; residual: a relay on the same network (stated) |
| A rogue iPhone (another vault) | the Mac panel's comparison; the vault's item count shown after the checks is information, not a control |
| The Mac's same-user main process | commit before reveal with `nonce_n` kept in the helper; the SAS covers both sides' keys; §5.2's stated panel limit applies unchanged |
| A stolen iPhone adds the thief's Mac | presence **and** the MP; replacement-only; the entry is revocable |
| A planted attempt marker next to an existing vault | `renroll_*` only in UNINITIALIZED; the attempt is a staging directory, swept only when no header exists; SE keys removed only by the tags the attempt recorded (SEC-I2) |
| A stale state given to the new Mac | iPhone freshness; signed floor; the Mac's first provider sync verifies |
| Laundering a revoked Mac's deliveries | replacement-only: the revocation has committed first, so its deliveries are already refused on the iPhone |

## 9. Spec amendments (with the code; reviewed in the final review)

§22.13 (this design); §22.1 (the anchoring exception); §22.4 (a verified
first materialization is a trusted key; the iPhone authorizes an
enrollment with presence and the MP); §22.8 and annex A.4 (the pin in the
reverse ACK); §22.10 (scope row; the reverse consumer); §22.14 (floor at
join, kept when the Keychain names the vault); §22.15 (`REPLACEMENT_ONLY`,
`NOT_FRESH`); §22.16 (the RE rows); §1.5 and §13.2 (`renroll_*`,
UNINITIALIZED only); §1.7 (the "Join a Vault" panel); §2.9 (the new
strings); §5 (a pointer to §22.13); §12 scenario 1 (the phone path); FFI
catalogue §4 (`authorize_*`; the Mac helper refuses them, the phone
refuses `renroll_*`).

## 10. Tests (RE rows; EN is taken by Phase E)

| ID | Test |
|---|---|
| RE-01 | end to end: an iPhone engine (C ABI) adds a fresh Mac; the Mac opens, holds the items; the provider activates it after the iPhone publishes; the iPhone peer-syncs with it |
| RE-02 | a reply that does not open the commit, or a swapped key, gives two different codes; `renroll_begin` returns no nonce |
| RE-03 | nothing kept without the panel's Continue; a cancel either side leaves no entry and no files; no `done` → staging discarded |
| RE-04 | bundle negatives: another signer, other keys or id, another vault, a bad manifest signature, a checkpoint not under the VK |
| RE-05 | secret: 5 wrong tries; expiry; reuse; a second correct hello |
| RE-06 | preconditions: another active Mac, a pending Mac revocation, behind, removed, pending rotation, not fresh, COMPROMISED → refused before any prompt |
| RE-07 | wrong MP on the iPhone → `WRONG_CREDENTIAL` with backoff; presence declined → nothing signed |
| RE-08 | a planted attempt marker or staging next to an existing vault removes nothing |
| RE-09 | a Mac without Touch ID joins with the MP in the panel, its commitment signed at join, and opens with the MP |
| RE-10 | a phone-recovered vault adds a Mac |
| RE-11 | v:2 / v:3 cross-refusal; a public host refused; the route proof required |
| RE-12 | the finish works with the iPhone vault LOCKED |

Cross-language vectors for the transcript, commit, proof and SAS (XV,
CryptoKit-only Swift target) are carried to the final review.

## 11. Owner defaults (changeable)

- Replacement only (no second simultaneous Mac).
- The joined Mac is a full device (can authorize, rotate, revoke).

## 12. Review of revision 1 (security + spec, 2026-10-16) — dispositions

| Finding | Disposition in revision 2 |
|---|---|
| SEC-B1 / SPEC-B1 no VK commitment on a Mac without Touch ID | §5 step 4: the MP in the join panel; checkpoint never skipped; commitment signed at join |
| SEC-B2 / SPEC-B2 the bundle launders a lost Mac's deliveries | §1 replacement only, after the revocation committed |
| SEC-I1 / SPEC-I2 `nonce_n` exposure, rate limit | §2, §3 step 1 |
| SEC-I2 / SPEC-I7 marker wipe, states, existing vault | §3 step 1 staging directory; UNINITIALIZED only; §8 |
| SEC-I3 floor at join | §5 step 6 |
| SEC-I4 / SPEC-I1 no `done` signal | §3 step 8 |
| SEC-I5 finish needs the VK | §3 step 5: prepared at confirm, works LOCKED |
| SEC-I6 / SPEC-I3 peer sync afterwards | §7 |
| SEC-I7 / SPEC-I4 second Mac out of scope | §1 |
| SPEC-I5 EN collides | RE prefix |
| SPEC-I6 coverage | §10 rows; XV carried |
| SPEC-I8 relayed QR | private/link-local host only; §8 |
| SPEC-I9 vault naming not a control | §8: information only |
| SEC-I8 / SPEC-I10 amendment list | §9 |
| SEC-O1…O3, SPEC-O1…O6 | applied in §2–§4 (commit covers name/platform; platform and key checks; proof before body; renamed strings; "presence" wording; named codes) |
