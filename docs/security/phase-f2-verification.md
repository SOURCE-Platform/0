# Phase F.2 — verification report

Spec: `credential-vault-implementation-spec.md` v0.5 candidate §22
(review loop closed at `1c20f77`). Synthetic data only throughout.
Regression policy (owner decision 2026-09-30): targeted tests per change;
the complete gate run is kept for the Phase J release gate.

## 1. F.2a part 1 — Mac-side rules (`f7c48bc` … `04c711a`)

| Commit | Spec | What changed | Tests |
|---|---|---|---|
| `f7c48bc` | §22.4 (F2-D3) | `change_master_password {mode:"reset"}` proves the Recovery Key against the committed `recovery.wrap` (same VK, same generation) before a new MP; `enroll_confirm` verifies the current MP after presence; UI button says the RK is needed | AU-01 `vault_rk_ops::mp_reset_needs_the_recovery_key`; AU-02 `enrollment::enrollment_needs_the_master_password`; existing enrollment and reset tests updated |
| `dacc8b8` | §22.14 (SY-13) | a store below the Keychain floor unlocks read-only (`behind`); authoring and authority ops return `VAULT_BEHIND`; the floor is never lowered; catching up clears it; `get_state` reports `behind` | SY-13 `vault_failclosed::older_store_unlocks_read_only` (replaces the Phase F refusal test) |
| `f7cb228` | §22.4 | `list_history`, `list_deleted` (metadata only), `restore_revision` (live record → successor; tombstoned record → new record); `delete_item` needs the MP after 10 deletions in 10 minutes, counted in `kv` against a non-decreasing clock | AU-06, AU-07 `history_ops` |
| `09084f8` | §22.11 | fully staged publications persisted in `staging/pending/`; `staged_publication` in `kv` is the authority, deleted by every `pending::add`; validated and resumed at boot while LOCKED; discarded otherwise; forgotten on commit or terminal failure | SG-01…SG-03 `staged_disk` |
| `04c711a` | §22.12 | fork evidence judged against the provider-confirmed registry (own unpublished entries excluded) | CX-01, CX-02, CX-03, CX-05 `compromised_entry` |

Targeted runs (all green): helper `enrollment`, `vault_rk_ops`,
`vault_contracts`, `revocation`, `device_unlock`, `vault_failclosed`,
`state_transitions`, `vault_lifecycle`, `ipc`, `history_ops`,
`staged_disk`, `session_teardown`, `sign_policy`; `vault-tests`
`retry_ipc`, `pending_scenarios`, `handle_retry`, `ipc_transcript`,
`compromised_entry`, `sync_scenarios`, `singleton_scenarios`,
`multi_writer`, `provider_forgery`. File-length audit clean.

**Sequencing notes (recorded, not silent):**

- `rev_sources`, the re-seal/set-aside rule and the provenance cutoff
  (§22.7) are moved from F.2a to F.2c: until peer deliveries exist every
  revision is `own` or `provider`, so the rules have nothing to act on and
  could only be tested by injected rows. They land with `peer_revs_put`.
- `peer_serve` / `peer_status`, `pending_remote.target_device_id` and the
  `peer_endpoint` bundle field wait for the §22.8 wire annex, which is
  reviewed before that code (spec §22.8, §22.17).
- The revoke flow's "set a new MP" branch is not a separate path in the
  Mac implementation: a wrong MP is refused, and a user who no longer
  knows the MP resets it with the RK first (`mode:"reset"`), then revokes.
  This satisfies §22.4 (no MP change without the RK).
- The UI for history and restore is not built yet (ops only).

### 1.1 Milestone review (security + verification, candidate `46773f0`) and fixes

Every finding was checked against the code before a disposition.

| Finding | Disposition and fix |
|---|---|
| SEC-B1 — `prove_mp` never tied `password.wrap` to the vault (a replaced wrap passed every MP gate) | **Accepted.** `prove_mp(store, vk, mp)` requires the unwrapped key to equal the resident VK at the header generation (shared `bound_to`, as `prove_rk`); the rotation's `Reseal` checks the same against the retiring VK. Tests `au_swapped_wrap` (change MP, rotate RK, bulk delete with a planted wrap). Spec §22.4 erratum. |
| SEC-B2 / VER-B2 — `behind` compared a local write counter; syncing could not clear it | **Accepted.** New `vault::floor`: the Keychain floor is bound to `vault_id` and records the local generation and the accepted provider state (generation, `state_commit`, confirmed registry head); `behind` is judged on the opened store; a verified provider exchange that reaches the floor (including "nothing newer") catches up by raising the store's local generation — the floor never goes down. Setup and recovery reset it for their vault. Spec §22.14 erratum. |
| SEC-B3 — while behind, the restored registry was the trust anchor | **Accepted.** While behind, `backup_apply` requires the served registry to contain the floor's registry head (else `SIGNATURE_INVALID`, nothing adopted) and offers never enter COMPROMISED. |
| VER-B1 — the app never synced a behind vault | **Accepted.** `Flows::backup_now` answers `VAULT_BEHIND` from `backup_prepare` with a sync; the worker does not sync twice. |
| SEC-I1 — rollback check read a cached header | **Accepted** (the opened store is used). |
| SEC-I2 / VER-O6 — a forward clock jump reset the deletion window | **Accepted.** Ages grow only by monotonic time the helper observed; no credit across a restart. |
| SEC-I3 / VER-I7 — no §15 backoff on the new MP/RK gates | **Accepted** (enrollment, bulk deletion, RK reset). |
| VER-I1 — SG-01 did not show completion | **Accepted.** `vault-tests/staged_resume`: resumed after a restart, committed while LOCKED, pending cleared, staging gone. |
| VER-I2 / SEC-O2 — staging survived COMPROMISED | **Accepted.** Forgotten on entry and on the adoption-failure path; never resumed with evidence on file. |
| VER-I3 — CX-05 did not cover the registry-entry race | **Accepted.** `cx05_racing_revocation_entry_is_never_adopted`. |
| VER-I4 — AU-07 single-device only | **Accepted.** `vault-tests/restore_converges`. |
| VER-I5 — `prove_rk`'s VK check untested | **Accepted.** `a_recovery_wrap_over_another_key_does_not_reset_the_mp`. |
| VER-I6 — SG-03 canaries too narrow | **Accepted** (VK raw and hex added). |
| VER-I8 — AU-03 untested; no erratum | **Accepted.** `revocation_needs_the_current_master_password`; §22.4 erratum (no set-new-MP branch inside revoke). |
| SEC-O1 / VER-O2 — blob list not bound; updates checked as a subset | **Accepted.** The kv record binds a hash of the blob list; files are size-capped before reading; the body's recovery-auth updates must equal the pending change's (none for a routine publication). |
| VER-O3 — stale record deletion untested | **Accepted** (SG-02 asserts the record is gone). The files themselves are removed at the next persist or boot. |
| VER-O7 — deleted item's title picked by timestamp | **Accepted** (the tombstone's parent is used; this also fixed a same-second flake). |
| VER-O8 — delete without activation; restore not in `BACKUP_AFTER` | **Accepted.** Both use `call_with_panel`; `restore_revision` triggers a backup. The UI does not yet show `behind` before an op fails (open, UI). |
| VER-O10 — floor not per vault; recovery never wrote it | **Accepted** (floor bound to `vault_id`; recovery resets it). |
| SEC-O3 — plaintext RK/VK copies in wrap/bip39 intermediates not zeroized | **Deferred** (pre-existing; recorded for the Phase J memory audit). |
| SEC-O4 — resolving a conflict to a tombstone is not counted | **Deferred** (a conflict resolution is a deliberate per-record act with presence; recorded). |
| VER-O1 — behind gating is a denylist | **Deferred** to F.2c, when `save_*` and peer ops are routed; recorded. |
| VER-O9 — persist runs under the core lock and writes every blob | **Recorded**; acceptable for synthetic-size vaults, revisit with real-size measurements. |
| VER-O11 — `confirmed_registry` fallback; RC-05 wording | **Recorded.** |

Targeted runs after the fixes (all green): helper `au_swapped_wrap`,
`revocation`, `staged_disk`, `history_ops`, `vault_failclosed`,
`vault_rk_ops`, `vault_lifecycle`, `enrollment`, `device_unlock`,
`state_transitions`, `vault_contracts`, `vault_rotation`,
`store_recovery`, `session_teardown`, `sign_policy`; `vault-tests`
`behind_sync` (SY-13 through a real provider), `staged_resume`,
`restore_converges`, `compromised_entry`, `total_loss`,
`recovery_scenarios`, `singleton_scenarios`, `pending_scenarios`,
`handle_retry`, `multi_writer`, `retry_ipc`, `ipc_backup`,
`publish_sync`, `sync_scenarios`, `provider_forgery`, `catchup_fixture`,
`ipc_transcript`; `vault-coordinator`; `cargo check -p SOURCE`.

**Dispositions omitted above:** VER-O4 (AU-01's "no RK typed" case also
passes without the RK check) — **accepted as noted**; the RK binding is
now proven by `a_recovery_wrap_over_another_key_does_not_reset_the_mp`.
VER-O5 (CX `not_compromised()` cannot fail at the fetch layer; no
"invalid signature by an active id" case) — **recorded**; the dispatch-
level COMPROMISED paths are covered by `behind_sync` and
`cx05_racing_revocation_entry_is_never_adopted`.

### 1.2 Bounded re-review of the fixes (`5133dc3`) and closure

Both reviewers closed most findings and found new ones. The security
blockers were fixed (they are security-critical, so the loop does not
close on them unfixed); everything else is fixed or recorded.

| Finding | Disposition and fix |
|---|---|
| VER-B3 — unlocking **through** a planted `password.wrap` (or `recovery.wrap`, or a device envelope sealed to the public agreement key) made the planted key resident; every later gate compared against it (reproduced: a device enrolled) | **Accepted.** `vault::vk_commit`: a Secure-Enclave-signed commitment to the vault key (verified under the public key the SE reports, not the device file) must verify on every unlock path before a key becomes resident; the helper signs it only for keys it already trusts (setup, its own rotations, verified adoption, recovery); a failed signature is retried while the key stays resident. Test `unlocking_through_a_planted_key_is_refused` (MP path and envelope path). Spec §22.4 erratum, §2.9 row. |
| SEC-B1 (new) — catch-up silently dropped a security change committed locally but unpublished | **Accepted.** The floor records the unpublished change (ops + base); a catch-up on a store that neither carries it nor shows it landed records `lost_change`, shown as a warning banner; redoing any security change clears it. Test `sy13_a_lost_unpublished_security_change_is_reported`. Spec §22.14. |
| SEC-I1 / VER-I13 — apply could enter COMPROMISED while behind | **Accepted.** While behind, a fork from apply is `SIGNATURE_INVALID`, nothing recorded. |
| SEC-I2 — `raise_generation` not crash-safe | **Accepted.** A `raise_target` is recorded first; `open` rolls forward to it (several generations, same registry head); cleared afterwards. |
| SEC-I3 / VER-I11 — backoff missing at revoke, RK rotation, handle retry | **Accepted** (`recovery_ops::backoff` at every MP gate). |
| VER-I9 — the behind offer arm untested | **Accepted.** `sy13_behind_never_enters_compromised_on_an_offer`. |
| VER-I10 — catch-up persistence across relock untested | **Accepted** (both SY-13 catch-up tests relock). |
| VER-I12 — spec still prescribed a set-new-MP branch in revoke | **Accepted.** §1.5 row, §11.4, §22.10 scope row, PV-01 and AU-03 now describe the RK reset first. |
| VER-I14 — dispositions for VER-O4/O5 | **Accepted** (above). |
| VER-O12 — catch-up after an unverified offer | **Accepted.** Only after `up_to_date`, a completed apply, or a commit. |
| SEC-O1 (new) — edited files could fake "not behind" | **Accepted.** At unlock: a commitment mismatch at the same provider generation, or a local registry without the floor's head, is behind. |
| SEC-O2 (new) — staged-file read followed symlinks/FIFOs | **Accepted** (regular files only, bounded read). |
| VER-O16 — worker reported "ok" while still behind | **Accepted** (status `behind` with the §22.14 copy). |
| VER-O17 — Deleted Items refetched on every parent render | **Accepted** (stable error callback). The annex and the Deleted Items UI get their own reviews (annex next; UI in the F.2a closing pass). |
| VER-O13, VER-O15, VER-O18 — tests missing for rotation-Reseal binding, exact update equality, enroll/retry-handle gates with a planted wrap, other-vault floor and recovery reset | **Recorded.** The shared `bound_to` / `prove_mp_resident` path is tested; the remaining direct tests are listed for the F.2a closing pass. |
| SEC-O3 (new) — recovery resets a same-vault floor | **Recorded** (recovery revokes every prior device; surfacing the older served state in FR-01 is an option for later). |

### 1.3 Focused security check of the vault-key commitment (`30539be`)

A narrow security check of the two security-critical fixes above.

| Finding | Disposition and fix |
|---|---|
| SEC-B1 — key change and commitment not atomic: a crash or error between them, or a lock with a pending retry, locked the user out permanently (every unlock `WRONG_CREDENTIAL`) | **Accepted.** The commitment is staged in the same journal as the key: `EnvelopePlan.commit_tag` stages it with every rotation (revoke, RK replacement, handle retry, recovery); adoption stages it with the adopted key and aborts if it cannot be signed; `lock()` makes a last attempt at a pending retry. Test `a_rotation_commits_its_key_commitment` (library rotation, no op-level retry). |
| SEC-B2 — a lost security change could still go unreported (pending row deleted, edited copy, MP change/enrollment never recorded) | **Accepted.** `floor::raise` checks the recorded change at every unlock and after every authority/provider op; "landed" requires the produced base **and** a newly accepted provider state; every authority op records its change at once; the lost list lives in the Keychain floor and is mirrored to `vault.db`. Test `a_vanished_pending_change_is_reported_at_unlock`. |
| SEC-B3 — the Secure Enclave key blobs are login-keychain items a thief with the login password can extract and use from any process | **Confirmed** with a throwaway key (a second process signed with the blob). **Pre-existing (Phase E); escalated to the owner** — the fix (data-protection keychain under a signed access group, or a biometry-bound agreement key) needs an Apple signing decision. See §2. |
| SEC-B4 — before the first provider state the registry was unanchored | **Accepted.** `floor::reset` anchors on the genesis entry. Test `a_rewritten_registry_before_the_first_commit_is_not_trusted`. |
| SEC-I1 — the envelope path could be steered into the read-only "no identity" fallback | **Accepted.** The envelope path refuses instead of falling back. |
| SEC-I2 — any new pending change cleared a lost-change warning | **Accepted.** Only the redone operation's warning clears. |

## 2. Owner checkpoint (open)

**Secure Enclave keys on the Mac (SEC-B3).** The helper stores each
Secure Enclave key's device-bound blob as a generic password in the login
keychain. Anyone who can approve a login-keychain prompt (a thief who
knows the Mac login password) can copy the blob and use the key from any
program on that Mac — open the device envelope (the vault key) and sign as
the device. That defeats F2-D3 on the Mac. Options:

1. **Data-protection keychain under the helper's own access group** —
   other programs cannot read the items at all, password or not. Needs
   the helper signed with a provisioning profile carrying
   `keychain-access-groups` (Apple Developer Program for a long-lived
   profile; free profiles expire every 7 days). Recommended.
2. **Biometry-bound agreement key on the Mac** (as on the iPhone) — the
   blob is useless without your fingerprint; Macs without Touch ID unlock
   with the master password instead. Changes approved unlock behaviour.
3. Both.

**Owner decision (2026-10-01): "Require Touch ID".** The Mac's agreement
key (the one that opens the device envelope, i.e. the vault key) becomes
biometry-bound (`.biometryCurrentSet`), as on the iPhone; a copied blob
is useless without the owner's fingerprint, and the login password alone
no longer opens the vault (Macs without Touch ID, or with the lid closed,
unlock with the master password). **Stated residual:** the Mac's
*signing* key cannot be biometry-bound (background provider signing), so
a thief with the login password can still copy it and sign as the Mac —
not read the vault, but publish disruptive states or enroll a device that
would receive *future* changes until the Mac is revoked from the iPhone.
The data-protection keychain (paid Apple Developer Program) closes this;
recommended to revisit before the Phase J first-credential gate.

**Owner decision (2026-10-02): "Password every time"** (review SEC-B2 —
a Mac with no Touch ID sensor or no fingerprint enrolled, where the
Enclave cannot make the biometry-bound key). Such a Mac never gets a plain
key: its agreement public key is made with the private half dropped at
once (`agree_discarded`), and every unlock goes straight to the master
password. Rejected: refusing such Macs (recovery onto a Mac mini or Mac
Studio would be impossible); a plain key (the login password would open
the vault). Condition: the master-password adoption path (F.2d) lands
before any other device can rotate.

## 3. Peer wire annex review (spec §22.8) — closed

Revision 1 (`d9a2f13`) review: SPEC-B1 (peer-token scope), SPEC-B2
(single revocation target), SPEC-I1…I11, SPEC-O1…O8 — all **accepted**
and fixed in revision 2 (`6260127`): separate peer-token store bound to
the device id, header only; `target_device_ids`; 24 KiB inline IPC;
signed status for helper caps; whole-bucket paging, unavailable entries,
byte-range objects; persisted `state_get` body; byte-exact encoding;
session re-check and single use; discovery hints; token survives
revocation; behind Macs serve hello/status only; PW tests in §22.16/§19.

Bounded re-review of revision 2: **no blockers**. SPEC-I1…I9 and
SPEC-O1…O8 **accepted** and fixed in revision 3: `peer_serve_begin` may
answer with a refusal or a signed status (with `size`); peer-session
streams allowed in the serving states (§1.5, §13.2); the behind check
also gates a LOCKED Mac; `awaiting_redo` per revocation target (§11.3.2);
offset always present; canonical Kahn order, strictly enforced; servable
subgraph heads; `peer_endpoint` typed again; one body per operation in
XV-PEER; PW-03 split, PW-11/12 added. The review loop for the annex is
closed; F.2c code may start against revision 3.

**Implemented (Touch ID decision):** `ov0_se_key_create_bio` creates the
agreement key with `.biometryCurrentSet`; `ov0_hpke_open_se_auth` opens
envelopes with a reason string (Touch ID is asked by the Enclave; no
biometry → `DEVICE_NOT_AUTHORIZED` → master-password fallback; declined →
`PRESENCE_DENIED`); the unlock path skips its own LA check for such a
key; adoption asks with its own reason. Test keys stay non-biometric.
Targeted runs green: `device_identity`, `device_unlock`, `enrollment`,
`revocation`, `au_swapped_wrap`, `commitment_journal`. **Open (needs the
owner's finger):** a manual check on the real Mac with a signed helper —
create a vault, unlock with Touch ID, cancel (no password fallback),
lid closed (master-password fallback).

## 4. F.2c — peer sync, Mac side (in progress)

| Commit | Spec | What | Tests |
|---|---|---|---|
| `ddc2cad` | §22.8, annex A.1–A.3 | `vault-proto::peer`: envelopes with their own prefixes and strict decoding; operation bodies; heads digest | `vault-proto/tests/peer_wire.rs` (PW-02 rules, zero/empty encodings, prefix separation, digest) |
| `54a872d` | §22.7, annex A.4 | schema v3 (`rev_sources`, `peer_replay`; v2 migrates in place); sources recorded for own, provider, join, recovery and our own commits | `storage::db` unit tests (migration) |
| `574a5ba` | annex A.4 | `target_device_ids` / `awaiting_redo_targets`: revocations tracked per target | PW-10 `two_adopted_away_revocations_are_tracked_per_target` |
| `1f0d0fc`, `8cf7a51` | §22.8 | receiver order, unsigned refusals, persisted replay cache, rate after the signature, who may speak (incl. pending targets), signed responses, `peer_status` | `peer_serve.rs` (PA-01…04, PA-08, PW-07, PW-09 rate only); PA-05 is requester-side (F.2c phone) |
| `ce367eb` | §22.7, annex A.3.1/3/4 | servable subgraph (freshness rule), hello digest, whole-bucket heads, canonical closures, unavailable reasons, behind/COMPROMISED gate | `peer_exchange.rs` (PS-12, PW-11, status 4) |
| `c9d9090` | §22.7, annex A.3.5 | `peer_revs_put` on an unlocked Mac: canonical order and closure or status 4, waiting rules, AEAD-open before admit, peer provenance | `a_phones_revisions_are_opened_before_admission` (PS-01/04) |
| `6c899ce` | §22.7 | revoker cutoff (`refused_peer`), cutoff on accepting a revocation, set-aside of peer-only revisions before every rotation and adoption with own descendants re-authored | `peer_cutoff.rs` (PS-08, PS-10) |
| `193301c` | annex A.3.5 | LOCKED inbox (bounded; admitted at unlock; purged on cutoff) | `a_locked_mac_holds_puts_until_unlock` (PS-11) |
| `e6f9842` | §1.5, annex A.2.2 | `peer_serve` IPC op; LOCKED floor check; freshness from the last verified provider exchange; large responses as stream sessions | `peer_ipc.rs` (PW-01; PW-08 lock abort only) |
| `fe2f65a` | annex A.2.1, A.4 | main app `POST /v1/vault/peer` (private networks, header-only peer token from its own store bound to the sender, 413 before the helper); `peer_endpoint` in the enrollment bundle | `only_private_networks_reach_the_peer_route`; since §5, `tokens_are_scoped_bound_to_their_device_and_refused_bare` (PW-06, PW-12) |
| `6990a76` | annex A.3.2 | `peer_state` from the kept verified `state_get` body; objects mode bound to the accepted state (no blobs held) | `vault-tests/peer_state.rs` (PW-04 state mode; objects mode deferred, §5) |
| `c99db49` | annex A.2.2 | `peer_serve_begin` and streamed request bodies; who-may-speak re-checked at completion; single-use sessions; main uses it for bodies over 24 KiB | `a_large_put_streams_in_after_the_envelope_checks` (PW-03) |

**Deferred out of the Mac-side F.2c batch (recorded):** "publish first"
before a self-started rotation (best effort, coordinator — set-asides
remain correct without it, only rarer with it); the XV-PEER vector file
and the CryptoKit-only Swift target (with F.2b, where the Swift side
exists); a "Forget this device" UI for peer tokens.

**Superseded list (kept for history):** Still to do in F.2c (Mac side): the LOCKED inbox (A.3.5); `peer_state`
with the persisted verified `state_get` body (A.3.2); "publish first"
before a revocation (coordinator); IPC wiring (`peer_serve`,
`peer_serve_begin`, peer-session streams, `get_state`/`fresh` tracking);
the main app's route, peer-token store and `peer_endpoint` in the
enrollment bundle; XV-PEER vectors. Then the milestone review (with the
Touch ID change).

## 5. F.2c Mac side — milestone review (candidate `c779e8b`) and fixes

Reviewers: `security-reviewer` and `verification-reviewer`, fresh
contexts, on `git diff 57527cd c779e8b` (the Touch ID change included).
Every finding was checked against the code before a disposition; two were
reproduced first (the migration comparison with the sqlite3 CLI; the
`device.json` flag with a scratch probe).

**Hardware facts checked on this Mac (2026-10-02, throwaway keys only):**
a `.biometryCurrentSet` key is created with Touch ID enrolled **even with
the lid closed** (Touch ID unavailable at that moment); with interaction
forbidden, such a key refuses a key agreement while a plain key does not —
which is how the helper now asks the Enclave whether a key is
biometry-bound. With the lid closed the Enclave's refusal is an LAError
`-4` ("not available in closed clamshell mode"), not
`biometryNotAvailable`; the bridge now maps every refusal other than the
user's own cancel/failure to "no biometry" (master-password fallback). A
Mac **without** a sensor or with no fingerprint enrolled was not
available to test; that case is an owner question (SEC-B2 below).

| Finding | Disposition | Fix / where | Test |
|---|---|---|---|
| SEC-B1 / VER-B4 set-aside outside flip and journal | **Accepted, fixed.** The set-aside (deletions, re-authoring) is one SQLite transaction with a stamped flip target, then one flip — the vault is consistent whether or not a rotation follows. The revoker's cutoff (`refused_peer`, inbox purge) is computed before the set-aside and written **inside the rotation's staged DB** (`RemoteChange.cutoff`), so a failed rotation refuses nothing | `storage/set_aside.rs`, `sync/change.rs`, `vault/revoke_core.rs` | `a_set_aside_on_its_own_leaves_an_openable_vault`; PS-08/PS-10 still green |
| SEC-B2 / VER-I9 no-Touch-ID Mac | **Owner decision 2026-10-02 (§2), implemented in §5.1.** Was open at this review. Creation with the lid closed was confirmed to work; a sensor-less Mac or one with no fingerprint enrolled could not be tested here. Error mapping fixed: clamshell, lockout, disconnected and non-LA refusals → `DEVICE_NOT_AUTHORIZED` (MP offered); only the user's cancel/failure → `PRESENCE_DENIED`; a malformed envelope → `INTEGRITY_FAILURE` | `Bridge.swift` | — |
| VER-B1 migration marks own revisions `provider` | **Accepted, fixed.** TEXT vs BLOB comparison: `CAST(author_device AS BLOB)`; the test now drops all four v3 tables and stores `author_device` as production does | `storage/db.rs` | `a_v2_database_migrates_to_v3` (was red, now green) |
| VER-B2 / SEC-I4 `device.json` flag skips presence | **Accepted, fixed.** Whether the agreement key needs the user is asked of the Enclave (`ov0_se_key_needs_user`: one key agreement with interaction forbidden); the file flag is a record only. Any failure to ask keeps the presence check. Identities from before the Touch ID decision therefore keep the LA check; they are synthetic test vaults only (no real vault exists yet) — re-creating the identity before the first real credential is a Phase J gate item, not a migration | `device/se.rs`, `device/identity.rs` | `a_planted_biometry_flag_does_not_skip_presence` |
| VER-B3 bridge over 200 lines | **Accepted, fixed.** The dead `ov0_hpke_open_se` moved to the PoC shim (only the PoC used it); creation paths shared; 200 lines | `Bridge.swift`, `poc/hpke-se/swift/PocShim.swift` | gate line count |
| SEC-I1 / VER-I1 caps | **Accepted, fixed.** Exchange cap: 64 MiB of answer bodies per sender in any 10 minutes → signed status 2. Per-peer waiting quota: a peer can no longer leave anything in `pending_revs` — a batch member resting on a waiting, refused or locally pending revision waits (counted, not kept) and anything still pending after the batch is removed and counted as waiting | `peer/verify.rs`, `peer/ops.rs`, `peer/admit.rs` | `the_exchange_cap_stops_at_64_mib_per_sender` |
| VER-I2 waiting revisions not held | **Accepted as a spec alignment.** Holding them would need the registry and key checks repeated at every retry and is the storage the quota is meant to bound; the phone re-offers them at the next exchange. §22.7 "Waiting" amended accordingly | spec §22.7 | — |
| SEC-I2 / VER-O1 provenance gaps | **Accepted, fixed.** (a) The non-revoker purge re-authors by `own` provenance, never by the author field; (b) a peer's revisions promoted within its batch get the peer as source; an empty source set counts as unconfirmed (set aside) | `storage/revoked.rs`, `peer/admit.rs`, `storage/set_aside.rs` | `a_forged_mac_author_is_not_an_own_revision` |
| SEC-I3 / VER-I6 tombstones dropped; parents | **Accepted, fixed.** Own tombstones are re-authored as tombstones (also on the revoked-author path); re-authored parents are the nearest remaining or re-authored ancestors | `storage/set_aside.rs`, `store_records.rs`, `sync/apply.rs` | `a_rotation_keeps_this_macs_deletions` |
| SEC-I5 prompt under the mutex; no MP adoption | **Mutex part accepted, fixed:** `backup_apply` opens the envelope it will need before taking the core mutex, and the apply uses that result for exactly that file. **MP-based adoption: deferred to F.2d** (adopting another device's key change becomes live there; spec text and test with it) | `vault/adopt_prompt.rs`, `vault/sync_ops.rs` | existing adoption suites |
| VER-I3 wire deviations | **Accepted, fixed:** unknown operation → signed status 4 (`PeerOp::Unknown`); a body on hello/status → status 4; > 2,000 revisions → status 2 (both paths); a record not held → unavailable reason 2; the LOCKED inbox checks decoding, canonical order and closure; `host_hints` are private IP literals | `vault-proto/peer`, `peer/ops.rs`, `serve_revs.rs`, `inbox.rs`, main `session.rs` | `malformed_and_oversized_requests_get_their_signed_status`, `peer_wire`, `host_hints_are_private_ip_literals` |
| VER-I4 sessions | **Accepted, fixed:** up to two peer sessions (in + out); a third begin, or a large answer with both taken, gets status 2; idle 60 s; `session_close` closes them | `vault/provider_ops.rs`, `vault/peer_serve.rs` | since §5.1, `at_most_two_peer_sessions_and_close_frees_one` (idle expiry untested) |
| VER-I5 objects mode serves nothing | **Deferred to the F.2c phone side** (added to the list below). The Mac answers honestly (reason 2) and the phone fetches from the provider; serving byte ranges needs the Mac to keep the registry and index blobs | — | — |
| VER-I7 freshness across sleep and lock | **Accepted, fixed:** cleared at every lock; fresh only while both the monotonic and the wall clock say < 15 min | `vault/peer_serve.rs`, `vault/mod.rs` | since §5.1, `freshness_ends_at_lock` (wall-clock check untested) |
| VER-I8 test gaps | **Accepted, mostly fixed:** who may speak with a pending target; Kahn tie-break and served heads pinned; heads digest pinned; inbox bounds, validation and purge; main token scope, binding, 401/413/403. **Open:** the LOCKED behind evaluation through `peer_serve` (PW-11) and provider provenance at publish/apply are covered only indirectly | `peer_hardening.rs`, `peer_wire.rs`, main route tests | as listed |
| VER-I10 discard count; reason code | **Accepted, fixed:** `list_devices` reports per device the changes only it delivered (`unpublished`), shown beside "Remove" in Settings; cutoffs count as `REFUSED_PROVENANCE` (6) | `vault/devices.rs`, `DevicesPanel.tsx`, `rev.rs` | — |
| VER-I11 deferred list | **Accepted:** Rust-side pins added now (Kahn, digest); the deferred list below is complete | — | — |
| VER-O2 heads rule | **Fixed:** served heads use the store's tombstone rule | `peer/graph.rs` | pinned test |
| SEC-O2 / VER-O3 unvalidated JSON forwarded | **Fixed:** the kept body is re-encoded from the parsed fields | `sync/remote.rs` (`canonical`) | `peer_state` |
| SEC-O3 LOCKED COMPROMISED stash | **Fixed:** persisted COMPROMISED evidence gates a LOCKED Mac too | `vault/peer_serve.rs` | — |
| VER-O4 main route | **Fixed:** axum's own refusals lose their body; loopback removed from "private". Old tokens on re-enrollment: main cannot tell a re-enrolled phone from a new one — deferred with "Forget this device" (the phone presents its old token when re-pairing, F.2b) | main `server.rs`, `routes_vault_peer.rs` | `axums_own_refusals_lose_their_body` |
| SEC-O1 LOCKED request opens the store | **Deferred (optional hardening):** bounded by the 60/min rate; a cached read-only open is an F.2d performance item | — | — |
| VER-O5 report hygiene | **Fixed** (table rows, overclaims) | this report | — |
| VER-O6 header refresh / test hook | **Partly:** `reset_rate` stays `#[doc(hidden)]` (it only forgets load counters); the header refresh after a peer put is cosmetic and left for F.2d | — | — |

**Deferred (complete list after this review):** "publish first" before a
self-started rotation; the XV-PEER vector file and the CryptoKit-only Swift
target (F.2b); "Forget this device" and old-token removal at re-pairing;
objects-mode byte ranges (F.2c phone side); MP-based adoption (F.2d);
cached LOCKED store open (F.2d); the LOCKED-behind and provider-provenance
tests named above.

**Targeted runs after the fixes:** `peer_hardening` (8), `device_unlock`
(7), `peer_wire` (6), `storage::db` (4), main `routes_vault_peer` (4);
helper `peer_cutoff`, `peer_exchange`, `peer_serve`, `peer_ipc`,
`revocation`, `vault_rotation`, `vault_rk_ops`, `history_ops`,
`staged_disk`, `commitment_journal`, `enrollment`, `au_swapped_wrap`; and
vault-tests `peer_state`, `pending_scenarios`, `behind_sync`,
`restore_converges`, `compromised_entry`, `staged_resume` — 18 suites,
all green. The PoC builds against the trimmed bridge. Next: one bounded
re-review of these fixes.

### 5.1 Bounded re-review of the fixes (`37e945c`) and closure

The same two reviewers, fresh contexts, on `git diff c779e8b 37e945c`.
Both confirmed every original finding closed in code (SEC-B2 aside), and
both found the same new blocker in a fix.

| Finding | Disposition | Fix / where | Test |
|---|---|---|---|
| SEC-B3 / VER-B5 `nearest` exponential on many-path peer graphs (a compromised phone could hang its own revocation) | **Accepted, fixed.** Nearest remaining ancestors computed once per leaving revision in parents-first order and remembered; no recursion | `storage/set_aside.rs` | `a_set_aside_over_a_many_path_peer_graph_finishes` (40 diamond levels, 2^40 paths) |
| SEC-I6 / VER-I12 failed rotation refusing nothing untested; flip test not discriminating | **Accepted, tests added** | — | `a_failed_revocation_rotation_refuses_nothing`, `a_set_aside_with_nothing_to_reauthor_still_flips` |
| VER-I13 exchange cap test exercised only the counter | **Accepted, test added** through the serving path, constant pinned | — | `the_serving_path_answers_status_2_at_the_exchange_cap` |
| VER-I14 overclaimed / missing tests | **Accepted:** sessions, freshness at lock, nothing left waiting now tested; report rows corrected | — | `at_most_two_peer_sessions_and_close_frees_one`, `freshness_ends_at_lock`, `a_peer_leaves_nothing_waiting_here` |
| VER-I15 open/deferred list incomplete | **Accepted:** owner decision recorded (§2); list below completed | this report | — |
| VER-I16 "byte for byte" vs re-encoding | **Accepted:** annex A.3.2 and §22.5 now specify the re-encoding (fixed key order, compact) | annex, spec | `peer_state` |
| VER-I17 Touch ID asked before the served state is authenticated | **Accepted, fixed:** the early open only for a state signed by a device of the confirmed registry; otherwise the apply verifies fully before any prompt | `vault/adopt_prompt.rs` | — |
| SEC-O4 / VER-O7 any failure read as "needs the user" | **Fixed:** 1 only for an LAError refusal (checked on hardware with throwaway keys: biometry-bound → LAError, plain → silent); other failures keep the presence check | `Bridge.swift` (200 lines) | DU-02b |
| SEC-O5 cut-off revisions servable | **Fixed:** never served | `peer/graph.rs` | — |
| VER-O8 unknown op on a behind Mac got status 1 | **Fixed:** status 4 before the gate | `peer/ops.rs` | — |
| VER-O9 re-arriving cut-off revision counted as reason 2 | **Fixed:** `REFUSED_PROVENANCE` | `storage/merge.rs` | — |
| VER-O10 cap charged before side effects; VER-O11 trivially passing host-hint test, merge-rejected child counted as waiting | **Noted, not changed:** the charge only affects the next answer (the put's own counts are already committed and the phone re-asks); the host-hint test checks form only; a merge-rejected child is re-offered and refused again — no state grows | — | — |
| SEC-B2 owner decision "password every time" | **Implemented:** discarded agreement key when the bio key cannot be made; unlock straight to the MP, no prompt first; never a plain key | `device/identity.rs`, `vault/device_unlock.rs`, spec §2.7 | `a_mac_without_touch_id_unlocks_with_the_master_password_only` |

**Deferred (complete):** "publish first"; XV-PEER file and CryptoKit-only
Swift target (F.2b); "Forget this device" and old-token removal at
re-pairing (F.2b); objects-mode byte ranges (F.2c phone side);
**master-password adoption (F.2d — must land before any other device can
rotate)**; cached LOCKED store open (F.2d); header refresh and
items-changed event after a peer put (F.2d); tests still missing: the
LOCKED-behind evaluation through `peer_serve` (PW-11), provider
provenance at publish/apply, peer session idle expiry, the freshness
wall-clock check, SEC-O3; legacy non-biometric test identities are
re-created before the first real credential (Phase J gate item).

**Closure.** No security-critical or spec-blocking finding remains; per
the bounded loop the checkpoint closes with the dispositions above (the
fix for the one new blocker is small, local and covered by a test that
hangs without it).

