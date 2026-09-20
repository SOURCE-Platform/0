#!/usr/bin/env bash
# Phase D gate (credential-vault-implementation-spec.md v0.3.1 §18 Phase
# D, incl. the Phase C.1 recovery-order and Phase D.1 checkpoint
# corrections): recovery layer.
#
#   1. Phase D helper tests: rotation engine (CR-08 store level, crash
#      matrix, import fingerprints, BK-10), registry + recovery_epoch
#      (RG-03…RG-17), the §4.8 registry checkpoint + repeated recovery
#      (CP-01…CP-08), FsBackupStore rehearsals RC-03…RC-07, RF-01…RF-08,
#      FR-01…FR-03, BK-10/13/14/16, RK IPC ops + UI-05 transcript audit
#   2. full helper suite (all phases, no fail-fast)
#   3. file-length audit
#   4. debug helper build + sign + verify
#   5. release helper: every OV0_VAULT_* debug override compiled out
#   6. signed E2E over the real socket: setup shows the RK → RK unlock →
#      add → rotate_recovery_key (VK rotation) → data kept → the new RK
#      unlocks (old-RK refusal: tests/vault_rk_ops.rs)
#   7. RK/MP never traverse IPC: every frame the client received (and the
#      helper log) scanned for ≥ 6 consecutive BIP-39 words and the MP
#   8. UI-04 live Recovery Key window (OV0_VAULT_SHEET_SCRIPT=autoshow-print):
#      real NSWindowSharingNone window, NSPrintOperation run from the
#      in-memory view while the window (and the capture bracket) is up,
#      no file / PDF written, unacknowledged window commits nothing
#   9. main-app IPC surface: no RK/word-bearing command argument
#  10. main app cargo check + frontend build
#  11. supply chain
#  12. Phase C gate regression (includes Phase B and Phase A)
#
# Synthetic credentials only. Check 8 shows a real window ~0.5 s.
set -uo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
SRC_TAURI="$ROOT/src-tauri"
T="$(mktemp -d /tmp/vhgate-d.XXXXXX)"
KC_PREFIX="ov0gated-$$-"
RESULTS=()
MP="synthetic-gate-d-master-password-0001"
PW1="synthetic-gate-d-login-password"
WORDLIST="$SRC_TAURI/vault-helper/src/crypto/data/bip39-english.txt"

cleanup() {
    [ -n "${HELPER_PID:-}" ] && kill "$HELPER_PID" 2>/dev/null
    for v in d1 d2; do
        for svc in com.racker.zero.vault.state com.racker.zero.vault.helper-prefs; do
            security delete-generic-password -s "${KC_PREFIX}${v}-${svc}" >/dev/null 2>&1
        done
    done
    rm -rf "$T"
}
trap cleanup EXIT

record() { RESULTS+=("$1|$2|$3"); printf '%s  %-62s %s\n' "$2" "$1" "$3"; }
fail() { record "$1" FAIL "${2:-}"; }
check() { local name="$1" ev="$2"; shift 2; if "$@"; then record "$name" PASS "$ev"; else fail "$name" "$ev"; fi; }
has() { [[ "$1" == *"$2"* ]]; }
first() { echo "$1" | head -1; }
verdict() { if [ -z "$WHY" ]; then record "$1" PASS "$2"; else fail "$1" "FAILED: $WHY| $2"; fi; }
wait_for_socket() { for _ in $(seq 1 100); do [ -S "$1" ] && return 0; kill -0 "$2" 2>/dev/null || return 1; sleep 0.05; done; return 1; }
stop_helper() { [ -n "${HELPER_PID:-}" ] && { kill "$HELPER_PID" 2>/dev/null; wait "$HELPER_PID" 2>/dev/null; HELPER_PID=; }; rm -f "$SOCK"; }
start_helper() {
    local dir="$1"; shift; stop_helper
    env OV0_VAULT_SOCKET_PATH="$SOCK" OV0_VAULT_DIR="$dir" \
        OV0_VAULT_KEYCHAIN_PREFIX="${KC_PREFIX}$(basename "$dir")-" OV0_VAULT_LA_STUB=allow "$@" \
        "$HELPER_BIN" >>"$T/helper.log" 2>&1 &
    HELPER_PID=$!
    wait_for_socket "$SOCK" "$HELPER_PID"
}
c() { "$CLIENT" "$SOCK" "$@" 2>&1 | tee -a "$OPLOG"; }
# Longest run of consecutive BIP-39 words in a file (RK leak detector).
max_word_run() {
    python3 - "$WORDLIST" "$1" <<'PY'
import re, sys
words = set(open(sys.argv[1]).read().split())
best = run = 0
for tok in re.findall(r"[a-z]+", open(sys.argv[2], errors="ignore").read().lower()):
    run = run + 1 if tok in words else 0
    best = max(best, run)
print(best)
PY
}

echo "== Phase D gate (workdir $T)"
cd "$SRC_TAURI" || exit 1

# --- 1. Phase D tests ------------------------------------------------------------------------
D_TESTS="--test vault_rotation --test registry_epoch --test registry_checkpoint --test recovery_total_loss --test recovery_finalize --test recovery_trusted --test vault_rk_ops"
# shellcheck disable=SC2086
cargo test -p source-vault-helper --no-fail-fast $D_TESTS >"$T/d-tests.log" 2>&1
D_PASS=$(grep -E "^test result" "$T/d-tests.log" | awk '{s+=$4} END {print s+0}')
D_FAIL=$(grep -E "^test result" "$T/d-tests.log" | awk '{s+=$6} END {print s+0}')
check "Phase D tests (rotation, registry/epoch, RC/RF/FR/BK, RK ops)" "$D_PASS passed, $D_FAIL failed" \
    [ "$D_FAIL" -eq 0 -a "$D_PASS" -gt 0 ]

# --- 2. full helper suite --------------------------------------------------------------------
cargo test -p source-vault-helper --no-fail-fast >"$T/suite.log" 2>&1
S_PASS=$(grep -E "^test result" "$T/suite.log" | awk '{s+=$4} END {print s+0}')
S_FAIL=$(grep -E "^test result" "$T/suite.log" | awk '{s+=$6} END {print s+0}')
check "full helper suite (Phases A–D)" "$S_PASS passed, $S_FAIL failed" [ "$S_FAIL" -eq 0 -a "$S_PASS" -gt 0 ]

# --- 3. file lengths -------------------------------------------------------------------------
if (cd "$ROOT" && node scripts/check-file-lengths.mjs) >"$T/lengths.log" 2>&1; then
    record "file-length audit (≤350 lines)" PASS "all files within cap"
else
    fail "file-length audit (≤350 lines)" "$(tail -3 "$T/lengths.log" | tr '\n' ' ')"
fi

# --- 4. debug helper build + sign ------------------------------------------------------------
if bash "$ROOT/scripts/build-helper.sh" debug >"$T/build.log" 2>&1; then
    # shellcheck disable=SC1091
    . "$SRC_TAURI/target/helper-bundle/debug/build.env"
    record "debug helper build+sign+verify" PASS "OU $TEAM_OU, deep-strict + DR"
else
    fail "debug helper build+sign+verify" "see build.log"; tail -20 "$T/build.log"; exit 1
fi
HELPER_BIN="$BUNDLE/Contents/MacOS/source-vault-helper"
TS="$([[ "$IDENTITY" == *"Apple Development"* ]] && echo --timestamp=none || echo "")"
CLIENT="$T/legit"
cp "$SRC_TAURI/target/debug/vault-test-client" "$CLIENT"
codesign --force $TS --sign "$IDENTITY" -i com.racker.zero "$CLIENT" 2>"$T/sign.log"

# --- 5. release: overrides compiled out ------------------------------------------------------
if cargo build -q -p source-vault-helper --release --bin source-vault-helper >"$T/release.log" 2>&1; then
    LEAKS=$(strings "$SRC_TAURI/target/release/source-vault-helper" | grep -oE 'OV0_VAULT_[A-Z_]+' | sort -u | tr '\n' ' ')
    check "release helper: all OV0_VAULT_* overrides compiled out" "${LEAKS:-none present}" [ -z "$LEAKS" ]
else
    fail "release helper: all OV0_VAULT_* overrides compiled out" "release build failed"
fi

SOCK="$T/s"; OPLOG="$T/ops.log"

# --- 6. signed E2E: RK lifecycle --------------------------------------------------------------
start_helper "$T/d1" OV0_VAULT_PANEL_SCRIPT="submit:$MP" || fail "E2E RK lifecycle" "helper did not start"
OUT_SETUP=$(c setup)
OUT_RKU=$(c unlock-rk)
OUT_ADD=$(c add-login "Gate D Login" "gate-d@example.test" example.test "$PW1")
REF=$(echo "$OUT_ADD" | sed -n 's/^REF=//p')
GEN_BEFORE=$(python3 -c "import json;print(json.load(open('$T/d1/header.json'))['vk_generation'])")
OUT_ROT=$(c rotate-rk)
GEN_AFTER=$(python3 -c "import json;print(json.load(open('$T/d1/header.json'))['vk_generation'])")
OUT_REV=$(c reveal "$REF" allow)
OUT_LOCK=$(c raw '{"op":"lock"}')
OUT_RKU2=$(c unlock-rk)
stop_helper
WHY=""
has "$OUT_SETUP" "OP_OK state=locked" || WHY+="setup:$(first "$OUT_SETUP") "
has "$OUT_SETUP" "Source Vault — Recovery Key" || WHY+="setup-no-rk-window-event "
[ -f "$T/d1/wraps/recovery.wrap" ] || WHY+="no-recovery.wrap "
has "$OUT_RKU" "OP_OK state=unlocked" || WHY+="rk-unlock:$(first "$OUT_RKU") "
[ -n "$REF" ] || WHY+="add:$(first "$OUT_ADD") "
has "$OUT_ROT" "OP_OK" || WHY+="rotate:$(first "$OUT_ROT") "
[ "$GEN_AFTER" = "$((GEN_BEFORE + 1))" ] || WHY+="vk_generation $GEN_BEFORE→$GEN_AFTER "
has "$OUT_REV" "\"password\":\"$PW1\"" || WHY+="data-lost-after-rotation "
has "$OUT_RKU2" "OP_OK state=unlocked" || WHY+="new-rk-unlock:$(first "$OUT_RKU2") "
verdict "E2E: setup shows RK → RK unlock → rotate RK (VK gen+1) → new RK" \
    "vk_generation $GEN_BEFORE→$GEN_AFTER; record intact; new RK unlocks"

# --- 7. RK/MP never traverse IPC --------------------------------------------------------------
RUN_OPS=$(max_word_run "$OPLOG"); RUN_LOG=$(max_word_run "$T/helper.log")
WHY=""
[ "$RUN_OPS" -lt 6 ] || WHY+="ops-log-word-run=$RUN_OPS "
[ "$RUN_LOG" -lt 6 ] || WHY+="helper-log-word-run=$RUN_LOG "
grep -q "$MP" "$OPLOG" "$T/helper.log" && WHY+="MP-in-transcript "
verdict "RK words / MP absent from every IPC frame and helper log" \
    "longest BIP-39 word run: frames $RUN_OPS, log $RUN_LOG (RK = 24)"

# --- 8. UI-04 live RK window + print --------------------------------------------------------
mkdir -p "$T/tmp"; touch "$T/marker"; OPLOG="$T/ops-print.log"; PROBE="$T/sheet-probe.json"
# Without a configured printer macOS refuses to run any print job (and
# shows a modal alert), so the print leg runs only when a printer exists.
if lpstat -p >/dev/null 2>&1; then SHEET_MODE=autoshow-print; else SHEET_MODE=autoshow; fi
start_helper "$T/d2" OV0_VAULT_PANEL_SCRIPT="submit:$MP" OV0_VAULT_SHEET_SCRIPT=$SHEET_MODE \
    OV0_VAULT_PANEL_PROBE="$PROBE" TMPDIR="$T/tmp/" || fail "UI-04 live RK window + print" "helper did not start"
OUT_PS=$(c setup)
stop_helper
USER_TMP="$(getconf DARWIN_USER_TEMP_DIR)"; USER_CACHE="$(getconf DARWIN_USER_CACHE_DIR)"
NEW_PDFS=$(find "$T" "$USER_TMP" "$USER_CACHE" -newer "$T/marker" -iname '*.pdf' 2>/dev/null | head -3)
TMP_FILES=$(find "$T/tmp" -type f 2>/dev/null | head -3)
WHY=""
if [ "$SHEET_MODE" = autoshow-print ]; then
    grep -q '"print_ran":true' "$PROBE" 2>/dev/null || WHY+="print-not-run "
    PRINT_NOTE="print ran in-memory"
else
    PRINT_NOTE="print leg NOT exercised: no printer configured on this Mac"
fi
grep -q '"sheet_on_screen":true' "$PROBE" 2>/dev/null || WHY+="window-not-on-screen "
has "$OUT_PS" '"title":"Source Vault — Recovery Key","visible":true' || WHY+="no-capture-bracket-open "
has "$OUT_PS" "OP_ERROR=PANEL_CANCELLED" || WHY+="unacknowledged-window-committed:$(first "$OUT_PS") "
[ ! -e "$T/d2/header.json" ] || WHY+="vault-left-behind "
[ -z "$NEW_PDFS" ] || WHY+="pdf-written:$NEW_PDFS "
[ -z "$TMP_FILES" ] || WHY+="temp-file:$TMP_FILES "
verdict "UI-04: RK window capture-excluded + bracketed, no file" "$PRINT_NOTE; $(cat "$PROBE" 2>/dev/null)"

# --- 9. main-app IPC surface ------------------------------------------------------------------
BAD=$(grep -nE '\b(words|mnemonic|recovery_key|rk_bytes|master_password|passphrase)\s*[:=?]' \
    "$SRC_TAURI/src/app/commands/vault.rs" "$ROOT/src/lib/vault.ts" | grep -vE '^\S+:[0-9]+:\s*(//|\*|/\*\*)' | head -3)
check "main-app IPC surface: no RK/MP-bearing command argument (UI-05)" "${BAD:-none}" [ -z "$BAD" ]

# --- 10. main app + frontend ------------------------------------------------------------------
if cargo check -q -p SOURCE >"$T/main.log" 2>&1 && (cd "$ROOT" && npm run build) >"$T/fe.log" 2>&1; then
    record "main app cargo check + frontend build" PASS "green"
else
    fail "main app cargo check + frontend build" "$(grep -E '^error' "$T/main.log" "$T/fe.log" | head -2 | tr '\n' ' ')"
fi

# --- 11. supply chain ---------------------------------------------------------------------------
OUT=$(bash "$ROOT/scripts/audit-deps.sh" 2>&1); RC=$?
[ $RC -eq 0 ] && record "supply chain (audit, vet, helper deps, rsa absence)" PASS "$(echo "$OUT" | grep -E 'dependency count|rsa' | tr '\n' '; ')" \
              || fail "supply chain (audit, vet, helper deps, rsa absence)" "$(echo "$OUT" | tail -3 | tr '\n' ' ')"

# --- 12. Phase C regression (includes B and A) --------------------------------------------------
if [ "${PHASE_D_SKIP_REGRESSION:-0}" = "1" ]; then
    fail "Phase C gate regression (incl. B, A)" "SKIPPED — not a gate run of record"
else
    echo "   (Phase C regression, which runs Phase B and Phase A)"
    OUT=$(bash "$ROOT/scripts/phase-c-gate.sh" 2>&1); RC=$?
    echo "$OUT" > "$T/phase-c.log"
    [ $RC -eq 0 ] && record "Phase C gate regression (incl. B, A)" PASS "$(echo "$OUT" | tail -1)" \
                  || fail "Phase C gate regression (incl. B, A)" "$(echo "$OUT" | grep FAIL | head -3 | tr '\n' ' ')"
fi

echo
FAILS=0
for r in "${RESULTS[@]}"; do case "$r" in *"|FAIL|"*) FAILS=$((FAILS+1)) ;; esac; done
if [ "$FAILS" -eq 0 ]; then echo "PHASE D GATE: PASS (${#RESULTS[@]} checks)"; exit 0; fi
echo "PHASE D GATE: FAIL ($FAILS of ${#RESULTS[@]} checks failed)"; exit 1
