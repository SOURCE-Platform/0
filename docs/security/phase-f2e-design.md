# Phase F.2e — reverse enrollment: the iPhone adds a replacement Mac (design, revision 1)

Owner decision F2-D4 (2026-09-29): the iPhone may authorize enrolling a
replacement Mac. Spec v0.5 §22.13 reserves it: "its roles (who serves TLS
and shows the QR, who assigns `device_id`, who shows the SAS, who signs,
seals and publishes) are decided in a separate design review. Until that
review closes, §5 is unchanged, the EN tests are not defined, and no
implementation starts. Authorizing it requires the MP (§22.4)." This
document is that design. Synthetic data only, as everywhere.

## 1. Why, and the cases it serves

- The Mac is lost, stolen or replaced, and the iPhone is the remaining
  vault device. Today the only way back to a Mac is total-loss recovery
  on the Mac, which revokes the iPhone and issues a new key.
- A vault recovered onto an iPhone (F.2d chunk 3) has no way to add a Mac
  at all ("stated: no Mac can join a phone-recovered vault until F.2e").
- Normal case: adding a second Mac while the first still exists. The
  forward flow (§5, Mac authorizes) already covers it; this flow is an
  alternative.

Non-goals: a phone authorizing another phone (forward §5 from the Mac
stays the path; out of scope here), and any change to forward §5.

## 2. Roles (the §22.13 questions)

| Question | Decision | Why |
|---|---|---|
| Who serves TLS and shows the QR | **The new Mac** (joiner): its main process runs the existing ephemeral enrollment server (fresh rcgen cert, port 0, bound for the session only); the QR is on the Mac's screen | The iPhone has the camera and already has a pinned TLS client (`PairingClient`); the Mac already has the ephemeral server. No server on iOS (no new listening surface, no extra entitlement) |
| Who assigns `device_id` | **The iPhone** (authorizer), in its hello | §5.2's rule: the authorizer assigns; a new device never chooses its registry identity |
| Who commits first | **The new Mac**, in the QR: `commit = SHA-256("ov0/renroll/commit/v1" ‖ nonce_n ‖ sign_pub ‖ agree_pub)` | The joiner's half (its keys and nonce) is fixed before it sees the iPhone's nonce, so neither a relay nor the Mac's own main process can search for matching codes (the role the forward `commit` plays, §5.2, F.2b SEC-B1) |
| Who shows the SAS | **Both**: the iPhone on its screen; the Mac **only in the helper's own panel** (§1.7), never the main app | Unchanged §5.2 rule on the Mac; an explicit confirmation on **both** devices |
| Who signs | **The iPhone**: the registry `enroll` entry (its SE signing key), after Face ID and the **master password** (§22.4) | An authority change needs the MP on the device making it (F2-D3) |
| Who seals | **The iPhone**: the envelope `HPKE-Seal(new Mac agree_pub, DeviceEnvelopePayload v2{VK})` through the bridge (`ov0_hpke_seal`, already a phone crossing since F.2d step 5) | The authorizer holds the VK |
| Who publishes | **The iPhone**: an ordinary `publish` with the new `enroll` entry (§11.4 provider activation), after the ACK | As forward: the authorizer publishes; `REMOTE_UPDATE_PENDING` until it commits |
| What the new Mac anchors on | The SAS-confirmed transcript, which binds the iPhone's signing key (its SHA-256, `auth_key`); the bundle's registry must end in an `enroll` entry signed by that key installing exactly this Mac's keys under the assigned id | The QR is the Mac's own, so it cannot carry the authorizer key the forward flow anchors on (§22.10, F.2b SEC-B3); the user's comparison of the two codes is the anchor |

## 3. Sequence

```
User            New Mac (main + helper)                    iPhone (SOURCE Vault)
 |  Vault tab → "Join from your iPhone"
 |               helper: renroll_begin
 |                 fresh SE keys (Touch ID-bound agreement key,
 |                 or "password every time" without Touch ID),
 |                 secret (16 B), nonce_n, commit; TTL 300 s
 |               main: ephemeral TLS server; QR v:3 shown
 |  Security → "Add a Mac" → scan ──────────────────────────────▶ authorize_begin {qr}
 |                                                                 checks QR; needs: UNLOCKED, not behind,
 |                                                                 not removed, no pending security change,
 |                                                                 fresh provider check (§22.7)
 |               ◀── POST /v1/vault/renroll/hello (TLS pinned to fp) ──
 |                   {secret, nonce_e, auth_device_id, auth_sign_pub,
 |                    new_device_id, vault_id, proto}
 |               helper: renroll_hello — secret (constant time, single
 |                 use, 5 tries); reply {nonce_n, sign_pub, agree_pub,
 |                 name, platform} (opens the QR commit)
 |               ───────────────────────────────────── reply ──▶ authorize_reply {reply}
 |                                                                 must open the commit; transcript v3; SAS
 |  Mac: helper panel "Join a Vault" shows the SAS                iPhone shows the SAS
 |  compare; Continue on the Mac panel ……… "They match" on the iPhone
 |                                                                 authorize_confirm: Face ID → MP (kind 1)
 |                                                                 → sign enroll entry, seal envelope,
 |                                                                 stage bundle (§5.2 contents) → stream
 |               ◀── POST /v1/vault/renroll/bundle (proof header) ──
 |               helper: renroll_complete — only after the panel's
 |                 Continue; §22.10-order checks (§5 below); vault
 |                 written LOCKED; ACK signed
 |               ─── {ack, peer_endpoint} ──────────────────────▶ authorize_finish {ack, peer_endpoint}
 |                                                                 verifies ACK → commits the entry
 |                                                                 (pending Enrollment) → publishes →
 |                                                                 keeps the new Mac's peer_endpoint
 |               main: teardown server; Mac unlocks with Touch ID
 |               or the master password
```

## 4. Messages

| Field / message | Spec |
|---|---|
| QR | JSON `{v:3, host, port, fp, secret (base32), commit, name, platform}`. `v:3` is reverse enrollment: a forward reader (`v:2`) refuses it and the reverse reader refuses `v:2` |
| TLS | the forward rules (§5.2): rustls ephemeral server on the Mac, the iPhone pins `fp` = SHA-256(cert DER), hostname ignored |
| Hello (iPhone → Mac) | `{v:3, secret, nonce_e, auth_device_id, auth_sign_pub, new_device_id, vault_id}`. `new_device_id` from `OsRng`, checked by the iPhone against its registry (no collision) |
| Reply (Mac → iPhone) | `{nonce_n, sign_pub, agree_pub, name, platform, proto}`; the iPhone refuses it unless `SHA-256("ov0/renroll/commit/v1" ‖ nonce_n ‖ sign_pub ‖ agree_pub)` equals the QR's `commit` |
| Transcript v3 | `SHA-256("ov0/enroll/transcript/v3-reverse" ‖ fp ‖ auth_key ‖ secret ‖ nonce_e ‖ nonce_n ‖ auth_device_id ‖ new_device_id ‖ sign_pub ‖ agree_pub ‖ vault_id)`, `auth_key` = SHA-256(`auth_sign_pub`). Domain-separated from v2 so a forward transcript never equals a reverse one |
| SAS | §5.2's construction over transcript v3 (8 chars, 40 bits) |
| Route proof | bundle and ACK routes carry `X-Ov0-Proof` = `HMAC-SHA256(secret, "ov0/renroll/proof/v1" ‖ nonce_e)`, checked by the Mac helper before main answers |
| Bundle (iPhone → Mac) | exactly §5.2's bundle: a §11.2 v2 state (blobs, signed manifest v2, §4.8 checkpoint), this Mac's envelope (the one the signed index names), the provider origin, and the iPhone's kept signed provider state (the Mac's floor, as F.2b SEC-B2). Streamed (§1.3), ≤ 256 MiB |
| ACK (Mac → iPhone) | the Mac signs `SHA-256("ov0/enroll/ack/v1" ‖ registry_head_hash ‖ auth_device_id)` with its SE signing key — §5.2's ACK, unchanged — plus the Mac's `peer_endpoint` (annex A.4: pin, token, port, hints) for the iPhone's peer sync |
| Timeouts, cancel | §5.2's: 300 s session, 30 s per message, either side cancels; nothing is kept on the Mac before its checks pass; the iPhone writes the registry entry only after a valid ACK |

## 5. The new Mac's checks (before anything is kept)

In this order — the §22.10 consumer order, with the anchor of §2:

1. The panel's Continue was given (the SAS was compared on the Mac).
2. Registry chain from the bundle, §4.4 rules 1–5, 7, 8, ending in one
   `enroll` entry that installs exactly this Mac's `sign_pub` /
   `agree_pub` under `new_device_id`, signed by an active device whose
   signing key hashes to the transcript's `auth_key` and whose id is
   `auth_device_id`.
3. The manifest signature under a device active in that registry.
4. This Mac's envelope, as the signed index names it, opened with its own
   agreement key (Touch ID) — or, on a Mac without Touch ID, skipped: the
   master password opens `password.wrap` at the first unlock ("password
   every time", owner decision 2026-10-02).
5. The checkpoint under that VK (when opened), bound to the manifest and
   the registry head; the `vault_id` equals the hello's.
6. The floor from the bundle's signed provider state, as F.2b SEC-B2.

Then the vault is written LOCKED, its VK commitment signed (§22.4), the
ACK signed. An attempt marker exists from the first key to the ACK's
acceptance; an unfinished attempt is removed whole at failure, cancel or
the next start (as the phone's `join.attempt`).

## 6. The iPhone's side

- **Preconditions** (refused before any prompt): UNLOCKED; not behind
  (§22.14); no removal lock (§22.9); not COMPROMISED; no security change
  waiting to publish (`ROTATION_PENDING` rule, owner decision 2026-10-12);
  a verified provider exchange in the last 15 minutes (§22.7 freshness —
  the new Mac must not be built from a stale state).
- **Authority:** Face ID, then the master password (secure entry kind 1)
  verified against the committed wrap with the §15 backoff (§22.4).
- **Sign and seal** inside the engine; the bundle is built by the same
  code the Mac's `enroll_confirm` uses (shared engine).
- **Commit and publish:** after a valid ACK, the `enroll` entry commits
  locally (pending `Enrollment`, §11.3.2) and Swift runs
  `provider_publish`; the banner of F.2d chunk 2 shows until it lands.
- **peer_endpoint:** the Mac's, from the ACK, replaces the stored one
  (Keychain, A.4) — the iPhone then syncs with the new Mac directly.
- **Capture:** the SAS is not secret; the MP entry is the existing secure
  entry.

## 7. Threats and answers

| Threat | Answer |
|---|---|
| Someone else scans the Mac's QR first (shoulder-surfing, a photo) | single-use secret; the Mac panel shows *their* code, which does not match the user's iPhone; the user cancels on the Mac. They cannot authorize without a vault device, the MP and Face ID |
| A rogue iPhone (another vault) makes the Mac join the wrong vault | the Mac panel's comparison; the panel names the vault being joined (its recovery handle, when the bundle carries it, and the item count) after the bundle checks, before the vault is opened |
| The Mac's same-user main process swaps keys or frames | the SAS covers both sides' keys as each side's own code computes them; the QR commit stops searching; the stated §5.2 limit (a same-user process drawing over the panel) applies unchanged |
| A stolen iPhone adds the thief's Mac | needs Face ID **and** the MP (§22.4); the new `enroll` entry appears in every device's list; the remaining devices can revoke it (RC-01) |
| A network attacker | TLS pinned to the QR's `fp` (physical channel); route proofs |
| A stale or rolled-back state given to the new Mac | the iPhone must be fresh (§22.7); the Mac's floor comes from the signed provider state; the Mac's first provider sync verifies against the provider (§11.3) |
| Replay | single-use secret, fresh nonces on both sides, transcript v3, ACK over the head hash |
| Downgrade or cross-flow confusion | `v:3` QR, `renroll` routes and domain strings distinct from forward `v:2` |

## 8. Spec amendments (written with the code, reviewed with it)

- §22.13: replace "reserved" with this design's roles, messages, checks
  and the EN rows.
- §5: one paragraph: "the reverse flow (§22.13) mirrors this sequence
  with the roles of §22.13's table; this section is otherwise unchanged".
- FFI catalogue §4: the phone ops `authorize_begin`, `authorize_reply`,
  `authorize_confirm`, `authorize_finish`, `authorize_abort` (and their
  stream use); §3 unchanged (seal is already a phone crossing).
- §1.5 Mac helper ops: `renroll_begin`, `renroll_hello`,
  `renroll_bundle_begin`, `renroll_confirm` (the panel), `renroll_complete`,
  `renroll_finish`, `renroll_abort`.

## 9. Tests (EN rows, §22.16)

| ID | Test |
|---|---|
| EN-01 | end to end in process: an iPhone engine (C ABI) adds a fresh Mac helper through the provider core; the Mac unlocks, holds the items, syncs with the iPhone directly, and the provider activates its key after the iPhone publishes |
| EN-02 | a reply that does not open the QR commit is refused; a swapped key on either side gives two different codes |
| EN-03 | the Mac keeps nothing without the panel's Continue; a cancel on either side leaves no registry entry and no files |
| EN-04 | a bundle whose last entry is signed by a key other than `auth_key`, installs other keys or another id, or comes from another vault: refused |
| EN-05 | wrong secret 5 times: session closed; expired QR; reused secret |
| EN-06 | iPhone preconditions: behind, removed, pending security change, not fresh → refused before any prompt |
| EN-07 | wrong MP → `WRONG_CREDENTIAL` with backoff; Face ID declined → nothing signed |
| EN-08 | interrupted attempt on the Mac removed whole at the next start; on the iPhone, no entry before the ACK |
| EN-09 | a Mac without Touch ID joins and opens with the MP |
| EN-10 | a phone-recovered vault adds a Mac (the F.2d stated gap closes) |

## 10. Plan (after this review closes)

One chunk, reviewed per chunk (owner decision 2026-10-11):
1. Engine: transcript v3, commit, the phone's `authorize_*` ops, the
   Mac's `renroll_*` ops (sharing the forward consumer's checks), the
   anchor variant.
2. Mac main: the ephemeral server's three `renroll` routes, the Vault tab
   "Join from your iPhone" screen with the QR.
3. iPhone: Security → "Add a Mac": scan, compare, confirm, progress.
4. Tests EN-01…10, spec and catalogue text.

## 11. Open questions for the owner

None blocking. Two defaults, both changeable:
- The Mac panel names the vault (recovery handle and item count) before
  the vault is kept. Default: yes.
- After joining, the new Mac is a full device (it can authorize, rotate,
  revoke), like any enrolled Mac. Default: yes.
