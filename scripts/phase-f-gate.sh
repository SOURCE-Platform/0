#!/usr/bin/env bash
# Phase F gate (credential-vault-implementation-spec.md v0.4 §18 Phase F):
# the provider protocol, multi-writer sync and total-loss recovery.
#
#   1. Phase F Rust suites: vault-proto, vault-provider-core,
#      vault-coordinator, the end-to-end simulator (vault-tests) and the
#      helper's Phase F tests
#   2. full helper suite (Phases A–F, no fail-fast)
#   3. the deployable provider (its own workspace): tests + cargo audit
#   4. file-length audit (covers every Phase F crate)
#   5. PR-01: no canary (MP, PK, VK, RK, ikm_c) in any IPC frame, helper
#      event or HTTP exchange across all flows
#   6. BK-18: no canary anywhere in provider storage
#   7. release helper: the debug provider origins are compiled out
#   8. main-app IPC surface: no key-bearing command argument (UI-05)
#   9. main app cargo check + frontend build
#  10. supply chain (cargo audit + cargo vet)
#  11. Dependabot alert #108 (glib) is still open — never dismissed
#  12. no test silently ignored (allowlist: the U-1 hardware measurement)
#  13. U-1: Secure Enclave signing latency, measured and recorded
#  14. EV-03 on a physical A15+ iPhone, by name (owner hardware)
#  15. Phase E gate regression (which runs D, C, B and A)
#
# Synthetic vaults and synthetic credentials only.
set -uo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
SRC_TAURI="$ROOT/src-tauri"
T="$(mktemp -d /tmp/vhgate-f.XXXXXX)"
RESULTS=()
export CARGO_INCREMENTAL=0

trap 'rm -rf "$T"' EXIT

record() { RESULTS+=("$1|$2|$3"); printf '%s  %-62s %s\n' "$2" "$1" "$3"; }
fail() { record "$1" FAIL "${2:-}"; }
counts() { grep -E "^test result" "$1" | awk '{p+=$4; f+=$6} END {print p+0, f+0}'; }
suite() {
    local name="$1" log="$2"; shift 2
    "$@" >"$log" 2>&1
    local rc=$? p f
    read -r p f <<<"$(counts "$log")"
    if [ $rc -eq 0 ] && [ "$f" = "0" ] && [ "$p" -gt 0 ]; then
        record "$name" PASS "$p passed"
    else
        fail "$name" "$p passed, $f failed; $(grep -E 'panicked|^error' "$log" | head -2 | tr '\n' ' ')"
    fi
}
ran_ok() { grep -qE "^test $2 \.\.\. ok" "$1"; }

echo "== Phase F gate (workdir $T)"
cd "$SRC_TAURI" || exit 1

# --- 1. Phase F suites ---------------------------------------------------------------------------
suite "Phase F crates (proto, provider-core, coordinator, e2e)" "$T/f-crates.log" \
    cargo test --no-fail-fast -p vault-proto -p vault-provider-core -p vault-coordinator -p vault-tests
F_TESTS="--test sign_policy --test xv_v04 --test revision_binding --test merge_order --test store_recovery"
F_TESTS+=" --test resolve_op --test session_teardown --test state_transitions"
# shellcheck disable=SC2086
suite "helper Phase F tests (signing, merge, teardown, states)" "$T/f-helper.log" \
    cargo test --no-fail-fast -p source-vault-helper $F_TESTS

# --- 2. full helper suite ------------------------------------------------------------------------
suite "full helper suite (Phases A–F)" "$T/helper.log" cargo test --no-fail-fast -p source-vault-helper
# F.2b: the engine's own unit tests (moved out of the helper) and FFI-01,
# which reads the iOS static library (review SEC-I5 / VER-I1).
suite "vault engine and SOURCE Vault FFI (F.2b)" "$T/engine.log" \
    bash -c "cargo build -p vault-ffi --target aarch64-apple-ios && cargo test --no-fail-fast -p vault-engine -p vault-ffi"

# --- 3. deployable provider ----------------------------------------------------------------------
suite "vault-provider service tests (own workspace)" "$T/provider.log" \
    bash -c "cd '$SRC_TAURI/vault-provider' && cargo test --no-fail-fast"
if (cd "$SRC_TAURI/vault-provider" && cargo audit) >"$T/provider-audit.log" 2>&1; then
    record "vault-provider cargo audit" PASS "$(grep -oE 'Scanning .* for vulnerabilities \([0-9]+ crate dependencies\)' "$T/provider-audit.log" | grep -oE '[0-9]+ crate' | head -1)"
else
    fail "vault-provider cargo audit" "$(grep -E '^(ID|Crate):' "$T/provider-audit.log" | head -2 | tr '\n' ' ')"
fi

# --- 4. file lengths -----------------------------------------------------------------------------
if OUT=$(node "$ROOT/scripts/check-file-lengths.mjs" 2>&1); then
    record "file-length audit (≤ 350 lines, Phase F crates included)" PASS "$(echo "$OUT" | tail -1)"
else
    fail "file-length audit (≤ 350 lines, Phase F crates included)" "$(echo "$OUT" | tail -3 | tr '\n' ' ')"
fi

# --- 5./6. canary scans --------------------------------------------------------------------------
if ran_ok "$T/f-crates.log" pr01_no_canary_in_any_transcript; then
    record "PR-01: no canary in IPC frames, events or HTTP" PASS "MP, PK, VK, RK, ikm_c (raw/hex/b64)"
else
    fail "PR-01: no canary in IPC frames, events or HTTP" "pr01_no_canary_in_any_transcript did not pass"
fi
if ran_ok "$T/f-crates.log" bk18_no_canary_in_provider_storage; then
    record "BK-18: no canary in provider storage" PASS "state, blobs, nonces, claims, throttle"
else
    fail "BK-18: no canary in provider storage" "bk18_no_canary_in_provider_storage did not pass"
fi

# --- 7. release: debug origins compiled out --------------------------------------------------------
if cargo build -q --release -p source-vault-helper >"$T/release.log" 2>&1; then
    BIN="$SRC_TAURI/target/release/source-vault-helper"
    LEAK=$(strings "$BIN" 2>/dev/null | grep -cE 'provider\.test|127\.0\.0\.1:8787')
    if [ "$LEAK" = "0" ]; then
        record "release helper: debug provider origins compiled out" PASS "0 debug origins in the binary"
    else
        fail "release helper: debug provider origins compiled out" "$LEAK matches"
    fi
else
    fail "release helper: debug provider origins compiled out" "release build failed"
fi

# --- 8. main-app IPC surface ---------------------------------------------------------------------
BAD=$(grep -nE '\b(words|mnemonic|recovery_key|rk_bytes|master_password|passphrase|vk|device_backup_cred|ikm|sk_c)\s*[:=?]' \
    "$SRC_TAURI/src/app/commands/vault.rs" "$SRC_TAURI/src/app/commands/vault_backup.rs" "$ROOT/src/lib/vault.ts" \
    | grep -vE '^\S+:[0-9]+:\s*(//|\*|/\*\*)' | head -3)
if [ -z "$BAD" ]; then record "main-app IPC surface: no key-bearing argument (UI-05)" PASS "none"
else fail "main-app IPC surface: no key-bearing argument (UI-05)" "$BAD"; fi

# --- 9. main app + frontend ----------------------------------------------------------------------
if cargo check -q -p SOURCE >"$T/main.log" 2>&1 && (cd "$ROOT" && npm run build) >"$T/fe.log" 2>&1; then
    record "main app cargo check + frontend build" PASS "green"
else
    fail "main app cargo check + frontend build" "$(grep -E '^error' "$T/main.log" "$T/fe.log" | head -2 | tr '\n' ' ')"
fi

# --- 10. supply chain ----------------------------------------------------------------------------
if OUT=$(bash "$ROOT/scripts/audit-deps.sh" 2>&1); then
    record "supply chain (cargo audit + cargo vet)" PASS "$(echo "$OUT" | grep -E 'dependency count|fully audited' | tr '\n' ' ')"
else
    fail "supply chain (cargo audit + cargo vet)" "$(echo "$OUT" | tail -3 | tr '\n' ' ')"
fi

# --- 11. Dependabot #108 stays open --------------------------------------------------------------
STATE=$(cd "$ROOT" && gh api repos/SOURCE-Platform/0/dependabot/alerts/108 --jq .state 2>/dev/null)
if [ "$STATE" = "open" ]; then record "Dependabot #108 (glib) still open, not dismissed" PASS "state=open"
else fail "Dependabot #108 (glib) still open, not dismissed" "state=${STATE:-unreadable}"; fi

# --- 12. no silently ignored test ---------------------------------------------------------------
IGNORED=$(cat "$T"/f-crates.log "$T"/helper.log "$T"/provider.log 2>/dev/null | grep -E '^test .* \.\.\. ignored' \
    | grep -vE '^test u1_se_signing_latency ' | head -3 | tr '\n' ' ')
if [ -z "$IGNORED" ]; then record "no ignored test outside the allowlist" PASS "allowlist: u1_se_signing_latency"
else fail "no ignored test outside the allowlist" "$IGNORED"; fi

# --- 13. U-1 SE signing latency ------------------------------------------------------------------
U1=$(cargo test -q -p source-vault-helper --test se_latency -- --ignored --nocapture 2>&1 | grep -E '^U-1 SE signing' | head -1)
MEAN=$(echo "$U1" | sed -nE 's/.*mean=([0-9.]+) ms.*/\1/p')
# A measurement, not a threshold (design U-1): above ~20 ms/request the
# design adds batch blob signing. The gate fails only if nothing was measured.
if [ -n "$MEAN" ]; then
    BATCH=$(awk "BEGIN {print ($MEAN < 20) ? \"per-request signing\" : \"batch blob signing indicated\"}")
    record "U-1 SE signing latency recorded" PASS "$U1 → $BATCH"
else fail "U-1 SE signing latency recorded" "no measurement"; fi

# --- 14. EV-03 on a physical iPhone --------------------------------------------------------------
# The xcodebuild log of the physical-device run: a real device destination
# (never the simulator) and the EV-03 tests passing by name. Run ONLY the
# two safe suites on the owner's iPhone — the whole target includes tests
# that reset the app's stored keys:
#   xcodebuild test -project SourceMobile.xcodeproj -scheme SourceMobile \
#     -destination 'platform=iOS,id=<device-udid>' \
#     -only-testing:SourceMobileTests/VaultCatchUpTests \
#     -only-testing:SourceMobileTests/VaultEV03Tests
EV03_LOG="${PHASE_F_EV03_LOG:-}"
EV03_OK=1
for t in ev03EnclaveOpensAV2Envelope ev03EnclaveSignsStateGet; do
    grep -qE "Test $t\(\) passed" "$EV03_LOG" 2>/dev/null || EV03_OK=0
done
grep -qE 'Suite VaultCatchUpTests passed' "$EV03_LOG" 2>/dev/null || EV03_OK=0
if [ -n "$EV03_LOG" ] && [ "$EV03_OK" = 1 ] && grep -qE 'platform=iOS,(id|name)=' "$EV03_LOG" && ! grep -q 'Simulator' "$EV03_LOG"; then
    record "EV-03 on a physical A15+ iPhone (by name)" PASS "Enclave open + sign, and the catch-up suite, on a device"
else
    fail "EV-03 on a physical A15+ iPhone (by name)" "not run — needs the owner's iPhone (set PHASE_F_EV03_LOG)"
fi

# --- 15. Phase E regression ----------------------------------------------------------------------
if [ "${PHASE_F_SKIP_REGRESSION:-0}" = "1" ]; then
    fail "Phase E gate regression (incl. D, C, B, A)" "SKIPPED — not a gate run of record"
else
    echo "   (Phase E regression, which runs Phase D, C, B and A)"
    OUT=$(bash "$ROOT/scripts/phase-e-gate.sh" 2>&1); RC=$?
    echo "$OUT" > "$T/phase-e.log"
    if [ $RC -eq 0 ]; then record "Phase E gate regression (incl. D, C, B, A)" PASS "$(echo "$OUT" | tail -1)"
    else fail "Phase E gate regression (incl. D, C, B, A)" "$(echo "$OUT" | grep FAIL | head -3 | tr '\n' ' ')"; fi
fi

echo
FAILS=0
for r in "${RESULTS[@]}"; do case "$r" in *"|FAIL|"*) FAILS=$((FAILS+1)) ;; esac; done
if [ "$FAILS" -eq 0 ]; then echo "PHASE F GATE: PASS (${#RESULTS[@]} checks)"; exit 0; fi
echo "PHASE F GATE: FAIL ($FAILS of ${#RESULTS[@]} checks failed)"; exit 1
