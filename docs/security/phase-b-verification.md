# Phase B Verification Report — Cryptographic Core

Date: 2026-09-18. Spec: `credential-vault-implementation-spec.md` v0.3
(commit `ff6b399`), §18 Phase B. Scope limit honored: no Phase C+ work —
no UI/panels, no extension, no backup/enrollment/HPKE/iPhone flows, no
import, no real credentials (every test and vector uses fixed synthetic
constants), and the Phase A IPC boundary is unchanged (no new ops, no
bulk-secret API).

## What shipped

All inside the `source-vault-helper` workspace member
(`src-tauri/vault-helper/`), every file ≤ 350 lines per repo rule:

- `crypto/secret.rs` — `SecretBytes<N>` / `SecretVec`: zeroize-on-drop,
  best-effort `mlock`, redacted `Debug`; `disable_core_dumps()`
  (setrlimit RLIMIT_CORE 0) wired into `main.rs` before anything else
  (§2.11); OS CSPRNG via `getrandom` 0.4.
- `crypto/kdf.rs` — Argon2id v1 tuple `m=64 MiB, t=3, p=1, outlen=32`
  (§2.3) + the `kdf_version` downgrade guard (CR-09).
- `crypto/hkdf.rs` — HKDF-SHA-256 with all eleven §2.9 info strings and
  named derivations (wrap keys MP/RK, record/meta subkeys, recovery auth,
  backup credentials MP/RK, locators, import fingerprint).
- `crypto/tlv.rs` — strict canonical TLV codec (§4.2): ascending tags,
  minimal big-endian integers, NFC strings, terminator, no trailing
  bytes; decode→re-encode is byte-identical or the input is rejected.
- `crypto/wrap.rs` — wrap constructions (§2.5): recovery wrap payload and
  device envelope payload TLV codecs (CR-12 presence split), JSON wrap
  files, seal/open with AAD `ov0/wrap ‖ vault_id ‖ kind`.
- `crypto/record.rs` — per-record subkey encryption with record/meta AAD
  (§2.6).
- `crypto/rotate.rs` — VK-generation rotation re-seal primitives
  (records + both wraps), exercised by CR-08.
- `crypto/bip39.rs` — in-house BIP-39 English codec (§2.4): vendored
  2048-word list (SHA-256 `2f5eed53…`, provenance recorded), 24-word RK
  encode/decode, normalization, checksum. Verified against the official
  reference vectors.
- `crypto/ecdsa.rs` — P-256 verification (§2.7): 65-byte uncompressed key
  parsing with on-curve check (§4.4 rule 8), low-S canonical form
  (high-S rejected pre-verification), prehashed verify; dev-only
  deterministic signing used solely by tests/vectors.
- `crypto/registry.rs` — device-registry entry model (§4.3): tags
  0x01–0x13, per-kind presence rules, entry hash, sign input, and the
  §4.5 recovery-epoch HMAC proof (`ov0/registry/recovery/v1`).
- `crypto/hex.rs` — in-house hex (avoids a `hex` crate).
- `crypto/vectors.rs` + `src/bin/gen_vectors.rs` — deterministic vector
  generator (§16.8) with `--check` freshness mode for CI; committed
  outputs under `tests/vectors/`.
- `src/bin/kdf_bench.rs` — Argon2id calibration benchmark (§2.3); output
  committed as `docs/security/argon2-calibration.md`.
- `fuzz/` — cargo-fuzz target `tlv_decode` (§16.1 smoke): strict TLV
  reader + registry-entry decoder with a canonical-round-trip oracle.
- `scripts/phase-b-gate.sh` — the 6-check gate below, reproducible via
  `npm run gate:phase-b`.

## Test evidence

76 helper tests, all green (42 lib units + 34 integration):

| ID | Where | Result |
|---|---|---|
| CR-01 unwrap with correct MP | `cr_wrap.rs` | PASS — v1 tuple at full cost, roundtrip |
| CR-02 unwrap with correct RK | `cr_wrap.rs` | PASS |
| CR-03 wrong MP | `cr_wrap.rs` | PASS — IntegrityFailure, no oracle |
| CR-04 wrong RK | `cr_wrap.rs` | PASS — valid-checksum decoy words fail identically |
| CR-05 1-byte ciphertext tamper | `cr_record.rs` | PASS — open fails |
| CR-06 AAD tamper matrix | `cr_record.rs` | PASS — vault/record/context swaps all fail |
| CR-07 2²⁰ nonce uniqueness audit | `cr_record.rs` | PASS — 1,048,576 production-path seals, no (key, nonce) repeat |
| CR-08 full VK rotation | `cr_record.rs` | PASS — 3 records + both wraps re-sealed, old generation dead |
| CR-09 KDF downgrade refusal | `cr_wrap.rs` | PASS — lower `kdf_version` rejected |
| CR-10 header tamper full path | `cr_wrap.rs` | PASS — salt/params/ciphertext swaps all fail open |
| CR-11 zeroization spot | `cr_ops.rs` (+ unit canary) | PASS — freed buffers verified zeroed |
| CR-12 recovery/device payload split | `cr_wrap.rs` | PASS — credential field forbidden in recovery payload, mandatory in device envelope, post-rotation too |
| CR-13 cred_mp/cred_rk derivation + no-persistence | `cr_ops.rs` | PASS — §2.9 contexts distinct/deterministic; canary scan of produced files finds no secret bytes (raw or hex) |
| RG-10 TLV canonicalization | `tlv_canon.rs` | PASS — re-encode byte-identical; reordered tags, padded ints, non-NFC, trailing/missing terminator/truncation, reserved+unknown tags, over-length names, per-kind presence, key length/curve rules |
| XV-RUST-SIDE | `xv_vectors.rs` | PASS — 6 tests: byte-freshness of all 5 committed families + HKDF row recompute + TLV signature/proof verification + BIP-39 decode + ECDSA low-S/high-S + proof field-binding |

Committed vector artifacts (`src-tauri/vault-helper/tests/vectors/`):
`xv_hkdf.json` (all 11 §2.9 strings), `xv_tlv.json` (genesis / enroll /
revoke / recovery_epoch entries with signatures + proof), `xv_bip39.json`
(4 official reference rows + 2 vault-produced rows),
`xv_ecdsa.json` (65-byte key, low-S signature, high-S rejection twin),
`xv_recovery_epoch.json` (VK + manifest + device keys + nonce → proof).
Freshness is CI-enforceable via `gen_vectors --check` (gate check 2).

Fuzz smoke (§16.1): `cargo +nightly fuzz run tlv_decode` with ASan +
coverage instrumentation — **119,308,640 executions in 601 s, zero
crashes, zero artifacts** in the final gate run; two earlier runs
(138.2M and 156.6M execs) were also clean. Corpus: 65 entries,
regenerated per run.

Argon2id calibration (§2.3): full table in
`docs/security/argon2-calibration.md`. Headline: v1 tuple costs **115 ms**
on this MacBook Air M2 (macOS 26.6.2), well under the 1 s budget for the
slowest supported Mac class; 128 MiB/t=3 measured at 305 ms as the
recorded v2 upgrade candidate. **Chosen tuple: `m=64 MiB, t=3, p=1`,
unchanged from the spec pin** — justification in the calibration doc.

## Gate evidence (all PASS, run 2026-09-18, this machine)

| # | Check (spec §18 Phase B) | Result |
|---|---|---|
| 1 | helper test suite | PASS — 76 tests, 0 failed |
| 2 | vector freshness (`gen_vectors --check`) | PASS — all committed vectors reproducible |
| 3 | file-length audit | PASS — all files ≤ 350 lines |
| 4 | supply chain (audit, vet, deps, rsa) | PASS — see below |
| 5 | Phase A gate regression | PASS — 11/11 (after gate-script race fix, see deviations) |
| 6 | fuzz smoke 600 s | PASS — 119.3M runs, no crash/panic/UB |

Supply-chain detail (check 4): `cargo audit` clean (rsa ignore unchanged);
`cargo vet` **Vetting Succeeded — 35 fully audited, 759 exempted**; the 35
new crates (RustCrypto line + `secrecy` + `unicode-normalization` line)
each carry a manual §17.4 review record in
`src-tauri/supply-chain/audits.toml` (unsafe-file counts from vendored
sources, FFI status, panic behavior, maintenance standing; `cmov` noted
for aarch64 inline-asm backends). Helper dependency tree: **95 crates**
(< 120 gate); no `rsa`; the helper has no direct `rand` dependency
(`rand_core` enters transitively via p256/ecdsa and is vet-covered).
`getrandom` 0.4 is the only CSPRNG source called by helper code.

Main crate sanity: `cargo test --lib` 259 pass / 1 pre-existing failure
(`core::storage_tests::test_storage_lifecycle`, broken by commit
`9c73bfa` before Phase A; out of scope, unchanged by this phase).

## Deviation record

1. **Crate-major substitutions (§17.1 pins vs. resolved versions).** The
   spec's minor pins (`argon2 0.5`, `chacha20poly1305 0.10`, `hkdf 0.12`,
   `p256 0.13`) predate the digest-0.11 line the main app already uses.
   Resolved: argon2 0.6.0, chacha20poly1305 0.11.0, hkdf/hmac 0.13,
   p256 0.14, ecdsa 0.17, signature 3.0, zeroize 1.9, secrecy 0.10.3,
   subtle 2.6, unicode-normalization 0.1.25, getrandom 0.4. Same
   algorithms, same maintainers; each vetted in `audits.toml`. API drift
   absorbed (hmac 0.13 `KeyInit`, ecdsa 0.17 infallible `normalize_s`,
   rand 0.10 dropping `OsRng` → direct `getrandom` calls, which also
   removed the direct `rand` dependency).
2. **`panic=abort` placement.** Cargo forbids `panic` in per-package
   profiles; §2.11's release requirement is implemented as
   `RUSTFLAGS="-C panic=abort -C debug=0"` on the release invocation in
   `scripts/build-helper.sh`, leaving the shared workspace profile (and
   the main app's unwind strategy) untouched.
3. **Phase A gate race fix.** `scripts/phase-a-gate.sh` check 4 raced a
   stale socket file (no unlink-on-exit): `wait_for_socket` could accept
   the dead helper's socket before the new helper unlinked/rebound it,
   flaking as ECONNREFUSED/ENOENT under nested invocation. Fixed
   gate-side (`rm -f` of the socket after the old helper exits);
   verified 4/4 consecutive full PASS runs standalone plus a PASS inside
   the Phase B gate. No helper code semantics changed.
4. **Formatting normalization.** Phase A helper files are rustfmt-clean
   now (`cargo fmt -p source-vault-helper`, reflow-only diffs) and clippy
   is warning-free (`--all-targets`); two Phase A one-liners simplified
   for lints (`state.rs` identity match, `main.rs` struct-init). The
   Phase A regression gate (check 5) proves no behavior drift.
5. **XV families deferred per phase plan.** XV-ECDH/HPKE, XV-HPKE-SE and
   XV-SAS require enrollment (Phase E); Apple-signed XV-ECDSA vectors
   require the Secure Enclave side (Phase E). Phase B ships the
   Rust-produced families the spec lists for this stage; the vector
   directory layout and freshness gate are in place for the rest.
6. **CR-07 runtime.** The 2²⁰-seal nonce audit takes ~150 s in debug
   builds (dominant cost of gate check 1). Kept on the production seal
   path deliberately; the gate budget absorbs it.

## Invariants confirmed

- No new IPC ops: helper surface remains `hello` / `get_state` / `lock`;
  the crypto core is library-only, invoked by nothing over the socket.
- No bulk-secret API; secrets live in `SecretBytes`/`SecretVec` and are
  wiped on drop (CR-11 canaries).
- The crypto core writes nothing to disk by construction; CR-13 scans the
  produced artifacts for canary secrets (raw and hex) and finds none.
- BIP-39, hex, and TLV codecs are in-house (no mnemonic/hex crates in the
  helper graph).

## Reproduce

```bash
npm run gate:phase-b        # full 6-check gate (~17 min: tests + Phase A regression + 10-min fuzz)
npm run gate:phase-a        # Phase A gate alone
cd src-tauri && cargo run -p source-vault-helper --release --bin kdf_bench   # calibration table
cd src-tauri && cargo run -p source-vault-helper --bin gen_vectors -- --check # vector freshness
```

Phase B is complete. Phase C (vault file I/O, unlock flows, secure panel
wiring) has not been started and requires a separate authorization.
