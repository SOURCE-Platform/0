# Phase F.2 design — iPhone vault client and direct Mac ⇄ iPhone sync

Date: 2026-09-29. **Revision 3** (after the reviews of revision 1,
`088c37f`, and the bounded re-review of revision 2, `dfd088a`). Status:
**owner decisions recorded (§11.1); next: spec amendment; no code.**
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
| Active-but-compromised device (unlocked thief, malware) — until revoked | everything a device may do with the authority it holds; win a revocation *race* | freeze other devices or poison their committed state (§4.3); make its own revocation impossible once the owner acts (§4.3, §6.2) |
| Revoked device (holds old VKs and its keys) | speak the peer protocol, forge revisions under the old VK | have anything it delivered survive the revocation (§4.6); obtain anything newer |
| Stolen iPhone or Mac **with its device passcode / login password** | pass the device's presence check | revoke other devices, change the MP or RK, authorize enrollment, or reset the MP (§6.2, both platforms per F2-D3) |
| Stolen locked iPhone | the hardware | the VK or plaintext (Data Protection + Enclave + no resident VK) |
| The provider | as Phase F | as Phase F, also via peers: forwarding confers no trust (§4.3) |
| Mac main process / same-user process with the TLS key | relay and tamper with peer bytes | change any signed peer content; wipe or revoke a phone (§5) |
| Other apps, iCloud backup, screenshots | platform-level access | vault files in a device backup (§7.3); secrets captured without a warning (§7.4) |

Stated residuals: a thief who snatches a phone **while the vault is
unlocked** can read it and can delete records with presence alone
(mitigated: bulk deletion needs the MP, and a restore path exists for
records whose latest change came from a later-revoked device, §6.3); a
passcode alone does not open the vault because the agreement key is
biometry-bound (§6.2); iOS cannot block screenshots (§7.4); the VK lives
in a networked app process on iOS unless F2-D2 separates the app (§7.1);
a compromised device can still *race* the owner's revocation until it
lands (§6.2 narrows what it can do in that window).

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
| `peer_revs_want` / `peer_revs` | serve: UNLOCKED or LOCKED; receive: UNLOCKED only (a LOCKED device keeps a bounded inbox, AEAD-opened and admitted at unlock) | revision objects and their full parent closure (§4.5) |
| `peer_status` | any | the signed registry status that replaces the Mac refresh (§5); **answered for any key the registry ever installed, including revoked ones** — it is public data and the only way a revoked device learns its status (spec §1.5, §4.7, §11.8) |

Caps per request, per response and per session; a per-peer quota for
pending revisions; rate limits per device (spec §1.3 style).

### 4.2 Who may speak

Except for `peer_status` (§4.1), "active" means **active in the receiver's local committed registry**,
including its own unpublished entries, **and not the target of a pending
revocation or a revocation awaiting redo** (§11.3.2 `awaiting_redo`). An
unknown, revoked or pending-revocation sender gets nothing, and a
requester checks that the responder is exactly the addressed device.

### 4.3 Two tiers: committed and provisional

`state_commit` is built from public inputs, so a still-active compromised
device can build a validly signed "provider state" the provider never
committed. A receiver therefore keeps two tiers:

- **Committed** — moved *only* by states the device's own provider
  `state_get` confirmed (same `state_commit`). It is the only input to
  fork and COMPROMISED decisions, the rollback floor, `seen` and
  `expected_state`, the publication base, singleton adoption (registry,
  header, wraps, VK, envelopes) and `Admit` sets.
- **Provisional** — forwarded states and `peer_status` entries. They are
  verified exactly as provider states (v0.4.1) and may only: deliver
  revisions at the current VK generation (recorded with the forwarder as
  the delivering peer; others wait); lock this device on its own
  revocation (§5); stop talking to a device; inform the UI.
- A provisional entry the provider's chain later contradicts is
  **discarded and attributed to its source** — never fork evidence, never
  COMPROMISED. The copy says "your Mac or your backup provider disagrees"
  (a provider equivocating between devices looks the same), and never
  recommends revoking the forwarder on this evidence alone.
- A forwarder forwards only a provider-committed state, byte-for-byte with
  its `recovery_auth`, or declines.
- Consequence: a singleton change (a rotation, a revocation) reaches a
  device that is offline from the provider only as revisions it cannot
  yet open (they wait) plus a banner; it is *adopted* at the device's next
  provider contact.

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
  not enough against a thief holding the old VK); revisions delivered
  inside a forwarded `peer_state` count as delivered by the forwarder.
  Adoption re-seals only revisions that are in a **provider-confirmed**
  state or in the device's own authoring journal.
- Residual, stated: a device that has not yet learned of D's revocation
  can still *send* D its new edits (sealed under the old VK). Mitigation:
  a device pushes local-only revisions to a peer only after a successful
  provider check within the last 15 minutes, and "sync now" does one.

### 4.6 Exchange

On app foreground, on "sync now", on reconnection — never in the
background (spec §4.7): `peer_hello` (floors + a heads summary as a
Merkle-style list per record-id prefix so differences are found in
O(log n) rounds) → `peer_state` for the side behind → `peer_revs_want` /
`peer_revs` for the differing records → local merge. Local-only revisions
reach the provider at the next publication by either device.

### 4.7 Transport

The pinned TLS channel on the local network: the phone **refuses to
connect with no stored pin**; the vault pin comes from QR pairing (camera-free
pairing is not accepted for vault traffic — its 20-bit code is too weak)
and is **stored separately from the SOURCE pairing pin** (camera-free
pairing can overwrite that one today); the vault pin is on the SPKI
(aligning the phone with spec §5.2) — existing phones re-pair for vault
traffic (F2-D2 (a) re-enrolls anyway); main checks
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

### 6.2 Authority: the device passcode is never enough (F2-D3)

Unlock: Face ID → own envelope → VK in memory; MP as fallback. Every
**authority-changing** op — revoke a device, rotate the RK, change the
MP, authorize an enrollment, and bulk deletion — requires the **current
MP**, verified against the committed `password.wrap`. Resetting a
forgotten MP (§1.5 `mode:"reset"`, and RC-01's "set a new MP" branch)
requires the **RK**. **F2-D3 applies this to the Mac as well** (a Mac
thief with the login password is the same adversary): it changes
owner-approved §12 scenario 5 and architecture §10.2 ("presence → set a
new MP" becomes "RK → set a new MP").

Keys (spec §2.7 amended): the **agreement key** (it opens the envelope,
i.e. the VK) is created with `.biometryCurrentSet` access control, so a
passcode or a face a thief adds cannot open the vault; the **signing
key** keeps no biometric control, so peer and provider requests and
background retries still sign. Consequences, stated: re-registering Face
ID invalidates the agreement key → the phone unlocks with the MP (which
re-seals a fresh envelope to a new agreement key via a normal
re-enrollment of that key); existing Phase F phones need new keys, i.e.
re-enrollment (combined with F2-D2 (a)).

### 6.3 What the phone can do (scope)

| Op | Phone | Notes |
|---|---|---|
| add / edit / delete / reveal records | yes | revisions authored by the phone; bulk deletion needs the MP |
| restore records changed last by a later-revoked device | yes | from the retained history |
| RC-01: revoke the Mac, rotate, publish | yes | MP required (§6.2); the "set a new MP" branch (when a stolen Mac changed it) requires the RK |
| RK rotation, MP change | yes | §6.2 |
| revoke another phone | yes | §6.2 |
| authorize enrolling a replacement Mac | F2-D4 | reverse enrollment — its roles (who serves TLS and shows the QR, who assigns `device_id`, who shows the SAS, who signs, seals and publishes) get their own design review before §5 is amended |
| total-loss recovery onto a phone | yes | §12 scenarios 3/4, same checks as the Mac |
| scenario 8 (a surviving device after compromise) | yes | exposure set = the whole vault for a full-vault device |
| restore a damaged record from a peer (§3.6, §15) | yes | the peer's committed revision, verified and AEAD-opened |

### 6.4 Rotation, publication and background

The same journaled rotation and one-transaction merge (shared engine). On
iOS a rotation is **fully staged before the new RK sheet is shown**, runs
inside a background task, and if the app is killed the journal rolls back
and the UI says plainly that the shown sheet is void. Security-driven
publications retry at every foreground; **a security-driven cutoff
completes only when the app is next opened** (stated honestly: staging is
in memory, re-staging needs the unlocked vault, and the key blobs are
`WhenUnlockedThisDeviceOnly`); the banner persists until then. If the
owner chooses on-disk staging (F2-D5), a `BGProcessingTask` can finish a
fully staged publication while locked.

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
   envelope is opened (the §5 enrollment bundle is anchored on the
   QR-pinned channel); forwarded states are provisional and never move
   the committed tier (§4.3).
3. Anything only a revoked device delivered is refused on revocation
   (§4.5); a revoked or pending-revocation device gets nothing (§4.2).
4. Singleton changes only as verified provider transitions (§4.4, with
   the two stated exceptions).
5. Rollback: a peer cannot move a device below its provider floor; the
   only removals are §3.2 revoked-author deletions and §4.5 provenance
   refusals.
6. No automatic deletion of key material or the store from any peer or
   provider response (§5).
7. Crash safety: the shared journal and one-transaction merge.
8. A device passcode or login password alone never changes authority
   (§6.2).

## 9. Tests (spec §16 additions)

- **PA** peer authentication: wrong audience/receiver, stale time, replay
  (across a restart), revoked / unknown / pending-revocation sender,
  unbound response, body swap by main, cross-protocol confusion, the
  helper refusing to sign a caller digest.
- **PS** peer exchange: missing parents, stale generation waits, unknown
  author waits, caps and quotas, forged tombstone refused by AEAD,
  forwarded forged / substituted / uncommitted state (provisional, blamed,
  never COMPROMISED), a chained-but-uncommitted singleton state (never
  adopted), the honest NEEDS_USER-redo case (no COMPROMISED), revocation
  still possible afterwards, provenance cutoff on revocation, a revoked
  phone still getting `peer_status`, a LOCKED receiver's bounded inbox.
- **EN** iPhone-authorized enrollment (if F2-D4). **MT** first
  materialization (forged-vault join refused).
- **IO** iOS: Keychain class, agreement key biometry-bound and signing
  key not, Face ID re-registration → MP unlock path, file protection,
  backup exclusion (on device), lock on background during rotation,
  capture hiding.
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
§17.3 (iOS supply chain); §19; plus §3.2 (provenance, open-before-admit,
waiting, parent closure), §2.10/§11.3 (re-seal only provider-confirmed or
own-journal revisions — a Mac behaviour change too), §4.6/§15 (forwarder
attribution; the peer-path exception to COMPROMISED), §11.8 (revoked
devices still get `peer_status`), §12 scenario 5 and §6.4 item 5 (F2-D3),
§7.1 (whether Phase G reuses PeerRequest); architecture §1, §5, §10.2,
§11.1, §16 and C8.

## 11. Owner decisions

- **F2-D1** One Rust engine on both platforms (recommended).
- **F2-D2** The vault in its own iPhone app (recommended) vs inside SOURCE
  Mobile with mitigations. Consequences of (a): its own QR pairing and
  pin, existing enrolled phones re-enroll as new devices, and Phase G
  approvals move into it.
- **F2-D3** On **both** the Mac and the iPhone, the MP (never the device
  passcode / login password) for authority changes, the RK for any MP
  reset; the phone's agreement key biometry-bound (recommended). Changes
  owner-approved §12 scenario 5 and architecture §10.2; Face ID
  re-registration means an MP unlock and a key refresh.
- **F2-D4** The iPhone can authorize a replacement Mac (recommended — else,
  after losing the Mac, getting a Mac back needs total-loss recovery,
  which removes the phone). Its protocol gets its own review first.
- **F2-D5** Inherited from Phase F: #1 COMPROMISED only on verified fork
  evidence (this design's peer-path rule depends on it); #2 in-memory vs
  on-disk staging (decides whether a phone can finish a security cutoff in
  the background); #5 SY-13 restore behaviour; and how a user leaves
  COMPROMISED (still unspecified).

### 11.1 Decisions recorded (owner, 2026-09-29)

| Decision | Choice |
|---|---|
| F2-D1 | **One Rust engine on both platforms.** |
| F2-D2 | **(a) A separate "SOURCE Vault" iOS app** (own pairing and pin; existing phones re-enroll; Phase G approvals move into it). |
| F2-D3 | **Yes, on both Mac and iPhone:** the MP for authority changes, the RK for any MP reset; the phone's agreement key biometry-bound. |
| F2-D4 | **Yes:** the iPhone can authorize a replacement Mac; its protocol gets its own design review first. |
| F2-D5 #1 | COMPROMISED only on fork evidence signed by the vault's own devices (confirms the v0.4.1 implementation; §15 `SIGNATURE_INVALID` erratum). |
| F2-D5 #2 | **On-disk staging** of fully staged publications (ciphertext and public data only), so a security cutoff can finish in the background. |
| F2-D5 #5 | SY-13: a restored older store opens **read-only until it has synced** (instead of refusing to unlock). |
| F2-D5 exit | Design a safe exit from COMPROMISED, for owner review. |

## 12. Review

Revision 2 addressed the revision-1 findings (security SEC-B1…B4, I1…I10;
spec SPEC-B1…B4, I1…I12); revision 3 addresses the bounded re-review
(security SEC-NB1/NB2, NI1–NI4, NO1–NO3; spec SPEC-B5…B7, I13–I18,
O6–O8). The review loop for this design is closed. Next: the owner
decisions, then the spec amendment (its own review), then code; F2-D4's
reverse-enrollment protocol gets its own design review.

### 12.1 Spec v0.5 amendment review (2026-09-30)

Candidate `bc59691` (spec §22 + markers), reviewed by the security and
spec reviewers; every finding was checked against the repository before a
disposition was recorded.

| Finding | Disposition |
|---|---|
| SEC-B1 / SPEC-B1 — key refresh "in the same publication" not acceptable to other devices (`apply.rs:111`), and a phone-authorized enrollment outside §5 | **Accepted.** Reverted to this design's wording: MP unlock, then a normal re-enrollment through the Mac; a phone-side self-refresh belongs to the F2-D4 review. |
| SEC-B2 / SPEC-B2 — re-seal rule vs §3.2/§2.10; unpublishable parent (`422 INDEX_INVALID`) | **Accepted.** §22.7 now defines sources, set-aside, re-authoring of own descendants, publish-first, and the revoker's cutoff; §3.2 and §2.10 marked; PS-08/09/10, SY-11 amended. |
| SEC-I1, SEC-I3, SPEC-I6 — provisional revocation lock, post-recovery `peer_status`, which registry is served | **Accepted** (§22.9). |
| SEC-I2, SPEC-I5 — one-way protocol; serve-side freshness | **Accepted** (§22.7, §22.8). |
| SEC-I4, SPEC-I2, SPEC-I3 — staging validation and `pending_remote` fields | **Accepted** (§22.11, §11.3.2: `version`, `in_flight`, `awaiting_redo`, `target_device_id`, `staged_publication`). |
| SEC-I5 — FFI list vs the envelope-open callback | **Accepted** (§22.2 audited crossings). |
| SEC-I6, SPEC-I8 — background completion on a screen-locked iPhone | **Accepted** as a stated limit; protection classes unchanged (changing them would be an owner decision). |
| SEC-I7, SPEC-I7 — "installed" vs "active" | **Accepted**; "active at the accepted head", CX-02. |
| SEC-I8, SPEC-I1, I9, I10, I13, I14 — stale or unmarked passages; pin wording | **Accepted**; passages amended, §5.2 pin erratum, `peer_endpoint`. |
| SEC-I9 — exit proposal may not be able to publish | **Recorded** in the proposal as an open point for the owner's review. |
| SPEC-I4 — body encodings, stream carriage, `peer_state` size | **Accepted in part**: gating, status value and direction fixed now; byte encodings go to a wire annex reviewed before F.2c code. |
| SPEC-I11 — tests without IDs; §19; EV-03 in the nested gate; XV independence | **Accepted** (§22.16 table, §19 item 31, gate text, CryptoKit-only Swift target). |
| SPEC-I12 — bulk-deletion counter, restore from history | **Accepted** (`kv` counter, `restore_revision`, AU-06/07). |
| SEC-O1…O4, SPEC-O1…O11 | **Accepted** except none rejected; O7 footer/header wording aligned. |

**Bounded re-review of revision 2 (`4930568`), 2026-09-30: no blockers
from either reviewer.** Their remaining text-level findings were fixed in
revision 3 of the candidate and the loop is closed:

| Finding | Fix |
|---|---|
| SPEC-N1 / SEC-I1 — publish-first index not ancestor-closed | it omits D-only revisions and their descendants; best-effort; never delays the revocation (§22.7) |
| SPEC-N4 / SEC-I2 — "active" and the racing revocation | active = in the provider-confirmed registry, own unpublished entries not counted; §11.3 rule-2 exception unchanged; CX-05 (§22.12, §4.6) |
| SPEC-N3 / SEC-I3 — post-recovery `peer_status` unreachable | the exception is dropped: after a recovery the phone shows `BACKUP_ACCESS_LOST` and re-enrolls; PS-14 and the §11.8 marker amended. This narrows this design's §4.1 claim: `peer_status` reaches a device revoked by an ordinary revocation, not one cut off by a recovery epoch |
| SPEC-N2 / SEC-I4 — restore vs the tombstone rule | restore of a deleted record creates a new record; `list_history` / `list_deleted` added (§22.4) |
| SEC-I5 — re-enrollment losing the phone's unpublished edits | the phone publishes first, or the user is told the count (§22.4) |
| SPEC-N5…N11, SEC-O1 | wording aligned (signed vs unsigned limits, gating, annex scope, SY-10/11, lock copy, device-test marks, PV-01, pin wording, token storage, counter clock) |

Open for the owner: the COMPROMISED exit procedure (§22.12 proposal).
Open for a separate review: reverse enrollment (F2-D4) and, before F.2c
code, the peer wire annex.
