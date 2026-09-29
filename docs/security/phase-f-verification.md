# Phase F Verification Report — provider protocol, multi-writer sync, total-loss recovery

Date: 2026-09-28.
Spec: `credential-vault-implementation-spec.md` v0.4 (approved, frozen) and
`phase-f-design-closure.md` (revision 3).

Scope: the Mac side of Phase F — the provider protocol (`vault-proto`,
`vault-provider-core`, the deployable `vault-provider`), the helper's
provider ops, multi-writer merge and singleton rules, remote-completion
status, total-loss recovery, the main-app coordinator and backup worker,
and the UI surfaces for all of it. **Synthetic vaults and synthetic
credentials only.** No real credential, Dashlane export, APNs key, S3
account or production origin was touched.

**Status: Mac side and iPhone catch-up reviewed and closed (spec
v0.4.1); Phase F itself is not yet closed.** What stops the Phase F exit
(§4) is EV-03 on the owner's physical iPhone, the fresh-environment
rehearsal, and the owner decisions in §5.

---

## 1. What landed

Commits `e79f0e3` … `b27e024` plus the final review-fix commit (m2–m9 and
two review rounds): 244 files, +18 427 / −4 122 at `b27e024`.

| Area | Where |
|---|---|
| Wire types, request signing, state commitment, recovery-auth keys | `src-tauri/vault-proto` |
| Provider logic (auth, validation, claims C1–C4, locate, throttle, GC) | `src-tauri/vault-provider-core` |
| Deployable service (axum + S3 SigV4, own workspace, Dockerfile) | `src-tauri/vault-provider`, `docs/security/vault-provider-deployment.md` |
| Helper sync/merge/publish/pending/recovery | `vault-helper/src/{sync,storage,recovery}` |
| Helper ops (§1.3 streams, §1.5 provider ops, handle retry, status) | `vault-helper/src/vault/{provider_ops,backup_ops,sync_ops,recovery_flow,retry_handle,remote_status}.rs` |
| Main-process flows | `src-tauri/vault-coordinator` |
| Main app (URLSession transport, backup worker, commands) | `src-tauri/src/core/vault_backup`, `src-tauri/src/app/commands/vault*.rs` |
| UI | `src/components/vault/{SetupCard,RecoverCard,BackupStatusLine,remoteCopy}.*`, `VaultPage.tsx` |
| iPhone (separate repo) | envelope v2 reader, named bundle registry (`fdec8cb`, `dd77a90`) |
| End-to-end simulator and scenarios | `src-tauri/vault-tests` |
| Gate | `scripts/phase-f-gate.sh` |

## 2. Review loop

Three independent reviewers (security, spec, verification) reviewed the
frozen candidate `efbdca2`, then re-reviewed the fixes at `b27e024` once.
The re-review found new problems introduced by the fixes; those were fixed
and covered by tests in the final commit (§2.3). No unlimited loop was
run.

### 2.1 Round 1 (candidate `efbdca2`) — blockers, all fixed in `b27e024`

| Finding | Problem | Fix | Test |
|---|---|---|---|
| VER-B1 / SPEC-B1 / SEC-B5 | a failed upload left a staged publication that blocked all later backup and sync until restart; revocations never retried | every outcome reaches the helper; transient failures keep the staging for a byte-identical re-send (also LOCKED); `backup_prepare` resumes it; attempts counted; session TTL enforced | `retry_ipc.rs` |
| SEC-B1 | a `create` re-staged while unlocked carried records sealed under a VK a handle retry later retired | a `create` is genesis-only; the provider refuses records in a create | `handle_retry.rs` |
| SEC-B2 | an older staging's commit cleared a newer pending security change | versioned `pending_remote`; a commit settles only the version it carried, else rebases | `pending_scenarios.rs` |
| SEC-B3 | `needs_user` never cleared; the redo never published | a change over a `needs_user` record starts afresh on the adopted base | `pending_scenarios.rs` |
| SEC-B4 / SPEC-I4 | a lost `200` was not recognized (false "redo", stale re-sends) | in-flight stagings recognized in a verified served state | `pending_scenarios.rs` |
| SEC-B6 | a pending rotation skipped checkpoint verification at the base generation | every served state is checkpoint-verified under its own VK (own envelope); target-authorized post-base entries are fork evidence | `pending_scenarios.rs` |
| SPEC-B2 | unsigned data could enter a permanent COMPROMISED (offer path) | fork evidence must verify under our registry; otherwise `SIGNATURE_INVALID`, no state change | `sync_scenarios.rs` |
| SPEC-B3 | `prev_manifest_hash` chaining not checked | checked for generation = accepted + 1 | `sync_scenarios.rs` |

Important findings fixed in the same commit: sync failures keep the vault
unlocked (SPEC-I3); COMPROMISED entry fails closed, drops staged writes and
signs reads only (VER-I2, SEC-O1); crash-safe recovery move and boot sweep
(VER-I1, SEC-I3); `rotate_recovery_key {suspected_theft}` (SPEC-I2,
SEC-I5); enrollment tracked as pending (SPEC-I6); session match in commit
results (SEC-I1); provider never writes a create into an existing vault
(SEC-I4); iPhone bundle registry lookup (SEC-I8b); `enroll_confirm` race
(SEC-O3); disconnect cleanup only by the current connection; honest worker
status (VER-I3); PR-01 with `sk_c` and every new key (VER-I4); CP-02/03
exact codes (VER-I5); BK-28 crash consistency and post-commit refusal
(VER-I6); CP-01 epochs and RU-03 re-send (VER-I7); gate fixes (VER-I8).

### 2.2 Re-review (candidate `b27e024`)

| Finding | Problem | Fix (final commit) | Test |
|---|---|---|---|
| SPEC-B5 / NEW-B1 (**blocker**) | the SEC-I4 guard made a re-sent `create` answer `200` without binding the handle (a crash between C3 and C4) — recovery by handle impossible | an identical re-send skips blob writes but still runs C1/C4 | HC-04 asserts `bound` |
| SPEC-I14 / NEW-B2 (**blocker**) | a change over a `needs_user` record dropped other adopted-away components and their security warning | components awaiting a redo are carried (`awaiting_redo`, `awaiting_security`) and survive the other change's commit | `an_unredone_revocation_survives_another_change` |
| SPEC-B2 (apply path, **blocker**) | `check_extends` ran before any verification, so a chained provider-built registry could still enter COMPROMISED | the served chain and manifest verify first; a divergence is a fork only if a device of our registry signed the manifest | `a_provider_built_registry_is_refused_not_a_fork` |
| NEW-I1 / VER-I10 / SPEC-O11 | a failed apply after a same-generation adoption restored the wrong VK | the VK is restored only if the committed singletons are unchanged | code |
| NEW-I2 / VER-O3 | a transient finalize failure discarded the recovery | kept and re-sent; `recovery_complete` resumes a completed attempt | `a_lost_finalize_answer_is_resent_not_discarded` |
| VER-I9 | the status line stuck on "Backing up…" while locked | the previous status stands when nothing ran | code |
| VER-I5 | CP-02 did not prove the checkpoint is needed | the forged manifest names the substituted registry's head | `cp02_…` |
| R1 | a colluding revocation target plus provider could install a device via a recovery epoch | with a revocation pending, any post-base recovery epoch is fork evidence | code |
| R2 | a replaced create landing late bound a different handle than the one kept | each in-flight create records its handle; the landed one is kept | code |
| VER-O2, SPEC-O12, VER-O5 | envelope `vk_generation` truncation; `412` not re-sent; unrecordable later-generation fork evidence; abandoned finalize identity | range check; `412` re-sent; ERROR when evidence cannot be recorded; identity wiped | code |
| SPEC-I1 | U-1 treated as a threshold; EV-03 pass condition too loose | U-1 recorded (decides batch signing); EV-03 requires a device destination and the tests by name | gate |

### 2.3 Deferred, with reasons

| Item | Why deferred |
|---|---|
| SPEC-B4 iPhone §4.7 catch-up, EV-03 | next milestone; EV-03 needs the owner's physical A15+ iPhone |
| SPEC-I7 revoke's "set a new MP" branch | UX; the reset-then-revoke workaround exists |
| SPEC-I8 remaining copy (device names, adopted-MP notice, refused-revision notices, both fork tips) | UI copy; the security warnings themselves are in place |
| SPEC-I9 SY-13 (unlock vs author after a restored `vault.db`) | owner decision (§5) |
| FR-02 long handles / `header.provider` on the sheet; CP-05 isolation; ST labels | display and test hygiene |
| SPEC-O14 enrollment pending not in the same commit as the append | a crash leaves an enrollment without its status line; the next publish still carries it |
| SEC-I6 "MP changed on another device" notice | UI; noted that it would make a skip-generation equivocation more visible |

### 2.4 iPhone §4.7 catch-up milestone and spec v0.4.1

The iPhone envelope catch-up (iPhone `ebda5ad`, Mac `6e92fb5`) was
reviewed by the security and verification reviewers, fixed, and
re-reviewed once (iPhone `39ac11c`, Mac `6a2156e`); the re-review's
findings were fixed in the final commits.

**The security finding that changed the spec (owner-approved v0.4.1).** A
device envelope is HPKE base mode, which authenticates no sender: a
provider can seal a VK of its choosing to any device's public agreement
key, and a checkpoint under that VK vouches only for itself. The v0.4
§4.7 order (envelope → checkpoint → registry, recovery epochs anchored by
that checkpoint) therefore let a provider acting alone get a forged state
accepted on the iPhone (SEC-B1/B2) and — on the Mac, in code already
reviewed — adopt a VK the provider chose and re-seal the vault under it
(SEC-B3; reproduced by `provider_forgery.rs`, which the unfixed code
accepted). Fixes:

| Where | Fix | Tests |
|---|---|---|
| Mac `apply` | a `recovery_epoch` the device has not accepted is believed only if its proof verifies under the VK it holds, for the manifest it last accepted; otherwise `SIGNATURE_INVALID`, no change | `provider_forgery.rs` (forged refused; a genuine recovery still revokes an up-to-date old device) |
| iPhone catch-up | order: registry (exact prefix of the Mac floor and the provider floor; only enroll/revoke after it; a new epoch → "confirm on your Mac"; nothing without a floor) → manifest signature → envelope → checkpoint | `VaultCatchUpTests` (forged epoch, fabricated vault, mid-chain genesis, swapped index, wrong size, signature flip, foreign checkpoint binding, wide key generation, rollback/fork/continuity) |
| iPhone registry verifier | §4.4 rule 4: a genesis only at seq 0 (a pre-existing gap the re-review found — SEC-B1-R) | `theVerifierRefusesAMidChainGenesis` |
| iPhone Mac refresh | a new recovery epoch believed only in the S-4 shape; recovered chains accepted (the E.1 defect) | `aRecoveryWithoutItsRevocationsIsNotBelieved`, `aGenuineRecoveryIsBelievedOnTheMacChannel` |
| iPhone floors | manifest floor = the Mac's last-accepted provider state (from the bundle); registry floor seeded at enrollment; the provider-path floor kept apart from the Mac's and undone (with the envelope and checkpoint) when the Mac's chain shows a fork | `storedOutcomeRaisesTheFloors` |
| iPhone robustness | strict checkpoint decode (a crafted key generation crashed the app), streamed size caps, no redirects, Keychain replace-in-place | `anOversizedKeyGenerationIsRefusedNotACrash` |

Accepted residual (recorded in §4.7): a compromised main process serving
the pinned channel could present a complete S-4-shaped chain and force a
phone to re-enroll; no secret is exposed. `sync/join.rs` (test-only in
Phase F) keeps a weak anchor, documented for F.2.

**Running EV-03 on the owner's iPhone** — only the two safe suites (the
whole target includes tests that reset the app's stored keys):
`-only-testing:SourceMobileTests/VaultCatchUpTests -only-testing:SourceMobileTests/VaultEV03Tests`.

## 3. Tests and measurements

Final run (the review-fix commit after `b27e024`): §3.1. Earlier runs: `efbdca2` 266
passed; `b27e024` 275 passed.

- Vault crates (`source-vault-helper`, `vault-proto`, `vault-coordinator`,
  `vault-provider-core`, `vault-tests`): see §3.1; 1 ignored — the U-1
  hardware measurement, allowlisted in the gate.
- `vault-provider` (own workspace): 4 passed; `cargo audit` clean.
- iPhone (`SourceMobileTests`, iPhone 17 Pro Max simulator): 36 passed.
- File-length audit, `tsc`, `cargo check -p SOURCE`: pass.
- **U-1 SE signing latency (MacBook Air M2):** mean 10.5–12.3 ms, p95
  12.7–16.9 ms over 200 requests (three runs) — per-request signing
  stands; no batch blob signing needed.

### 3.1 Final run

Vault crates: **278 passed, 0 failed, 1 ignored (U-1)**; `vault-provider`
4 passed; iPhone 36 passed (unchanged since `dd77a90`); file lengths,
`tsc` and `cargo check -p SOURCE` clean.

## 4. Phase F exit — what remains

1. ~~iPhone §4.7 envelope catch-up~~ — done (§2.4).
2. EV-03 on a physical A15+ iPhone, gated by name.
3. Fresh-environment rehearsal (kill both devices, recover) — needs a fresh
   macOS account and a reachable provider.
4. Remaining named tests: BK-03, BK-06, RF-06, RL-03, RU-01/02/05 by
   name, SY-13, ST-02/03, EV-05, CP-07/08, FR-03 later-device.
5. The owner decisions below.

## 5. Owner decisions (bundled)

1. **COMPROMISED entry** — implemented as "verified fork evidence only";
   confirm, and approve the §15 `SIGNATURE_INVALID` erratum.
2. **In-memory staging** — a staged publication survives lock but not a
   helper restart; after a restart, a pending change re-stages at the next
   unlock (§13.3 says "resumed at launch").
3. **Recovery over a LOCKED vault** — only from UNINITIALIZED today.
4. **`rotation_progress` event** — not emitted (rotation is synchronous).
5. **SY-13** — a restored older `vault.db` refuses to unlock (ERROR) rather
   than unlocking read-only until synced.
6. **Production provider origin** — the release allowlist is empty.
7. **Handle normalization Cn approximation** — accept, or ship a
   generated unassigned-code-point table.
8. **Spec errata batch** — class bytes 2/3, §13.3 timeout wording,
   `rotate_recovery_key {suspected_theft}`, tombstone rule, the
   main-supplied handle on the recovery sheet.

Infrastructure the owner controls: hosting + S3, the physical iPhone for
EV-03, a fresh macOS account for the rehearsal, the 370 leftover `ov0*`
Keychain items, later APNs (.p8) and the Chrome extension ID.

## 6. Deviations (recorded, not silent)

In-memory stream staging; recovery only from UNINITIALIZED; an abandoned
recovery returns to UNINITIALIZED; a fork found during apply may lock
(VK consumed) and re-enter COMPROMISED at unlock; ROTATING_KEYS not
reported; added read-only `remote_update_status` op and `vault_open` in
`get_state`; `SheetReason::HandleRetried`; `enroll_confirm` streams a large
bundle as `{session, stream, size}`; recovery-auth class bytes 2/3; the
stricter generation admission and the tombstone direct-parent rule; the
empty release origin allowlist.
