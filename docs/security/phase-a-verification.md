# Phase A Verification Report — Vault Helper Skeleton

Date: 2026-07-21. Spec: `credential-vault-implementation-spec.md` v0.3
(commit `ff6b399`), §18 Phase A. Scope limit honored: no Phase B work, no
vault keys, no credential storage, no HPKE/recovery/backup/extension work,
no real credentials anywhere in this phase.

## What shipped

- `src-tauri/vault-helper/` — new workspace member `source-vault-helper`
  (1,815 lines incl. tests; every file ≤ 350 lines per repo rule):
  - `ipc/framing.rs` — 4-byte big-endian length-prefixed JSON frames,
    64 KiB hard cap, fail-closed on oversize/malformed (§1.4).
  - `ipc/peer_auth.rs` + `ffi/security.rs` — bidirectional SecCode peer
    authentication (§1.4): getpeereid UID gate → LOCAL_PEEREPID pid →
    `SecCodeCopyGuestWithAttributes` → `SecCodeCheckValidityWithErrors`
    against the pinned designated requirement
    `anchor apple generic and certificate leaf[subject.OU] = "9RGW34CMA2"
    and identifier "com.racker.zero(.nm-host| .vault-helper)"`.
    Six-entry-point hand-maintained Security.framework FFI (minimal-dep
    rule §17.4); unsafe blocks carry per-site invariant comments.
  - `ipc/server.rs` — socket at `<vault_dir>/helper.sock` (dir 0700,
    socket 0600), one connection per client class with same-class
    replacement, hello-first enforcement, proto-major mismatch disconnect.
  - `state.rs`, `ops.rs` — UNINITIALIZED/LOCKED subset of the §13.1 state
    machine; `hello`/`get_state`/`lock` only; all other ops → `UNKNOWN_OP`.
  - `main.rs` — lifecycle §1.6: 30-min zero-client idle exit, SIGTERM/SIGINT
    with 5 s client-drain grace.
  - `ipc/client.rs` + `bin/vault-test-client.rs` — client with reverse
    SecCode check before any byte is sent; gate driver binary (not shipped).
- `SourceVaultHelper.app` plumbing: `vault-helper/Info.plist` (LSUIElement,
  `com.racker.zero.vault-helper`, no entitlements), `scripts/build-helper.sh`
  (build → bundle → sign hardened-runtime → `codesign --verify --deep
  --strict` → `-R=` DR test → entitlements-absent assertion).
- `scripts/phase-a-gate.sh` — the 11-check gate below, reproducible via
  `npm run gate:phase-a`.
- `scripts/audit-deps.sh` — gained the §17.4 Phase A checks: helper
  dependency count report + `rsa` absence in the helper graph.

## Gate evidence (all PASS, run 2026-07-21, this machine)

| # | Check (spec §18 Phase A) | Result |
|---|---|---|
| 1 | crate tests | PASS — 27 tests (framing roundtrip/oversize/partial, op dispatch, UNKNOWN_OP refusal, state detection, SecCode requirement compilation, unsigned-self rejection, 7 socket-level E2E) |
| 2 | build + sign + verify bundle | PASS — `codesign --verify --deep --strict`, DR test against the production pin, entitlements absent |
| 3 | signed client accepted; boot UNINITIALIZED | PASS — socket perms 600, vault dir 700 |
| 4 | helper restart leaves state machine at LOCKED | PASS — synthetic `header.json` present → `STATE=locked` after SIGKILL-less restart |
| 5 | lock op honored | PASS — `LOCK_STATE=locked`, idempotent |
| 6 | peer-auth rejects unsigned/ad-hoc clone | PASS — ad-hoc client clone dropped before any response frame |
| 7 | peer-auth rejects wrong identifier (same team) | PASS — same cert, `com.example.impostor` identifier dropped |
| 8 | client rejects ad-hoc helper clone (reverse check) | PASS — `peer code invalid (OSStatus -67050)`, no byte sent |
| 9 | idle exit with zero clients | PASS — 2 s debug override of the 30-min spec value |
| 10 | SIGTERM exits within 5 s grace while client attached | PASS — client still attached at exit |
| 11 | supply chain: audit + vet + helper deps + rsa absence | PASS — audit clean (rsa ignore unchanged, documented), `cargo vet` 759 exempted (no new entries), helper dep count **17** (< 120 gate), no `rsa` in helper graph |

Restart→LOCKED note (check 4): Phase A has no UNLOCKED state at all, so
"restart leaves state machine at LOCKED" is verified as: with a vault
header present, every boot state is LOCKED. The stronger property (VK lost
on crash) has no meaning until Phase C introduces keys.

## Deviation record

1. **Team-OU discovery.** `security find-identity` displays this machine's
   identity as `Apple Development: <developer email> (<cert UID>)`, but
   the parenthetical is the certificate **UID**, not the team OU. The leaf
   certificate's `subject.OU` is `9RGW34CMA2` — the production team pinned
   by the spec. **No debug relaxation was used anywhere in this gate**; the
   DR strings above are the exact spec strings. `build-helper.sh` reads the
   OU from the certificate, not the display name.
2. **`OV0_VAULT_DEV_TEAM_OU` mechanism (present, unused here).** For
   machines without a production-team identity, debug builds may compile a
   different team OU into the DRs via this compile-time env var (signing
   doc rule 3: same-team relaxation, never "any process"). It is
   debug-assertions-gated; release builds ignore it, and
   `build-helper.sh release` refuses a non-production identity outright.
3. **Debug-only env overrides.** `OV0_VAULT_SOCKET_PATH`, `OV0_VAULT_DIR`,
   `OV0_VAULT_IDLE_SECS` exist for gate/development use and compile to the
   spec defaults in release.
4. **`UNKNOWN_OP` code.** §15's user-facing catalog has no
   "not implemented in this phase" code; the skeleton answers every
   out-of-scope op with internal code `UNKNOWN_OP`. Phase B+ maps spec ops
   to §15 codes as they land.
5. **LaunchServices embedding deferred.** Phase A builds/signs/verifies the
   bundle and proves the boundary with a signed test client; embedding
   `SourceVaultHelper.app` under `Contents/Library/vault-helper/` inside
   SOURCE.app and launching via LaunchServices belongs to the first phase
   where the main app consumes the client library (its API is already in
   `ipc/client.rs`). No main-app behavior changed in Phase A.

## Invariants confirmed

- Helper bundle: hardened runtime, **zero entitlements** (no
  disable-library-validation), LSUIElement, identifier
  `com.racker.zero.vault-helper`.
- Cargo.lock grew by 10 lines (the helper package entry only); all helper
  dependencies were already in the vetted baseline.
- Main crate unaffected: `cargo check` clean; `cargo test --lib` 259 pass,
  1 pre-existing failure (`core::storage_tests::test_storage_lifecycle`,
  broken by commit `9c73bfa` before this work; out of scope).
- `npm run check:file-lengths` clean.

## Reproduce

```bash
npm run gate:phase-a     # full 11-check gate
npm run build:helper     # build + sign + verify the bundle only
npm run audit:deps       # audit + vet + helper dependency gates
```

Phase A is complete. Phase B (vault file format, Argon2id, secure panel)
has not been started and requires a separate authorization.
