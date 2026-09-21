# Phase E Verification Report — device identity + enrollment

Date: 2026-09-20. Spec: `credential-vault-implementation-spec.md` v0.3.1
§2.7, §2.8, §2.9, §2.12, §4, §5, §11.4, §12 scenario 8, §18 Phase E.

Scope: Secure Enclave device identity, device envelopes over the proven
§2.12 Path A bridge, the §5 enrollment protocol with its ephemeral TLS
server, registry enrollment, per-device backup credential issuance,
revocation with its mandatory VK rotation, and the macOS + iPhone
surfaces for all of it. **Synthetic vaults and synthetic credentials
only.** Nothing from Phase F (remote production backup), Phase G
(iPhone approval), the Chrome extension or the Dashlane importer was
started, and no real credential was handled.

Status: **gate green, and verified on two real devices** — a MacBook Air
(M2) and an iPhone 13 mini, each using its own Secure Enclave keys,
through four enrollments and three revocations (§7b). Remaining open
items are release-gate work, not Phase E work (§10).

---

## 1. What landed

| Area | Files |
|---|---|
| SE keys + FFI | `vault-helper/src/device/se.rs`, `vault-apple-crypto/Sources/VaultAppleCrypto/Bridge.swift` |
| Device identity | `vault-helper/src/device/identity.rs` (`SeDevice`, `device.json`) |
| Envelopes | `vault-helper/src/device/envelope.rs`, `creds.rs`, `rotate.rs` |
| Enrollment (helper) | `vault-helper/src/enroll/{session,transcript,wire,vectors}.rs`, `vault-helper/src/vault/{enroll_ops,enroll_commit}.rs` |
| Devices / revocation | `vault-helper/src/vault/devices.rs`, `vault-helper/src/registry/log.rs` |
| Rotation journal | `vault-helper/src/storage/{rotation,rotation_journal}.rs` (`ExtraStaging`, marker `stage`) |
| macOS app | `src-tauri/src/core/vault_enroll/{session,server}.rs`, `src-tauri/src/app/commands/vault.rs` |
| Frontend | `src/components/vault/DevicesPanel.tsx`, `src/lib/vault.ts` |
| iPhone (separate repo) | `SourceMobile/Vault/{VaultKeys,VaultRegistry,VaultEnrollment,VaultSupport,VaultView}.swift`, `SourceMobileTests/VaultVectorsTests.swift` |
| Gate | `scripts/phase-e-gate.sh` |

## 2. Secure Enclave device identity (§2.7, §2.8)

Two SE-resident P-256 keys per device, by role — signing (registry
entries, enrollment ACK) and agreement (HPKE envelopes) — created
together at vault creation and referenced by one Keychain tag.
`device.json` holds public material only: device id, name, platform, the
tag, and the two 65-byte uncompressed public keys.

- **Never one key for both roles** (§2.7): asserted distinct, both
  on-curve, both `0x04`-prefixed (DV-01).
- **Randomized signing, canonical wire form**: SE/CryptoKit ECDSA is not
  deterministic, so two signatures over the same digest differ and both
  verify; every signature is normalized to low-S before it enters a
  hash-chained object, and is verified against this device's own recorded
  public key before it ships (DV-02, `ecdsa::normalize_low_s`).
- **Prehash correctness**: CryptoKit's `signature(for: Data)` would hash
  the digest a second time. The bridge signs through a `Digest`-conforming
  wrapper so the §2.7 domain-separated prehash is what gets signed. This
  was a real bug, caught by DV-02 before anything depended on it.
- **Missing keys are detected, not assumed** (§2.8): deleting the SE keys
  makes the identity unloadable (`DEVICE_NOT_AUTHORIZED`) rather than
  silently trusted (DV-03).

## 3. Device envelopes (§2.2, §2.5, §2.9, §2.12)

`wraps/devices/<device_id>.wrap` is the §2.2 `DeviceEnvelopePayload` (VK +
that device's own backup credential) sealed with HPKE base mode at the
proven suite, to the device's SE agreement key. `info` is exactly
`"ov0/envelope/v1" || vault_id || device_id || enrollment_nonce`.

Evidence (EV-01…EV-03): a round trip through the Enclave; the
encapsulated key is 65 bytes; changing the vault id, the device id or the
enrollment nonce makes the envelope unopenable; a flipped ciphertext byte
fails; an envelope sealed to another device's key does not open with this
one's. The file format itself is a Phase E addition — the spec fixes the
payload and the HPKE parameters, not the framing (§2.5 updated).

**Envelopes rotate with the VK.** They and the issuer's credential record
are staged through the rotation journal and listed in the commit marker's
new `stage` array, so a rotation cannot commit a new VK while leaving a
device holding a wrap of the dead one. Each envelope keeps the credential
its device was issued and the enrollment nonce it was first bound to, so
only the VK inside changes (RC-01 asserts both).

## 4. Enrollment (§5)

The helper owns every decision; the main process owns only the socket.

1. `begin_enrollment {fp}` — the main app has already bound its ephemeral
   TLS listener, so the certificate fingerprint the phone will pin off the
   screen enters the helper's transcript. The helper mints a 16-byte
   single-use secret (26 characters in the repo's unambiguous base32) and
   a 16-byte nonce, TTL 300 s.
2. `enroll_hello` — constant-time secret check, five failures end the
   session; both public keys must be distinct, on-curve, 65-byte points;
   platform and name are validated. **The Mac assigns the new
   `device_id`** and returns it with `nonce_e` and `mac_device_id`, so a
   new device cannot choose its own registry identity.
3. **SAS** — 8 characters from `HKDF(transcript, info="ov0/enroll/sas/v1")`.
   The code is *never* transmitted: each side derives it from the same
   transcript, which is what makes comparing the two screens meaningful.
4. `enroll_confirm` — LA presence on the Mac, then the signed enroll entry
   and the sealed envelope. **Nothing is written to the registry yet.**
5. `enroll_ack` — the phone's signature over
   `SHA-256("ov0/enroll/ack/v1" ‖ registry_head ‖ mac_device_id)`.
   Verifying it is what appends the entry, writes the Mac's copy of the
   envelope and records the issued credential.

Failure behavior, all tested: wrong secret ×5 tears the session down and
even the right secret then fails (EN-02); a replayed hello is refused
(EN-03); a forged ACK is `SIGNATURE_INVALID`, leaves no registry trace,
and destroys the session so the real device cannot rescue it (EN-04); an
ACK over a different head does not verify (EN-05); malformed identities
are refused before any transcript exists (EN-06); a lock in flight
destroys the session (EN-07); presence denial stops everything (EN-08).

**Transfer bundle**: exactly a §11.2 snapshot — objects, signed manifest,
§4.8 checkpoint — plus the envelope. All ciphertext or public
verification state; no key material crosses IPC.

## 5. Registry (§4) and backup credentials (§11.4)

- Vault creation now writes the **genesis entry**, self-signed by this
  Mac's SE key, and `header.registry_head` / the manifest head name it
  from the first moment (they move in lockstep on every append).
- Enrollment appends a signed `enroll` entry; `list_devices` reports the
  verified chain's view.
- The authorizing device issues a fresh 32-byte credential per device
  inside the envelope and keeps a VK-sealed record of what it issued
  (`wraps/devices/creds.bin`), because re-sealing at rotation must
  preserve the credential the device already holds (§11.4: credentials
  rotate only by re-enrollment). Registration at a provider is Phase F;
  issuance and preservation are proven here.

## 6. Revocation (§11.4, §12 scenario 8)

`revoke_device` is presence → master password → **new Recovery Key,
acknowledged** → registry `revoke` entry → VK rotation that re-envelopes
every surviving device.

- RC-01: registry entry written, `vk_generation` 1→2, the revoked
  device's envelope removed, the Mac's re-sealed (new VK, same credential,
  same enrollment nonce), records still readable, heads still in lockstep.
- RC-02: the VK the revoked device took away at enrollment opens **none**
  of the rotated records — revocation is cryptographic, not a flag.
- Self-revocation is refused (a vault with no authorizer is not a state
  worth reaching); an unknown device is `NOT_FOUND`.
- A dismissed Recovery Key window or a wrong master password aborts
  everything: no entry, no rotation, no Recovery Key shown.

## 7. macOS app and iPhone

**macOS**: an ephemeral rcgen certificate and an OS-chosen port, bound
only for the session, three routes (`hello`, long-polled `bundle`, `ack`),
torn down on completion, cancellation, expiry or tab unmount. The Vault
tab lists devices, shows the QR, shows the SAS with copy that says
plainly what a mismatch means, and offers removal.

**iPhone** (`~/Documents/source mobile`): generates its own SE keys,
pins the QR's fingerprint before sending anything, derives the SAS
itself, verifies the registry chain, checks that the entry installing it
carries exactly the keys it generated, opens the envelope inside its own
Enclave, and only then ACKs and stores. Anything that fails wipes its
keys and its stored state.

## 7a. Revocation-status refresh (owner decision, 2026-09-21)

The first hardware run exposed a gap the test suite could not: after the
Mac revoked an enrolled iPhone, the iPhone went on displaying "Paired
with your Mac's vault", across app restarts. Revocation is a local
registry write on the Mac and the enrollment channel is gone by then, so
nothing had told it — and Phase E has no phone-side vault usage that
would have failed and revealed it.

The revocation itself was never in question: the VK had rotated and the
device's envelope was deleted, so the phone could decrypt nothing written
afterwards. What was wrong was the phone asserting a trust relationship
it had never verified. The owner's decision was to implement both halves:

**The Mac stays authoritative.** Revocation is unchanged — registry
revoke entry, mandatory VK rotation, deleted envelope, re-encrypted
state. Nothing a phone says can alter the registry, and in particular no
device can trigger a VK rotation (and a new Recovery Key) remotely.

**The phone asks; it is never told.** `registry_status` (helper) returns
the signed registry and the vault id, and one route —
`GET /v1/vault/registry` — serves it over the already-paired,
TLS-pinned channel under the same bearer auth as every other route. It
is read-only: no records, no VK, no wraps, no recovery material, no
backup credentials, no writes, and not sync. Two tests pin that surface,
one on the helper's response shape and one on the route's response type.
The op deliberately works while the vault is **locked** — the registry
involves no VK, and a phone must be able to discover a revocation
whether or not someone is using the Mac.

**The phone verifies before believing.** It parses the chain, checks
every signature, and enforces a rollback floor: it remembers how much
chain it has accepted, and a Mac answering with less history — or
different history at a seq it already accepted — is rejected as
tampering rather than read as revocation. Only a cryptographically valid
revoke entry naming *this* device clears local state.

**Event-driven, never polling** (§4.7): app launch, foreground, the
Vault or Settings screen opening, reconnection to the Mac (hooked to the
upload queue's unreachable→delivered edge, so it fires once on the edge
rather than per delivery), and a manual check. No background polling
infrastructure was added.

It is a *status refresh*, not a notification: there is no guaranteed
delivery and the Mac never pushes. The phone is correct when no answer
ever comes.

### What the UI now claims

A stored key proves this device was enrolled once; it does not prove the
Mac still considers it active, so the UI stopped saying "Paired":

| State | Shown as |
|---|---|
| Verified recently, still enrolled | "Active in your Mac's vault" + when it was confirmed |
| Enrolled, not confirmed lately | "In your Mac's vault" + "Last confirmed …" |
| Mac unreachable or verification failed | same calm line; the reason appears **only** if the user asked for a check |
| Valid revoke entry for this device | "Removed from the vault" — loud, and the only state that deletes anything |

The unreachable case deliberately shares the calm presentation: a Mac
that is asleep is the ordinary condition, and an alarm there teaches
users to ignore the one alarm that matters. A network failure never
reads as revocation and never wipes a key.

Two vocabulary defects surfaced in review and were fixed: "forget this
vault" on the phone used to collapse into "Not paired", which read as a
contradiction against the Mac still listing the device — it now says
"Vault removed from this iPhone" and states that the Mac still lists it;
and the Mac's device list now says what it means ("devices allowed to
open this vault … a device that deletes its own copy stays on this list
until you do"). Devices also show when they were enrolled, because a
phone re-paired after a removal reports the same model name as the
entry it replaced.

### Tests (all green)

| Case | Result |
|---|---|
| Active device stays active after a valid refresh | pass |
| Valid revoke entry marks this phone revoked and clears its enrollment state | pass |
| Another device's revocation does not affect this phone | pass |
| Truncated registry rejected | pass |
| Rewritten history at a seen seq rejected | pass |
| Tampered registry rejected | pass |
| Entry signed by a non-authorizer rejected | pass |
| Network failure changes nothing locally and never reads as revocation | pass |
| Restart after confirmed revocation stays revoked | pass |

The fixtures build genuinely signed TLV chains — canonical encoding, the
same domain separators, low-S signatures — so these run the production
parser and chain verifier rather than a stub.

## 7b. The hardware run (MacBook Air M2 + iPhone 13 mini)

§18's Phase E gate asks for two real devices enrolling, revoking and
rotating on synthetic vaults. Done, on a synthetic vault at `/tmp/pe/v`,
with the user's real vault untouched throughout. The registry is the
transcript:

```text
seq 0  genesis  48e84410…  MacBook Air          01:01:46
seq 1  enroll   5b1796d2…  iPhone               12:54:18
seq 2  revoke   5b1796d2…                                  vk 1 → 2
seq 3  enroll   a6d83e9a…  iPhone 13 mini       13:45:51
seq 4  revoke   a6d83e9a…                                  vk 2 → 3
seq 5  enroll   021bdbb0…  iPhone 13 mini       13:56:47
seq 6  enroll   d76c933c…  iPhone 13 mini       14:11:25
seq 7  revoke   d76c933c…                                  vk 3 → 4
```

Every enrollment used the iPhone's own Secure Enclave keys, its own SAS
derivation, and its own envelope decapsulation inside its Enclave. Every
revocation rotated the VK and deleted that device's envelope. Each
re-pairing minted a **new** device identity — `5b1796d2`, `a6d83e9a`,
`021bdbb0`, `d76c933c` are four distinct identities with distinct keys,
which is §4.4 rule 5 holding in practice rather than in a test.

The last two entries are the revocation-status refresh working end to
end: the Mac revoked `d76c933c` at seq 7, and the phone — asked nothing,
told nothing — fetched the signed registry, verified the chain, found
its own revoke entry, reported "Removed from the vault" and deleted its
key and envelope.

`021bdbb0` remains enrolled on purpose: that phone chose "forget this
vault", which is local cleanup and not revocation. From the Mac's side a
forgotten device and a fully armed one are indistinguishable, because a
device's claim about itself is not evidence. Both UIs now say so.

### What the hardware run broke that the suite did not

Beyond the six defects in §5 above, the second round found four more,
all fixed:

| Defect | Why the suite missed it |
|---|---|
| Recovery Key window never said *why* it appeared, or that the previous key had stopped working | the window's copy had no test; now two |
| iOS status stuck at the value computed before enrollment | the status object outlived the state it described |
| A helper op added but the signed bundle never rebuilt — the phone was told "your Mac's vault isn't running", which the user could see was false | the tests ran against `cargo` output, the app runs the signed bundle |
| Two devices sharing a model name were indistinguishable in the list | no test looks at a list with duplicate names |

The third is the one worth keeping: a route that flattens every failure
into one message will eventually tell the user something they can see is
untrue. It now distinguishes "too old to answer", "no vault", and
"couldn't answer".

## 7c. Phase E.1 closure pass (2026-09-21)

Three items, at the manager's direction, with no new architecture beyond
what the spec already required.

### Argon2id wording corrected

An earlier revision recorded the v1 hardware floor as raised to
A15-class and the tuple as frozen. That was not an explicit owner
decision — it read an exploratory conversation as a settled one — and
§19 item 30 requires one, or an A12-class measurement, to close.
Reverted across the spec (§2.3, §21 OQ-3, §19 item 30), the calibration
notes, this report and the review summary. The tuple `m=64 MiB, t=3,
p=1` is provisional again, no A12 performance is estimated, and the
tuple is not weakened while the gate is open.

### §2.8 device-envelope unlock — implemented

Presence check → the Secure Enclave decapsulates
`wraps/devices/<self>.wrap` → the VK and this device's
`device_backup_cred` come back together → the vault opens, with no
master password. The agreement private key never leaves the Enclave.

The refusals carry the security weight, because **an envelope on disk is
not authority to open a vault**:

| Condition | Result | Why |
|---|---|---|
| No SE key (restored device, wiped key) | `DEVICE_NOT_AUTHORIZED`, vault stays LOCKED | §2.8 calls this documented behavior; the MP path remains, which is what makes it recoverable |
| Device revoked in the registry | refused even though its envelope is on disk | the registry decides, not the file |
| Envelope from before a VK rotation | `WRAP_CORRUPT` | it holds a key retired by §2.10 |
| Another device's envelope planted in this slot | refused | sealed to an Enclave this Mac does not have |
| Presence denied | LOCKED, **not** counted as a wrong credential | a refusal is the user declining, not a failed guess |

Six regression tests (DU-01…DU-06) cover exactly those rows, against
real Secure Enclave keys and real envelopes. The main app now takes this
path by default and falls back to the master password **only** when the
device has no usable envelope — never on a denied presence check, which
would turn "cancel" into "try harder".

### iOS `recovery_epoch` compatibility — closed, no protocol circularity

A vault that has been through total-loss recovery could not enroll an
iPhone: the registry contains `recovery_epoch` entries whose §4.5 proofs
are keyed by VKs retired at the moment each epoch committed, so no newly
enrolled device can ever verify one. The phone refused the chain rather
than trusting it, which was correct and unusable.

The resolution was already in the Phase D.1 design and needed no new
architecture — a fresh enrolling device is in exactly the position §4.8
was written for. The **order of the bundle checks is what resolves it**,
and is now normative in §4.8:

1. open the envelope → the current VK;
2. verify the §4.8 checkpoint under that VK — only the vault itself can
   produce it, so this is what says "this registry head is mine";
3. verify the chain anchored on that head: §4.4 rules in full, signed
   entries still verifying under their authorizer, `recovery_epoch`
   entries checked structurally rather than by an extinct proof key;
4. confirm the head matches the checkpoint and the bundle, and that the
   entry installing this device carries the keys this device generated.

§4.4 was not weakened, and one rule was **added** that the Swift side
had been missing: every entry declares the epoch it belongs to, only a
`recovery_epoch` may advance it, and an entry claiming any other epoch
is rejected. That surfaced as a test failure it would have been easy to
dismiss as a bad fixture.

Tests — 13 on the phone (all green), plus a Rust cross-check:

| Case | Result |
|---|---|
| Ordinary enrollment, no epochs, still works without any checkpoint | pass |
| An epoch is refused when no checkpoint is in hand | pass |
| A recovered vault enrolls a new iPhone under a valid checkpoint | pass |
| A checkpoint MAC'd under the wrong VK is rejected | pass |
| An authentic checkpoint for a different head cannot be replayed | pass |
| Tampered recovery history is rejected | pass |
| Epoch must advance by exactly one | pass |
| Rollback/fork rules still enforced under anchoring | pass |
| No vault key, current or extinct, is ever persisted on the phone | pass |

The Rust cross-check lives in the enrollment end-to-end test: it decodes
the bundle's checkpoint, authenticates it under the VK the envelope
yielded, and asserts it binds the same head, vault and key generation
the Swift client demands. A drift on either side now fails there rather
than on a user's phone.

## 8. Tests and vectors

| Suite | Tests |
|---|---|
| `device_identity` | 7 — SE roles, randomized low-S signing, missing keys, envelope round trip, info binding, cross-device refusal, no secret in `device.json` |
| `enrollment` | 8 — EN-01…EN-08 above |
| `revocation` | 5 — RC-01, RC-02, refusals, two abort paths |
| `xv_vectors` | 6 — including the new **XV-ENROLL** family |
| iOS `VaultVectorsTests` | 5 — the same XV-ENROLL values recomputed in Swift |

XV-ENROLL pins what a second implementation gets silently wrong: the
transcript hash, the SAS derivation, the ACK digest, the envelope `info`
bytes, and the base32 alphabet. HPKE ciphertexts are randomized and
cannot be a fixed KAT; the suite itself stays pinned by the §2.12 PoC
vectors.

The end-to-end enrollment test is not a mock: the "phone" is a second,
independent Secure Enclave identity on this Mac. It opens the envelope in
its own Enclave and signs the ACK with a key the enrolling side never
sees, so both sides run production code.

## 9. Gate results

```text
PASS  Phase E tests (SE identity, envelopes, enrollment, revocation)  26 passed, 0 failed
PASS  full helper suite (Phases A–E)                                 181 passed, 0 failed
PASS  file-length audit (≤350) + §2.12 bridge (≤200)                 bridge 200 lines
PASS  debug helper build+sign+verify (with Swift bridge)             OU 9RGW34CMA2, deep-strict + DR
PASS  release helper: all OV0_VAULT_* overrides compiled out         none present
PASS  E2E: SE genesis entry, list_devices, enrollment session        65/65 distinct keys; 26-char secret;
                                                                     cancel leaves no trace
PASS  no VK / backup credential / MP in any IPC frame or log         transcript scanned
PASS  main-app IPC surface: no key-bearing command argument (UI-05)  none
PASS  main app cargo check + frontend build                          green
PASS  supply chain (audit, vet, no new crypto crate for HPKE)        96 deps (gate < 120); hpke absent
PASS  iPhone: XV-ENROLL vectors + revocation-status rules            25 tests on iPhone 17 Pro Max
PASS  Phase D gate regression (incl. C, B, A)                        PHASE D GATE: PASS (12 checks)

PHASE E GATE: PASS (12 checks)
```

**Re-run after the Phase E.1 closure pass**, 21 September 2026 — same
12 checks, all green, with the suites grown by the new work:

```text
PASS  Phase E tests (SE identity, envelopes, enrollment, revocation)   32 passed  (was 26: +6 device unlock)
PASS  full helper suite (Phases A–E)                                  187 passed, 0 failed
PASS  iPhone: XV-ENROLL vectors + revocation-status rules              35 tests   (was 25: +10 epoch/checkpoint)
PASS  Phase D gate regression (incl. C, B, A)                         PHASE D GATE: PASS (12 checks)

PHASE E GATE: PASS (12 checks)
```

The first attempt at this run failed one check, and the failure was
worth having: a Phase A test asserted that `unlock` answers `UNKNOWN_OP`,
which was true for as long as nothing implemented it. Implementing §2.8
made it answer `BAD_STATE` on an uninitialized vault instead — the
correct refusal. The expectation was updated and the test extended to
keep covering a genuinely unknown op, so the original coverage did not
quietly disappear with the fix.

Run of record, 21 September 2026, ~70 minutes including the nested
regression (Phase D → C → B → A, of which 10 minutes is Phase B's fuzz
pass over the TLV decoder).

One note on the evidence itself. An earlier run reported the iPhone
check as `PASS … 0` — the tests had genuinely run and passed, but the
counter in the gate script matched a single byte against xcodebuild's
multi-byte `✔`, so the evidence line was meaningless. A gate that cannot
evidence a pass should not claim one, so the counter now reads the
"Test run with N tests" summary and **fails the check when the count is
absent or zero**. The run above is from after that fix.

## 10. Open items, stated as open

1. **Argon2id support-floor calibration remains open.** The tuple
   `m=64 MiB, t=3, p=1` stays **provisional**. §19 item 30 closes either
   by measuring an A12-class iPhone or by an **explicit** owner change to
   the v1 support floor. Raising the floor to A15-class was discussed on
   2026-09-21 and briefly recorded here as decided; that was a misreading
   of an exploratory conversation and has been reverted. The iPhone 13
   mini figures remain evidence for A15 hardware only.
2. **Real-printer exercise** (§19 item 29) is unchanged and still open.
3. **CS-03 keystroke suppression** stays vacuous until macOS keystroke
   capture exists.
4. **A phone learns about revocation only when it asks.** The refresh is
   event-driven and unacknowledged: a phone that never runs, never has
   network, or is never opened will go on holding a key it no longer has
   access with. That key opens nothing — the VK rotated at revocation —
   so the exposure is a stale local claim, not access. Guaranteed
   delivery would need a push channel, which Phase E does not have.
5. **Delivering a re-sealed envelope to an offline device is Phase F.**
   After a rotation the surviving device's new envelope sits on the
   authorizing Mac; until sync exists, that device cannot open the new
   state. Stated rather than papered over.
6. **iOS registry verification covers §4.4 rules 1–5 and 8.** A
   `recovery_epoch` entry (rule 6) is *refused* rather than trusted,
   because authorizing one needs a VK the phone does not hold at
   enrollment time. A vault that has been through total-loss recovery
   therefore cannot enroll a phone until that path is implemented.
7. **Envelope-based unlock is not wired.** §2.8 describes unlock as
   "HPKE-decapsulate `devices/<self>.wrap` with the SE agreement key
   after an LA presence check". This Mac's envelope now exists and
   carries its backup credential, and the decapsulation path is proven
   (EV-01), but the unlock path is still the Phase C master-password
   flow. Switching it is a change to the lock state machine (§13) rather
   than to envelope code, and was left out of Phase E deliberately rather
   than half-done.
8. **Keychain concurrency.** SE key blobs live in the login keychain
   (the data-protection keychain needs an entitlement the gate and test
   binaries do not carry). That keychain's global lock stalls under
   concurrent access from several threads; the helper serializes vault
   ops, and the SE test binaries take the same `serial()` guard the other
   op suites use. This was observed, not theorized — eight parallel test
   threads deadlocked inside `SecKeychainItemCopyContent` before the
   guard was added.

## 11. Spec deviations and clarifications (all synced into v0.3.1)

1. **Typed enrollment ops instead of an opaque relay pair.** §1.5 listed
   `relay_to_device` / `relay_from_device`. Enrollment uses typed ops
   (`enroll_hello`, `enroll_confirm`, `enroll_ack`, `cancel_enrollment`)
   because the helper must route those frames into its session state
   machine anyway, and a typed schema is something it can validate. The
   opaque relay remains for §6 approvals.
2. **The Mac assigns the new `device_id`** (§5.2 row added).
3. **Device-envelope file shape and the credential record** are specified
   (§2.5); the credential record's HKDF context is a new §2.9 row.
4. **Revocation issues a new Recovery Key** (§11.4), because the rotation
   it mandates rewrites `recovery.wrap` and the helper never retains the
   RK.
5. **Envelopes rotate inside the rotation journal** (§2.10), via the
   commit marker's new `stage` array.
6. **`begin_enrollment` takes the certificate fingerprint** rather than
   returning host/port: the main process binds the listener first, and the
   fingerprint must enter the helper's transcript.
7. **Bridge surface** (§2.12): 8 production symbols, `aad` on seal/open,
   PoC-only entry points moved out of the shipping bridge into the PoC's
   own shim. The keychain choice is documented there too.
8. **The sealed envelope crosses IPC; the payload never does.** §1.5's
   never-list named "DeviceEnvelopePayload bytes". The *sealed* envelope
   is HPKE ciphertext addressed to another device's Secure Enclave key —
   neither the main process nor this Mac can open it — and the helper has
   no network, so the enrollment bundle is the only way it reaches the
   enrolling device. The never-list now says **plaintext** bytes and
   states the exception explicitly.
9. **`vault.db`/manifest heads move with the registry** — `set_registry_head`
   keeps header and manifest in lockstep so §3.5 catches a registry from a
   different point in time.

10. **One vault route on the always-on mobile server** (owner decision,
    2026-09-21). §5 kept vault traffic off that server so it would not
    become enrollment attack surface. `GET /v1/vault/registry` is the
    single, read-only exception, carrying public verification state
    (§4.7) that the phone verifies itself. It is not enrollment, not
    sync, and has no write path. Called a *registry status refresh*
    rather than a notification, because nothing guarantees delivery.
11. **`registry_status` answers while the vault is locked.** Every other
    device op requires an unlocked vault. This one must not: the
    registry involves no VK, and a phone has to be able to learn it was
    revoked without someone first unlocking the Mac.

## 12. One regression this phase introduced, and how it surfaced

Linking the Swift bridge into the helper **library** made every crate
that links `vault_helper` need the Swift runtime's `@rpath` search path —
and `cargo:rustc-link-arg` does not propagate from a dependency's build
script. The helper's own binaries and tests were fine; the main app's
test binary and the fuzz target died at load with
`Library not loaded: @rpath/libswift_Concurrency.dylib`.

Phase E's own eleven checks were green while this was broken. What caught
it was the **Phase D → C → B regression**, which is the entire reason the
gates nest. The fix adds the runtime search path in the two crates that
produce final binaries against the helper (`src-tauri/build.rs`,
`vault-helper/fuzz/build.rs`), each with a comment saying why.

## 13. Supply chain

Phase E added **no Rust dependency**. HPKE comes from Apple CryptoKit
through the §2.12 bridge, so the `hpke` crate stays confined to the PoC
workspace and the shipping helper's graph is unchanged. The macOS app
reuses `rcgen`, `axum-server`/rustls and `qrcode`, all already present.
The iPhone app adds no third-party package (§17.3).
