#!/usr/bin/env bash
# Dependency security audit for the Rust supply chain.
#
# Baseline for the credential-vault supply-chain rules (security architecture
# §15.4, invariant 13): Cargo.lock is committed, and this script runs
# `cargo audit` (RustSec advisory database) against it.
#
# Usage: npm run audit:deps
set -euo pipefail

cd "$(dirname "$0")/../src-tauri"

if ! command -v cargo-audit >/dev/null 2>&1; then
    echo "error: cargo-audit is not installed." >&2
    echo "Install it with: cargo install cargo-audit --locked" >&2
    exit 2
fi
if ! command -v cargo-vet >/dev/null 2>&1; then
    echo "error: cargo-vet is not installed." >&2
    echo "Install it with: cargo install cargo-vet --locked" >&2
    exit 2
fi

# Known vulnerabilities fail the check (cargo audit exits nonzero);
# unmaintained/soundness notices are printed as warnings only.
#
# RUSTSEC-2023-0071 (rsa, Marvin timing attack): no fixed release exists,
# and rsa is not in the macOS build graph (`cargo tree -i rsa` is empty —
# it enters the lockfile only via target-gated, non-shipped sqlx backends;
# SOURCE ships sqlite only). Re-evaluate on every sqlx/rustls upgrade.
cargo audit --ignore RUSTSEC-2023-0071

# Supply-chain review gate: the current graph is exempted as the bootstrap
# baseline (supply-chain/config.toml); any NEW dependency added to
# Cargo.lock fails this check until a human vets or deliberately exempts it.
cargo vet
