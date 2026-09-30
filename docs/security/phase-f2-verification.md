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
