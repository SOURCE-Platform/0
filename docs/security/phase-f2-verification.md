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
