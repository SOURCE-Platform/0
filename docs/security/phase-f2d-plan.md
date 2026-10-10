# Phase F.2d — phone authority: plan

Date: 2026-10-10. **Revision 2**, after the spec and security reviews of
revision 1 (`e7da81a`).

Spec: v0.5 §22.17 milestone 4 ("publication, rotation, revocation (RC-01),
total-loss recovery on a phone"), plus:
- §2.7 (master-password adoption);
- §22.5 (tiers);
- §22.7 (freshness, publish first);
- §22.9 (lifting the removal lock, `BACKUP_ACCESS_LOST`);
- §22.10 (iPhone rules: staged-first rotation, background task, file
  protection);
- §22.11 (on-disk staging);
- the items earlier milestones deferred here (phase-f2-verification
  §5–§9).

This is an internal working plan; each step goes through the usual review
loop. **Steps 2, 5 and 6 change the FFI catalogue or the §22.2 seal
crossing, which is a spec change. Each gets its own design review before
code (SPEC-I10).**

## Steps

### 1. Master-password adoption — in review

It must land before any other device can publish (§2.7, owner decision
2026-10-02).

- The served `wrap_mp` is opened with the master password when this
  device's envelope answers `DEVICE_NOT_AUTHORIZED`.
- The revision-1 review asked for these fixes, all before closing (see
  phase-f2-verification §10.1):
  - **Gate:** the prompt is raised only after the apply's own
    verification of the served state (chain, head, signer, manifest,
    extension), and after the floor anchoring when behind. It no longer
    checks against the local registry (SEC-B1, SPEC-I4, VER-I2).
  - **KDF:** the wrap's KDF block must equal the served header's,
    policy-checked, before any prompt (SEC-I1).
  - **Backoff:** a wrong password drives the §15 backoff (VER-B1,
    SEC-I2).
  - **Panel:** a dedicated "Apply a Security Change" panel. It is raised
    only by a user-started sync; a timer sync answers
    `MP_ADOPTION_REQUIRED` (SEC-I3).
  - **Base key:** a pending local rotation keeps its base key sealed
    under the current key, so a served state at the base generation needs
    no credential (SEC-I4).
  - **Tests and spec text:** tests for each property (VER-B2). The §2.7
    text covers the phone and the §22.4 relation; a §22.16 row is added
    (SPEC-I5, SPEC-I6).

### 2. The phone's provider path — design under review

`vault-coordinator` is linked into `vault-ffi`. Swift supplies one
transport callback.

**Spec amendments, written and reviewed before code (SPEC-I1):**
- §22.2: Swift's list gains "network carriage" (the pinned pairing and
  peer channels, and provider HTTPS through `http_send`). Each carries only
  ciphertext, public data and single-use requests the engine signed.
- §22.2: "the catalogue lists every exported entry point and every
  callback".
- §11.1: an iPhone mapping of the helper and main rows.
- §1.2: on SOURCE Vault the engine and the network share one process (the
  stated residual).
- Catalogue §1, §2, §4 and §6: the callback, the ops, state gating, and
  `provider_run` allowed while LOCKED.

**The in-process adapter (SEC-I6, SPEC-I2):**
- It never takes the op lane.
- It runs only a fixed list of provider sub-ops, checked like `IOS_OPS`.
  Recovery ops stay excluded until step 5's design.
- `MAX_REQUEST` applies to every frame it builds.
- Events are flushed between sub-ops.
- A thread-local re-entrancy guard answers `BAD_STATE` to a callback that
  calls the engine.
- A cancel flag is set by lock, by `ov0_engine_close` and by
  background-task expiry. It is checked before each `http_send` and passed
  to Swift.

**`http_send` contract (SEC-I7, SEC-O1, SPEC-I2):**
- Inputs: origin, method, path, auth, body, `timeout_ms`, `max_body`.
- The engine asserts that the origin equals the vault header's provider.
- The response body goes into an engine-owned buffer of `max_body`;
  anything over it is refused.
- Caps: `state_get` and commit/put answers ≤ 64 KiB; `blob_get` ≤ the
  transfer cap of that hash (≤ 8 MiB).
- "Unreachable" is reported apart from an HTTP status.
- Swift session: ephemeral, `urlCache = nil`, standard certificate
  validation (no pinning, D-12), and redirects refused.
- Called only on the op thread, never from lock, close or tick.
- Must not call back into the engine.

**Retry and access-loss state lives in the engine (SPEC-I3):**
- The §11.3.2 strike count is persisted in `kv`, with the policy
  constants and a phone BK-13 variant.
- Locate is probed at the engine's origin.
- Security-driven retry follows §11.3.2, with `BACKUP_REVOCATION_FAILED`.

Once the `provider_*` ops exist, the step-wise `backup_*`, `stream_*` and
`sign_provider_request` ops leave `IOS_OPS` (SEC-O2, SPEC-O6). `cargo vet`
scope covers `vault-coordinator` on iOS.

### 3. Using the provider path

- Freshness on the phone (§22.7). With it, the phone pushes local-only
  revisions (`peer_revs_put`).
- `peer_state` (provisional, §22.5). **Objects mode (PW-04) is either
  implemented or explicitly replaced by provider fetches (SPEC-I9).**
- A provider-confirmed state in which the phone is still active lifts the
  removal lock (§22.9).
- The `BACKUP_ACCESS_LOST` copy.
- *Sync* triggers: unlock and "Sync now". Publication of a staged change
  follows §22.10/§22.11 instead (step 4).

### 4. Publication from the phone

- The phone publishes its own edits through the coordinator.
- On-disk staging (§22.11) with the §22.10 file protection.
- A `BGProcessingTask` for a staged publication; `provider_run` while
  LOCKED.
- IO-06.
- Add and edit for logins in the phone UI (synthetic data until Phase J).
- **Before this step:**
  - §5.2 SEC-O3 (the early prompt for a signer only the served registry
    knows) is resolved by step 1's gate (SPEC-I4);
  - SEC-I4 of `e7da81a` must be closed: a pending local rotation keeps
    its base key, sealed under the current key, so a served base state
    needs no credential.

### 5. Authority on the phone — own design review before code

**Staged-first rule (SPEC-B1).** Every phone op that rotates the vault key
or issues a Recovery Key returns to `IOS_OPS` only in the §22.10
staged-first, background-task form. The rotation is fully staged before
the sheet, and the sheet says whether the key is live. This applies to:
- `rotate_recovery_key`;
- `revoke_device` (RC-01);
- recovery finalize;
- the scenario-8 sequences built from them.

IO-04 covers each one. Catalogue §4 is amended accordingly.

**Publish first (§22.7, SPEC-I8a).** A rotation the phone starts itself is
preceded by an ordinary publication, which omits revisions only the
revoked device delivered.

**Total-loss recovery on a phone:**
- an iOS identity with a biometry-bound key (not "This Mac");
- the origin from the engine's default;
- a fresh floor (§22.14);
- an `Uninitialized` start, which depends on step 6 for an already-paired
  phone.
- Stated: no Mac can join a phone-recovered vault until F.2e (§22.13).

**Seal crossing.** The §22.2 seal crossing gains its phone forms
(`ov0_hpke_seal` carries a VK, catalogue §3). They are reviewed here.

The owner decision "iPhone adds replacement Mac" needs reverse enrollment
(F.2e) and is not in this milestone.

### 6. "Remove this vault" — own design review before code

The explicit action deletes the phone's keys, store and Keychain items
(floor, `peer_endpoint`).
- It warns that this may be the only remaining copy, until
  `BACKUP_ACCESS_LOST` corroborates the removal, and whenever local-only
  revisions exist.
- It covers the F.2b ACK-lost path (VER-O1).
- It is how the owner's synthetic test vault (verification §7.3) is
  removed.

Before a Face ID re-registration, the phone publishes its pending
revisions, or warns how many would be lost (§22.4, SPEC-I8b).

### 7. Carried items

- the cached LOCKED store open;
- the header refresh and items-changed event after a peer put;
- the RK sheet says "this iPhone";
- §5.2 SEC-O4;
- §22.10 table rows: scenario 8 as the surviving device, and restoring a
  damaged record from a peer;
- AU-06 and AU-07 on the phone;
- the open floor and authorizer negative cases that need a provider
  fixture.

## Tests per step (§22.16 IDs)

| Step | Tests |
|---|---|
| 1 | new MA rows (§22.16); SY-13 (password-only Mac); IO-02 (phone adoption) |
| 2 | FFI-01; PA-07; catalogue §6 additions; PV-01 PR-01/BK-18 canaries over `http_send` transcripts |
| 3 | PS-02, PS-05–07, PS-12 (push side), PS-13, PS-14; CX-01…04 phone variants; SY-13 phone variant; PV-01 SY-01…12 variants; PW-04 and the requester side of PW-01/05; phone BK-13 |
| 4 | PV-01 ST-01…05, RU-01…05, BK-26/27; SG-01…03 phone variants; IO-06; AU-06; AU-07; PR-01/BK-18 over the phone's disk and logs |
| 5 | RC-01; AU-01, AU-03 (iPhone), AU-04; IO-04 for each rotating op; PS-08–10; CX-05; SY-10/SY-11; CP-01…08, FR-01…03, KD-01…03 on a phone; IO-02 (re-enrollment) |
| 6 | PS-13 / PS-14 "nothing deleted" until the explicit action; a new removal row |
| 7 | PW-11 (LOCKED behind); the persisted COMPROMISED evidence test |

## Owner-facing consequence

From step 2 on, the phone needs network access to the backup service.
The real service is owner infrastructure and is not configured yet. All
tests use the in-process provider core; a device run of steps 2–6 waits
for it.
