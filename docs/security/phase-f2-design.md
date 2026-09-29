# Phase F.2 design — iPhone vault client and direct Mac ⇄ iPhone sync

Date: 2026-09-29. **Revision 2** (after the security and spec reviews of
revision 1, `088c37f`). Status: **design for re-review; no code.**
Authorized by the owner ("yes i want direct Mac ↔ iPhone sync"); spec
v0.4.1 §18 requires this design review before implementation. Owner
decisions are marked **F2-D*n*** (§11).

Scope (spec §18 Phase F.2): an iPhone record store with the §3.2 merge,
key rotation and publication; iPhone-initiated revocation (§12 scenario 1,
RC-01); direct Mac ⇄ iPhone peer sync over the pinned channel.

## 1. Goals and non-goals

Goals: (1) the iPhone holds the whole vault and can read and edit offline;
(2) Mac and iPhone converge directly when they can reach each other and
through the provider otherwise; (3) the iPhone alone survives the loss of
the Mac (RC-01, total-loss recovery on a phone); (4) nothing weakens Phase
F, and the v0.4.1 rule holds everywhere: **trust is anchored on what a
device already accepted, never on a key or a state another party could
have chosen.**

Non-goals: Mac-to-Mac enrollment; sync without the provider across the
internet (no relay — §21 OQ-2 already fixes this for v1); push approvals
(Phase G); autofill (Phase H).

## 2. Threat model

| Adversary | Can | Must not be able to |
|---|---|---|
| Network attacker (any network the Mac or phone joins) | observe, drop, replay, redirect | read, inject, roll back, fork; pass as a device |
| Active-but-compromised device (unlocked thief, malware) — until revoked | everything a device may do | make revocation of itself impossible (§4.4, §4.5); poison other devices' floors (§4.4) |
| Revoked device (holds old VKs and its keys) | speak the peer protocol, forge revisions under the old VK | have anything it delivered survive the revocation (§4.6); obtain anything newer |
| Stolen iPhone **with its passcode** | unlock the phone, pass Face ID fallback | revoke other devices, change the MP or RK, authorize enrollment, or reset the MP (§6.2) |
| Stolen locked iPhone | the hardware | the VK or plaintext (Data Protection + Enclave + no resident VK) |
| The provider | as Phase F | as Phase F, also via peers: forwarding confers no trust (§4.3) |
| Mac main process / same-user process with the TLS key | relay and tamper with peer bytes | change any signed peer content; wipe or revoke a phone (§5) |
| Other apps, iCloud backup, screenshots | platform-level access | vault files in a device backup (§7.3); secrets captured without a warning (§7.4) |

Stated residuals: a thief who knows the passcode can *read* the vault on
the phone (§6.2 limits only authority changes); iOS cannot block
screenshots (§7.4); the VK lives in a networked app process on iOS unless
F2-D2 separates the app (§7.1).

## 3. One vault engine for both platforms (F2-D1)

**Recommendation: compile the Rust vault core for iOS** (store, §3.2 merge,
journal, rotation, publication, verification), behind a narrow C ABI;
Swift supplies the Enclave, Keychain, Face ID and UI. The existing Swift
verifier (`VaultCatchUp`, `VaultRegistry`, `VaultCheckpoint`) is retired in
favour of the engine — keeping both would recreate the divergence the
Phase F reviews kept finding. A Swift rewrite would also need Argon2id and
XChaCha20-Poly1305, which CryptoKit lacks. Costs: iOS Rust build and
supply chain (spec §17.3's Apple-only rule for iOS changes under either
option — `cargo vet` scope extends to the iOS target), app size, and an
FFI boundary (§7.2). The XV vectors keep one independent check: the
engine against the committed Rust-generated vectors plus the retired Swift
verifier's tests ported as a fixture suite.

## 4. Peer sync protocol

### 4.1 Messages and signatures

New TLVs, their own domain prefixes (added to spec §2.9), never the
provider's:

```text
PeerRequest  prehash = SHA-256("ov0/peer/request/v1"  ‖ tlv)
  vault_id, sender_device_id, receiver_device_id, operation, body_sha256,
  t, n (16 B)
PeerResponse prehash = SHA-256("ov0/peer/response/v1" ‖ tlv)
  vault_id, responder_device_id, requester_device_id,
  request_prehash, status, body_sha256, t
```

- **The helper builds and signs every field itself**; no op signs a
  caller-supplied digest (§1.5). A response signature covers the body hash,
  so main or anyone in the path cannot swap the body.
- Verification order: signature (sender resolved in the receiver's
  registry) → time window ±300 s → replay cache (persisted across
  restarts for the window) → only then parse the body.
- Peer operation table (spec §1.5 additions, all helper ops, state-gated):

| op | receiver state | carries |
|---|---|---|
| `peer_hello` | any with a vault | floors, heads summary (§4.5) |
| `peer_state` | any with a vault | the latest *provider-committed* state, verbatim (§4.3) |
| `peer_revs_want` / `peer_revs` | UNLOCKED or LOCKED (ciphertext only) | revision objects and their full parent closure (§4.5) |
| `peer_status` | any | the signed registry status that replaces the Mac refresh (§5) |

Caps per request, per response and per session; a per-peer quota for
pending revisions; rate limits per device (spec §1.3 style).

### 4.2 Who may speak

"Active" means **active in the receiver's local committed registry**,
including its own unpublished entries, **and not the target of a pending
revocation or a revocation awaiting redo** (§11.3.2 `awaiting_redo`). An
unknown, revoked or pending-revocation sender gets nothing, and a
requester checks that the responder is exactly the addressed device.

### 4.3 Forwarded provider states are provisional

A peer may forward only a state *the provider committed* (its
`state_commit` as the provider served it), byte-for-byte, with its
`recovery_auth`; a forwarder that no longer holds those exact blobs
declines. The receiver verifies it exactly as a provider-served state
(v0.4.1), **but**:

- it never becomes `seen`, the rollback floor or `expected_state` until
  the device's own provider `state_get` shows the same `state_commit`;
- a divergence first seen on the peer path (a fork, a wrong
  `prev_manifest_hash`) **blames the forwarder**: the state is refused,
  peer sync with that device stops, and the user is told — it never
  enters vault-wide COMPROMISED;
- the device that signed any such evidence can always still be revoked.

### 4.4 Singleton changes stay linear

Enrollment, revocation, rotation, MP and RK changes are published only as
provider transitions (CAS). Exceptions, stated: a device's own local
commit (pending), and the §5 enrollment bundle (verified as §4.8 with the
v0.4.1 order).

### 4.5 Revisions: provenance and the revocation cutoff

- A revision received from a peer is **AEAD-opened under the current VK
  with its full AAD before admission** (a forged tombstone with garbage
  ciphertext never enters the graph).
- Batches carry the full parent closure; out-of-closure revisions are
  refused as malformed, never kept pending indefinitely. A revision at
  another VK generation, or by an author not yet known, **waits**
  (neither admitted nor counted), as `apply.rs` does today.
- **Provenance:** the store records which peer delivered each revision.
  When the device accepts D's revocation, it refuses every revision it
  received *only from D* that is not listed in D's revocation manifest's
  index — whatever author field it carries (§3.2's author-based rule is
  not enough against a thief holding the old VK). Adoption re-seals only
  revisions that are in a committed state or in the device's own authoring
  journal.
- Residual, stated: a device that has not yet learned of D's revocation
  can still *send* D its new edits (sealed under the old VK). Mitigation:
  the phone pushes local-only revisions to a peer only after a provider
  check within the last N minutes, and the UI's "sync now" does one.

### 4.6 Exchange

On app foreground, on "sync now", on reconnection — never in the
background (spec §4.7): `peer_hello` (floors + a heads summary as a
Merkle-style list per record-id prefix so differences are found in
O(log n) rounds) → `peer_state` for the side behind → `peer_revs_want` /
`peer_revs` for the differing records → local merge. Local-only revisions
reach the provider at the next publication by either device.

### 4.7 Transport

The pinned TLS channel on the local network: the phone **refuses to
connect with no stored pin**; the pin comes from QR pairing (camera-free
pairing is not accepted for vault traffic — its 20-bit code is too weak);
the pin is on the SPKI (aligning the phone with spec §5.2); main checks
the bearer token before any helper call; the Mac binds peer routes only on
private-network interfaces and refuses public source addresses. All of
this is defence in depth: the signatures of §4.1 are what authorize.

## 5. The Mac refresh becomes a signed peer status

The Phase E refresh (`GET /v1/vault/registry`, unsigned, served by main)
is replaced by `peer_status`, signed by the Mac helper under its registry
key and bound to the phone's nonce. On any channel the phone believes a
new `recovery_epoch` or its own revocation **only** with that signature
from a device active in its accepted registry, or with the epoch's proof
under the VK it holds. **The phone never deletes its store automatically:**
learning of its own revocation locks the vault, stops sync, and offers
"remove this vault from this iPhone" to the user. This closes the v0.4.1
residual (compromised main could force a re-enroll) and, in F.2, the data
loss it would otherwise cause.

## 6. The iPhone as a vault device

### 6.1 Trust state (one model)

The phone's anchors are its **accepted registry** (seeded at enrollment
from the QR-pinned bundle, then moved only by verified signed states) and
its **provider floor** (generation + manifest hash). First
materialization of the full vault (a phone enrolled in Phase F, or a new
one) = the v0.4.1 catch-up order — registry prefix → manifest signature →
own envelope → checkpoint — then fetching the index's revisions; **never
`join.rs`** (test-only). A full-vault phone accepts a new
`recovery_epoch` like the Mac: proof under the VK it holds, else via a
signed `peer_status` (§5). The Phase F two-floor model is migrated into
the engine; with signed peer status it collapses to one registry floor.

### 6.2 Authority on the phone (the passcode is not enough)

Unlock: Face ID → own envelope → VK in memory; MP as fallback. But every
**authority-changing** op on the phone — revoke a device, rotate the RK,
change the MP, authorize an enrollment — requires the **current MP**,
verified against the committed `password.wrap`; the device passcode never
satisfies it. The MP reset branch (§1.5 `mode:"reset"`) on the phone
requires the **RK**. Enclave keys use `.biometryCurrentSet` access control,
so a face added by a thief does not unlock them. (F2-D3.)

### 6.3 What the phone can do (scope)

| Op | Phone | Notes |
|---|---|---|
| add / edit / delete / reveal records | yes | revisions authored by the phone |
| RC-01: revoke the Mac, rotate, publish | yes | MP required (§6.2); includes the "set a new MP" branch when a stolen Mac changed it (§11.4) |
| RK rotation, MP change | yes | §6.2 |
| revoke another phone | yes | §6.2 |
| authorize enrolling a replacement Mac | F2-D4 | reverse enrollment |
| total-loss recovery onto a phone | yes | §12 scenarios 3/4, same checks as the Mac |
| scenario 8 (a surviving device after compromise) | yes | exposure set = the whole vault for a full-vault device |
| restore a damaged record from a peer (§3.6, §15) | yes | the peer's committed revision, verified and AEAD-opened |

### 6.4 Rotation, publication and background

The same journaled rotation and one-transaction merge (shared engine). On
iOS a rotation is **fully staged before the new RK sheet is shown**, runs
inside a background task, and if the app is killed the journal rolls back
and the UI says plainly that the shown sheet is void. Security-driven
publications retry at every foreground and via a `BGProcessingTask`; the
banner persists. Staging is in memory (Phase F deviation), re-staged from
`pending_remote` after a restart.

## 7. iOS platform rules

### 7.1 The vault boundary (F2-D2)

The iOS app today also hosts the Mac agent connection, audio recording
and background modes. Options: **(a) a separate "SOURCE Vault" iOS app**
(own process, own Keychain access group, no agent or audio code) —
recommended; (b) the same app with enforced mitigations: the engine is
unreachable from agent code, the vault locks whenever the app leaves the
foreground (including audio-background), and vault and agent are never on
screen together. Either way, architecture §5/§16 and spec §1.7 are
amended.

### 7.2 FFI contract

A catalogue of every C ABI entry point with a **never-crosses list**
(VK, PK, RK bytes, `sk_c` never cross; MP enters and RK words / record
plaintext leave only through the audited entry points); no raw-VK or
sign-arbitrary-digest function; panic = abort; Swift copies minimized and
zeroed where the type allows. A lint (like UI-05/SC-01) checks the surface.

### 7.3 Storage

Keychain: `WhenUnlockedThisDeviceOnly` (consistent with spec §2.7; no
class migration), access group not shared with any extension. Files
(`vault.db`, journal, wraps): `NSFileProtectionCompleteUnlessOpen`,
**excluded from iCloud/Finder backup** (`isExcludedFromBackup`); the
rollback-evidence floor lives in the Keychain; a restored or reinstalled
phone with a mismatched store refuses to author until it has synced.

### 7.4 Capture

Secrets and the RK sheet render in a secure text layer; hidden while
`UIScreen.isCaptured`; the app-switcher snapshot is blanked; a screenshot
of an RK sheet triggers a warning to replace the key. iOS printing goes
through the share sheet, with copy that warns about PDF copies.

## 8. Invariants (review checklist)

1. No plaintext, VK, PK, RK, MP or `sk_c` crosses the peer channel or the
   FFI outside the audited entry points (§7.2).
2. Every accepted state, from the provider or a peer, verifies against the
   device's accepted registry and the manifest signature before any
   envelope is opened; forwarded states are provisional (§4.3).
3. Anything only a revoked device delivered is refused on revocation
   (§4.5); a revoked or pending-revocation device gets nothing (§4.2).
4. Singleton changes only as verified provider transitions (§4.4, with
   the two stated exceptions).
5. Rollback: a peer cannot move a device below its provider floor; §3.2
   revoked-author deletions still apply (the only removals).
6. No automatic deletion of key material or the store from any peer or
   provider response (§5).
7. Crash safety: the shared journal and one-transaction merge.
8. The passcode alone never changes authority (§6.2).

## 9. Tests (spec §16 additions)

- **PA** peer authentication: wrong audience/receiver, stale time, replay
  (across a restart), revoked / unknown / pending-revocation sender,
  unbound response, body swap by main, cross-protocol confusion, the
  helper refusing to sign a caller digest.
- **PS** peer exchange: missing parents, stale generation waits, unknown
  author waits, caps and quotas, forged tombstone refused by AEAD,
  forwarded forged / substituted / uncommitted state (provisional, blamed,
  never COMPROMISED), revocation still possible afterwards, provenance
  cutoff on revocation.
- **EN** iPhone-authorized enrollment (if F2-D4). **MT** first
  materialization (forged-vault join refused).
- **IO** iOS: Keychain class, file protection, backup exclusion (on
  device), lock on background during rotation, capture hiding.
- **FFI** surface lint; **XV-PEER** vectors for the new TLVs.
- Phone variants of SY-01…13, ST-01…05, RU-01…05, BK-26…28, CP/FR/KD for
  recovery on a phone, RC-01 step by step (with the set-new-MP branch),
  the stolen-phone-with-passcode suite, and PR-01/BK-18 canaries (VK,
  plaintext, MP, RK, `sk_c`) over peer transcripts, the phone's disk and
  logs. Device tests asserted by name in the gate.

## 10. Spec and architecture amendments (to draft after the decisions)

Spec header and §18 (authorization, scope, gate); §1.2/§1.5 (peer ops);
§1.7 (iOS boundary); §2.7/§2.8 (phone Keychain and rollback item); §2.9
(peer prefixes); §3.1/§3.6 (iOS layout; restore from peer); §4.7 (peer
status replaces the refresh; "no general sync" / "not from a Mac route"
superseded); §4.8 (enrollment consumer); §5 (reverse enrollment, pin
rules); §11 intro, §11.3.2 (phone publisher copy and retries), §11.4
(peer vs provider signing); §12 scenarios 1, 2 (stolen full-vault phone),
7, 8; §13 (phone states); §14 (iOS capture); §15 (peer errors); §16 (§9);
§17.3 (iOS supply chain); §19; architecture §1, §5, §10.2, §11.1, §16 and
C8.

## 11. Owner decisions

- **F2-D1** One Rust engine on both platforms (recommended).
- **F2-D2** The vault in its own iPhone app (recommended) vs inside SOURCE
  Mobile with mitigations.
- **F2-D3** On the phone, the MP (not the passcode) for authority changes,
  and the RK for an MP reset (recommended).
- **F2-D4** The iPhone can authorize a replacement Mac (recommended — else,
  after losing the Mac, getting a Mac back needs total-loss recovery,
  which removes the phone).
- **F2-D5** Inherited from Phase F: in-memory staging (#2), SY-13 restore
  behaviour (#5), and how a user leaves COMPROMISED (still unspecified).

## 12. Review

Revision 2 addresses the revision-1 findings (security SEC-B1…B4,
I1…I10; spec SPEC-B1…B4, I1…I12). One bounded re-review, then the owner
decisions, then the spec amendment, then code.
