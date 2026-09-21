# Phase E — summary for review

Date: 2026-09-21. Spec: `credential-vault-implementation-spec.md` v0.3.1.
Full detail in `phase-e-verification.md`; this is the short version.

| | |
|---|---|
| Phase | E + E.1 closure pass. **Complete** |
| Gate | PASS, 12 checks, including the nested D → C → B → A regression |
| Hardware | MacBook Air (M2) + iPhone 13 mini — 4 enrollments, 3 revocations |
| Data | Synthetic vault, synthetic credentials only |
| Commits | `3550680`, `e0e8c7a`, `9d12d33` (desktop) · `3acb7ac`, `d6fe885` (SourceMobile) — **not pushed** |
| Phase F | Not started |

## What was built

Everything in the authorized Phase E list: production Secure Enclave
signing and agreement keys as separate keys (§2.7), device envelopes over
the Path A CryptoKit bridge proven in Phase E0, the §5 enrollment
protocol with its ephemeral TLS server, SAS verification, registry
enrollment, per-device backup credential issuance, revocation with the VK
rotation it mandates, and the macOS and iPhone surfaces for all of it.

Phase E added **no Rust dependency**. HPKE comes from Apple CryptoKit
through the §2.12 bridge, so the helper's dependency graph is unchanged
at 96 crates and the `hpke` crate stays confined to the PoC workspace.

## The revocation-status gap, and what shipped

The first hardware run surfaced this: after the Mac revoked an enrolled
iPhone, the iPhone went on displaying "Paired with your Mac's vault",
across app restarts. Revocation is a local registry write and the
enrollment channel is gone by then, so nothing had told it.

The revocation itself was never in question — the VK had rotated and the
envelope was deleted, so the phone could decrypt nothing written
afterwards. What was wrong was the phone asserting a trust relationship
it had never verified.

Per the owner's decision, both halves were implemented as a
**phone-initiated status refresh**, not a push notification:

- **The Mac stays authoritative.** Revocation is unchanged. Nothing a
  device says alters the registry, so no device can force a key rotation
  — or invalidate a Recovery Key — remotely.
- **The phone asks; it is never told.** One route,
  `GET /v1/vault/registry`, on the already-paired and certificate-pinned
  channel, backed by a `registry_status` helper op. It returns the signed
  registry and the vault id and nothing else: no records, no VK, no
  wraps, no recovery material, no backup credentials, no writes, not
  sync. Two tests pin that surface and fail if a field appears whose name
  looks like key material. It answers while the vault is **locked** on
  purpose — a revoked device must be able to find out without someone
  first unlocking the Mac.
- **The phone verifies before believing.** It parses the chain, checks
  every signature under §4.4, and enforces a rollback floor against what
  it has already accepted: a Mac answering with less history, or
  different history at a seq it has accepted, is rejected as tampering
  rather than read as revocation. Only a cryptographically valid revoke
  entry naming that exact device clears local state.
- **Event-driven, never polling:** launch, foreground, the screen that
  reports vault standing opening, reconnection, manual refresh.

### One deliberate deviation

Four UI states were specified. *Unable to verify* was given the same calm
presentation as *not recently verified* rather than its own alarm: a Mac
that is asleep is the ordinary condition, and alarming on it teaches
people to ignore the one alarm that matters. The four states remain
distinct in the model and in what gets acted on — only the volume
differs. Trivial to make literal if preferred.

## The hardware run

§18 asks for two real devices enrolling, revoking and rotating on
synthetic vaults. The registry is the transcript:

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

Four distinct device identities with distinct keys, so §4.4 rule 5 held
in practice rather than only in a test. Every revocation rotated the VK
and deleted that device's envelope. The last two entries are the status
refresh end to end: the Mac revoked `d76c933c`, and the phone — asked
nothing, told nothing — fetched the signed registry, verified it, found
its own revoke entry, and deleted its key.

`021bdbb0` is still enrolled on purpose. That phone chose "forget this
vault", which is local cleanup and not revocation. From the Mac's side a
forgotten device and a fully armed one are indistinguishable, because a
device's claim about itself is not evidence. Both interfaces now say so.

## What the hardware runs cost, and what they bought

**Ten defects the test suite had not caught.** Round one: a blocking
socket handed to tokio that killed the enrollment server on start; an
activation yield not awaited, so the helper's Touch ID sheet could open
behind the app window; the Swift bridge linked into the helper library
without the runtime search path downstream binaries needed; iOS pairing
state read once and never refreshed; the device announcing itself as a
privacy-gated generic "iPhone"; and re-pairing reusing Secure Enclave
keys that §4.4 rule 5 says must be new.

Round two: the Recovery Key window never explaining why it appeared
during a device removal, or that the previous key had stopped working;
the iOS status object outliving the state it described; a helper op added
without rebuilding the signed bundle, so the phone reported "your Mac's
vault isn't running" — something the user could see was false; and two
devices sharing a model name being indistinguishable in the list.

**Every one of the ten sits in a seam** — helper to app, app to phone,
source to signed bundle — which is exactly what unit tests on the helper
cannot reach. The argument is for hardware runs early in F and G rather
than as the final step of a phase.

A process note in the same spirit: an earlier gate run reported the
iPhone check as `PASS … 0`. The tests had genuinely passed, but the
counter matched a single byte against a multi-byte glyph, so the evidence
line was meaningless. The counter now reads the test-run summary and
**fails the check when the count is absent or zero** — a gate that cannot
evidence a pass should not claim one. The recorded run is from after that
fix.

## The E.1 closure pass

Three items, no new architecture — two were already specified and the
third was a correction.

**Argon2id wording.** An earlier revision recorded the v1 hardware floor
as raised to A15-class and the tuple as frozen. That was not an explicit
owner decision, and §19 item 30 requires one (or an A12-class
measurement) to close. Reverted everywhere; the tuple is provisional
again and the gate is open.

**§2.8 device-envelope unlock — implemented.** Presence check, the
Secure Enclave decapsulates this device's envelope, and the VK and
`device_backup_cred` come back together. No master password. The
substance is in the refusals, because an envelope on disk is not
authority to open a vault: the registry decides whether the device may
still unlock, a pre-rotation envelope is refused as a retired key,
another device's envelope cannot be planted into this slot, and a
missing Secure Enclave key yields the §2.8 re-enroll-or-recover
condition rather than looking like corruption. A denied presence check
leaves the vault locked and does not fall through to a password prompt —
that would turn "cancel" into "try harder". Six tests against real
Secure Enclave keys.

**Recovered vaults can now enroll a phone — and there is no protocol
circularity.** The problem was real: `recovery_epoch` proofs are keyed
by VKs retired the moment each epoch commits, so a newly enrolled device
can never verify one. But the Phase D.1 checkpoint already covers this
case; what was wrong was the order of operations. The phone now opens
its envelope first (yielding the current VK), verifies the §4.8
checkpoint under that VK, and only then verifies the chain anchored on a
head the vault itself vouched for. Historical epochs are audit evidence,
checked structurally, never re-proved. No extinct VK is transmitted,
used or stored.

§4.4 was not weakened — it was strengthened on the phone. Every entry
must declare the epoch it belongs to, only a `recovery_epoch` may
advance it, and an entry claiming any other epoch is rejected. That rule
surfaced as a test failure that would have been easy to write off as a
bad fixture. The Rust enrollment test now authenticates the bundle's
checkpoint exactly as the phone does, so a drift on either side fails
there rather than on a user's device.

**One gate check failed on the first attempt**, and it was worth having:
a Phase A test asserted `unlock` answers `UNKNOWN_OP` — true for as long
as nothing implemented it. Implementing §2.8 made it answer `BAD_STATE`
on an uninitialized vault, which is the correct refusal. The expectation
was corrected and the test extended so a genuinely unknown op is still
covered.

## Open items

1. **Argon2id support floor — still open.** The tuple
   `m=64 MiB, t=3, p=1` remains **provisional**. §19 item 30 closes
   either by measuring an A12-class iPhone or by an **explicit** owner
   decision narrowing the v1 support floor. Raising it to A15-class was
   discussed on 2026-09-21 and briefly written up here as decided —
   that was my error, reading an exploratory conversation as a
   confirmed decision, and it has been reverted. The iPhone 13 mini
   measurement (median 89 ms) is evidence for A15 hardware only and does
   not close the gate. No A12 performance is estimated and the tuple is
   not weakened in the meantime.
2. **Real-printer exercise** (§19 item 29) unchanged and still open.
3. **CS-03 keystroke suppression** stays vacuous until macOS keystroke
   capture exists.
4. **A phone learns about revocation only when it asks.** A device that
   never runs, or never has network, keeps holding a key — one that
   decrypts nothing, since revocation rotated the VK. Guaranteed delivery
   needs a push channel, which Phase E does not have.
5. **Delivering a re-sealed envelope to an offline device is Phase F.**
6. **iOS registry verification** covers §4.4 rules 1–5 and 8. A
   `recovery_epoch` entry is refused rather than trusted, so a vault that
   has been through total-loss recovery cannot enroll a phone yet.
7. **Envelope-based unlock is not wired** (§2.8). The envelope exists and
   the decapsulation path is proven; unlock is still the master-password
   flow. Left out deliberately rather than half-done.

## State of the tree

Committed separately and **not pushed**: `3550680` and `5a23ef4` in the
`0` repo (helper, macOS app, frontend, spec, verification report, gate),
`3acb7ac` in `source mobile` (the iPhone vault client). Both working
trees are clean. Phase F has not been started.
