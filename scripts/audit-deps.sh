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

# Known vulnerabilities fail the check (cargo audit exits nonzero);
# unmaintained/soundness notices are printed as warnings only.
cargo audit
