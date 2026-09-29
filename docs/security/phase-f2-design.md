# Phase F.2 design — iPhone vault client and direct Mac ⇄ iPhone sync

Date: 2026-09-29. Status: **design draft for review; no code.** Authorized
by the owner ("yes i want direct Mac ↔ iPhone sync"); spec v0.4.1 §18
requires this separate design review before implementation.

Scope (spec §18 Phase F.2): an iPhone record store with the §3.2 merge,
key rotation and publication; iPhone-initiated revocation (§12 scenario 1,
RC-01); direct Mac ⇄ iPhone peer sync over the pinned channel.

## 1. Goals and non-goals

Goals:

1. The iPhone holds the whole vault (ciphertext at rest, VK only while
   unlocked) and can read and edit it offline.
2. Mac and iPhone converge directly when they can reach each other, with no
   provider involved, and through the provider otherwise.
3. The iPhone alone can survive the loss of the Mac: revoke it, rotate the
   VK and publish (RC-01), or recover from total loss.
4. Nothing added weakens the Phase F properties; in particular the v0.4.1
   rule holds everywhere: **trust is anchored on what a device already
   accepted, never on a key another party could have chosen.**

Non-goals for F.2: Mac-to-Mac enrollment (owner decision pending), sync
over the internet without the provider (no relay), push approvals (Phase
G), browser autofill (Phase H).

## 2. Threat model additions

| Adversary | Can | Must not be able to |
|---|---|---|
| Network attacker on the LAN | observe, drop, replay, redirect peer traffic | read anything, inject anything accepted, roll a device back |
| A paired but **revoked** device (stolen) | speak the peer protocol with its old keys, hold old VKs | get anything newer than its revocation; have its post-revocation writes accepted anywhere (§3.2 `Admit(D)`) |
| The provider (untrusted) | as in Phase F | as in Phase F, including via the peer path (a peer forwarding provider data gains no trust by forwarding it) |
| A compromised Mac main process | drive the pinned channel | anything beyond the §1.4 boundary already accepted (v0.4.1 residual) |
| Physical attacker with a locked iPhone | the device | the VK or plaintext (iOS data protection + Secure Enclave + no resident VK while locked) |
| A malicious app on either device | normal app privileges | reach the helper/vault process or its Keychain items |

## 3. Architecture decision: one vault engine for both platforms

**Recommendation: compile the existing Rust vault core for iOS** (record
store, §3.2 merge, rotation journal, publication, verification — the code
in `vault-helper`/`vault-proto` minus macOS-only IPC), exposed to Swift
through a narrow C ABI, with Swift supplying the Secure Enclave, Keychain,
Face ID and UI.

Why: the Phase F reviews found real bugs precisely where the Mac and the
phone implemented the same checks twice (manifest floors, registry rule
4, checkpoint decoding). One implementation of merge, verification and
rotation removes that class of divergence, keeps one set of tests and one
fuzzing surface, and makes a pen test cover both platforms at once.

Costs: an iOS Rust build (the SQLite store via bundled `rusqlite`, the
Secure Enclave through the same Swift bridge pattern as
`vault-apple-crypto`), app size, and an FFI boundary that itself must be
reviewed. Alternative (reimplement in Swift) is rejected for divergence
risk. **Owner decision D-1.**

On the iPhone the engine runs in-process (iOS has no separate helper
daemon); the vault Keychain items use `WhenPasscodeSetThisDeviceOnly`,
the VK lives only in memory while unlocked (Face ID → own envelope in the
Enclave, §2.8), and the screen is protected from capture while secrets
are shown.

## 4. Peer sync protocol

### 4.1 What moves

Peer sync never carries a "state" that competes with the provider's
linear chain — that would turn honest divergence into fork evidence. It
carries two things:

1. **Revisions** (`OV0OBJ02` objects, sealed under the VK, bound to their
   author device) — merged with the §3.2 heads model, which is already
   order-independent and idempotent. A device admits a revision only at
   its current VK generation and only from an author active in its
   accepted registry (`Admit(D)` for revoked authors, unchanged).
2. **Verified provider states** a peer already accepted — forwarded
   verbatim (manifest, checkpoint, index, blobs). The receiver verifies
   them exactly as if the provider had served them (§4.7/§4.8 as amended
   in v0.4.1): forwarding confers no trust.

**Singleton changes stay linear.** Enrollment, revocation, VK rotation, MP
and RK changes are published only as provider transitions (CAS), where the
§11.3 singleton rule already resolves races. A device that is offline from
the provider can still receive such a change from its peer — as a
forwarded, verified provider state.

### 4.2 Channel and authentication

- Transport: the existing pinned TLS channel (Mac HTTPS server; the phone
  pins its certificate fingerprint from pairing, TOFU), local network only.
- Every peer request is signed like a `ProviderRequest` (§11.4) with the
  sender's Secure Enclave key and a peer audience
  (`peer:<receiver device id>`), fresh `t`/`n`, replay window. The
  receiver requires the sender to be **active in its own accepted
  registry**; a revoked or unknown sender gets nothing.
- Responses are bound to the request (the request prehash is echoed and
  signed by the responder).
- The pinned channel is served by the Mac main process, but peer requests
  are answered by the **helper** (§1.2): main only relays bytes, as it does
  for the provider.

### 4.3 Exchange

1. Each side announces its accepted provider floor (generation, manifest
   hash) and a compact summary of its heads (a hash per record set).
2. The side behind on the provider chain receives the verified provider
   state (4.1 item 2) and applies it through the normal verifier.
3. Both sides exchange the revisions the other lacks (by summary
   difference), each admitted by the rules in 4.1.
4. Local-only revisions are published to the provider by whichever device
   next publishes (the provider remains the durable copy).

## 5. iPhone as a full vault device

- **Unlock:** Face ID → own envelope (Enclave) → VK in memory; MP entry as
  fallback; auto-lock on background, timeout and screen lock.
- **Edits:** records created/edited on the phone are revisions authored by
  the phone's device id.
- **Rotation and publication:** the same journaled rotation and the same
  publication as the Mac (shared engine), signing provider requests with
  the phone's Enclave key.
- **RC-01 (Mac lost, iPhone retained):** the phone appends a signed
  revoke, rotates, publishes; the provider deactivates the Mac's key at
  that commit. Requires fresh Face ID and the MP (to re-seal
  `password.wrap`) and issues a new RK sheet (on-screen, capture-protected;
  printing as on the Mac).
- **Total-loss recovery on an iPhone** (§12 scenarios 3/4 with the phone
  as the new device): same flow and checks as the Mac.

## 6. Security invariants F.2 must keep (review checklist)

1. No plaintext, VK, PK, RK, MP or `sk_c` ever leaves the vault engine;
   the peer channel carries ciphertext and public data only (PR-01 scan
   extended to peer transcripts).
2. Every accepted state, from the provider or a peer, verifies against the
   device's accepted registry and the manifest signature before any
   envelope is opened (v0.4.1).
3. A revoked device obtains nothing new over the peer channel and its
   later writes are never admitted (§3.2).
4. No singleton change is ever accepted except as a verified provider
   transition.
5. Rollback: a peer cannot move a device below its provider floor or
   remove revisions it has admitted.
6. The phone never deletes key material on a provider or peer response
   (only the Mac refresh / own-revocation rules, §4.7).
7. Crash safety: the phone uses the same journal and one-transaction merge
   as the Mac.

## 7. Tests (to be specified in the spec amendment)

Peer versions of SY-01…SY-13 (merge), BK-04/05 (rollback/fork via a peer),
a revoked-peer suite, forwarded-state forgery suite (the v0.4.1 attacks
through the peer path), RC-01 end to end on devices, PR-01 over peer
transcripts, fuzzing of every peer message parser, and the EV-03-style
device run on a physical iPhone.

## 8. Owner decisions

- **D-1** One Rust engine on both platforms (recommended) vs a Swift
  reimplementation.
- **D-2** Peer sync on the local network only (recommended for F.2) vs
  also through a relay later.
- **D-3** iPhone auto-lock default (recommended: lock when the app leaves
  the foreground, and after 5 minutes of inactivity).

## 9. Review

Per the credential-vault workflow: freeze this draft, run the
`security-reviewer` and `spec-reviewer`, reconcile, one bounded re-review,
then the owner decisions above, then the spec amendment, then code.
