# Phase E0 proof of concept (spec §2.12)

Interoperability evidence only — **not** product code, not built by any
gate, and not a dependency of the helper or the app:

- `hpke-se/` — Rust RFC 9180 HPKE (`hpke` crate) linked against the
  §2.12 Swift bridge (`../vault-apple-crypto`). Its own cargo workspace on
  purpose, so the shipping helper's dependency graph and supply-chain
  surface are unchanged. `cargo test` runs the macOS legs and the RFC 9180
  known-answer test.
- `hpke-ios/` — the on-device leg: CryptoKit `HPKE.Sender` →
  `HPKE.Recipient` with a Secure-Enclave key, plus the same RFC 9180
  vector, on a real iPhone. `./hpke-ios/build-and-run.sh` builds, installs,
  and launches it; uninstall afterwards (the command is printed).

Synthetic data only. Results: `docs/security/phase-e0-verification.md`.
