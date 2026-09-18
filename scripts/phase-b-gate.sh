#!/usr/bin/env bash
# Phase B gate (implementation spec §18, Phase B row). Runs every Phase B
# gate check end-to-end and prints a PASS/FAIL line per check; exits
# nonzero if any check fails.
#
#   1. full helper test suite: lib units + CR-01…13, RG-10, XV-RUST-SIDE,
#      IPC regression (includes CR-07's 2^20 nonce audit, slow in debug)
#   2. vector freshness: gen_vectors --check (spec §16.8)
#   3. file-length audit (repo modularity rule, extended to vault-helper)
#   4. supply chain: scripts/audit-deps.sh (cargo audit, cargo vet,
#      helper dependency count, rsa absence)
#   5. Phase A gate regression (no IPC boundary drift; spec §19: every
#      prior phase gate stays green)
#   6. fuzz smoke: 10 min libFuzzer against the TLV decoder (spec §16.1)
#
# Check 6 requires: rustup toolchain install nightly --profile minimal &&
# cargo +nightly install cargo-fuzz --locked. Fuzz corpus/artifacts live
# under src-tauri/vault-helper/fuzz/ (gitignored except fuzz targets).
set -uo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
SRC_TAURI="$ROOT/src-tauri"
T="$(mktemp -d /tmp/vhgate-b.XXXXXX)"
RESULTS=()

cleanup() { rm -rf "$T"; }
trap cleanup EXIT

record() { # name, PASS|FAIL, evidence
    RESULTS+=("$1|$2|$3")
    printf '%s  %-62s %s\n' "$2" "$1" "$3"
}

fail() { record "$1" FAIL "${2:-}"; }

echo "== Phase B gate (workdir $T)"

# --- 1. full helper test suite -------------------------------------------------
echo "   (running full suite; CR-07 audits 2^20 seals and takes minutes in debug)"
if (cd "$SRC_TAURI" && cargo test -p source-vault-helper --quiet) >"$T/tests.log" 2>&1; then
    TOTAL=$(grep -oE '[0-9]+ passed' "$T/tests.log" | awk '{s+=$1} END {print s}')
    FAILED=$(grep -oE '[0-9]+ failed' "$T/tests.log" | awk '{s+=$1} END {print s+0}')
    [ "$FAILED" -eq 0 ] \
        && record "helper test suite (units, CR/RG, XV, IPC)" PASS "$TOTAL tests, 0 failed" \
        || fail "helper test suite (units, CR/RG, XV, IPC)" "$FAILED failures in $T/tests.log"
else
    fail "helper test suite (units, CR/RG, XV, IPC)" "see $T/tests.log"
    tail -5 "$T/tests.log"
fi

# --- 2. vector freshness ---------------------------------------------------------
if (cd "$SRC_TAURI" && cargo run -q -p source-vault-helper --bin gen_vectors -- --check) >"$T/vectors.log" 2>&1; then
    record "vector freshness (gen_vectors --check)" PASS "all committed vectors reproducible"
else
    fail "vector freshness (gen_vectors --check)" "$(tail -1 "$T/vectors.log")"
fi

# --- 3. file-length audit ---------------------------------------------------------
if (cd "$ROOT" && node scripts/check-file-lengths.mjs) >"$T/lengths.log" 2>&1; then
    record "file-length audit (≤350 lines)" PASS "all files within cap"
else
    fail "file-length audit (≤350 lines)" "$(tail -3 "$T/lengths.log" | tr '\n' ' ')"
fi

# --- 4. supply chain ---------------------------------------------------------------
OUT=$(bash "$ROOT/scripts/audit-deps.sh" 2>&1); RC=$?
[ $RC -eq 0 ] && record "supply chain (audit, vet, helper deps, rsa absence)" PASS "$(echo "$OUT" | grep -E 'dependency count|rsa' | tr '\n' '; ')" \
              || fail "supply chain (audit, vet, helper deps, rsa absence)" "$(echo "$OUT" | tail -3 | tr '\n' ' ')"

# --- 5. Phase A regression ----------------------------------------------------------
OUT=$(bash "$ROOT/scripts/phase-a-gate.sh" 2>&1); RC=$?
[ $RC -eq 0 ] && record "Phase A gate regression (IPC boundary unchanged)" PASS "$(echo "$OUT" | tail -1)" \
              || fail "Phase A gate regression (IPC boundary unchanged)" "$(echo "$OUT" | grep FAIL | head -2 | tr '\n' ' ')"

# --- 6. fuzz smoke --------------------------------------------------------------------
if command -v cargo-fuzz >/dev/null 2>&1 && rustup toolchain list | grep -q '^nightly'; then
    echo "   (fuzz smoke: 10 min against the TLV decoder)"
    OUT=$(cd "$SRC_TAURI/vault-helper" && cargo +nightly fuzz run tlv_decode -- -max_total_time=600 -timeout=10 2>&1); RC=$?
    if [ $RC -eq 0 ] && echo "$OUT" | grep -q "Done .* runs in"; then
        RUNS=$(echo "$OUT" | grep -oE "Done [0-9]+ runs" | head -1)
        record "fuzz smoke: TLV decoder (600 s)" PASS "$RUNS, no crash/panic/UB"
    else
        fail "fuzz smoke: TLV decoder (600 s)" "$(echo "$OUT" | tail -3 | tr '\n' ' ')"
    fi
else
    fail "fuzz smoke: TLV decoder (600 s)" "cargo-fuzz/nightly missing: rustup toolchain install nightly --profile minimal && cargo +nightly install cargo-fuzz --locked"
fi

# --- summary ---------------------------------------------------------------------------
echo
FAILS=0
for r in "${RESULTS[@]}"; do
    case "$r" in *"|FAIL|"*) FAILS=$((FAILS+1)) ;; esac
done
if [ "$FAILS" -eq 0 ]; then
    echo "PHASE B GATE: PASS (${#RESULTS[@]} checks)"
    exit 0
else
    echo "PHASE B GATE: FAIL ($FAILS of ${#RESULTS[@]} checks failed)"
    exit 1
fi
