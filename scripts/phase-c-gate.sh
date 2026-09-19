#!/usr/bin/env bash
# Phase C gate (implementation spec §18, Phase C row). Runs every Phase C
# gate check end-to-end and prints a PASS/FAIL line per check; exits
# nonzero if any check fails.
#
#   1. full helper test suite (units, CR/RG, XV, IPC, vault op tests)
#   2. file-length audit (repo modularity rule)
#   3. debug helper: build + bundle + sign + verify (scripts/build-helper.sh)
#   4. release helper: compiles, and every OV0_VAULT_* debug override is
#      compiled out (no override name present in the binary)
#   5. E2E lifecycle over the signed socket: setup → unlock → add → list
#      (metadata only) → update → reveal allow → delete
#   6. capture_check reverse flow: reveal with the client's capture handler
#      answering "unsafe" → CAPTURE_UNSAFE + capture_unsafe event, no secret
#   7. unknown unlock kind → UNKNOWN_OP
#   8. auto-lock minutes op: 4 rejected, 5 accepted
#   9. panel events: secure_panel_visible true→false bracket with title
#  10. change_master_password: old MP → WRONG_CREDENTIAL after restart,
#      new MP unlocks, data survives the re-wrap
#  11. LA presence denial blocks mutation (PRESENCE_DENIED)
#  12. auto-lock observed (OV0_VAULT_AUTO_LOCK_SECS=3) → locked/timeout
#  13. UI-01/UI-03 live panel (OV0_VAULT_PANEL_SCRIPT=autoshow): real
#      window with native secure fields shown, dismissed by the watchdog
#      within seconds (not the 120 s timeout), cancel brackets, no partial
#      vault; focus/secure-input probe values recorded for UI-02
#  13a. (C.1) explicit lock preempts an in-flight REAL panel on the same
#      connection (tests/lock_preempts_panel.rs, OV0_VAULT_APPKIT_TESTS=1)
#  13b. (C.1) SIGTERM with a signed client attached and stderr a broken
#      pipe (the orphaned-helper incident) → exits within 10 s
#  14. main app: cargo check + capture wiring/decision tests (panel events
#      → suppression, helper-frontmost → no keystrokes)
#  15. frontend: npm run build
#  16. supply chain: scripts/audit-deps.sh
#  17. Phase B gate regression (includes Phase A; spec §19). Skip with
#      PHASE_C_SKIP_REGRESSION=1 while iterating — a gate run of record
#      must not skip it.
#
# Synthetic credentials only. Keychain isolation: every helper runs with a
# run-unique, per-vault OV0_VAULT_KEYCHAIN_PREFIX and the items are deleted
# on exit. Check 13 briefly shows a real panel on screen (~400 ms).
set -uo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
SRC_TAURI="$ROOT/src-tauri"
T="$(mktemp -d /tmp/vhgate-c.XXXXXX)"
KC_PREFIX="ov0gatec-$$-"
RESULTS=()

MP="synthetic-gate-master-password-0001"
MP_NEW="synthetic-gate-master-password-0002"
PW1="synthetic-gate-login-password-v1"
PW2="synthetic-gate-login-password-v2"

cleanup() {
    [ -n "${HELPER_PID:-}" ] && kill "$HELPER_PID" 2>/dev/null
    for v in v1 v2 v3 v4 v5 bp; do
        for svc in com.racker.zero.vault.state com.racker.zero.vault.helper-prefs; do
            security delete-generic-password -s "${KC_PREFIX}${v}-${svc}" >/dev/null 2>&1
        done
    done
    rm -rf "$T"
}
trap cleanup EXIT

record() { # name, PASS|FAIL, evidence
    RESULTS+=("$1|$2|$3")
    printf '%s  %-62s %s\n' "$2" "$1" "$3"
}
fail() { record "$1" FAIL "${2:-}"; }
check() { # name, evidence, condition-command...
    local name="$1" ev="$2"; shift 2
    if "$@"; then record "$name" PASS "$ev"; else fail "$name" "$ev"; fi
}

wait_for_socket() { # path, pid
    for _ in $(seq 1 100); do
        [ -S "$1" ] && return 0
        kill -0 "$2" 2>/dev/null || return 1
        sleep 0.05
    done
    return 1
}

stop_helper() {
    [ -n "${HELPER_PID:-}" ] && { kill "$HELPER_PID" 2>/dev/null; wait "$HELPER_PID" 2>/dev/null; HELPER_PID=; }
    rm -f "$SOCK"
}

# start_helper <vault_dir> [ENV=VAL ...] — always isolated keychain + LA stub.
# One keychain prefix per vault dir: the §2.8 seen-generation item is
# per-install state, so sharing it across vaults would read as rollback.
start_helper() {
    local dir="$1"; shift
    stop_helper
    env OV0_VAULT_SOCKET_PATH="$SOCK" OV0_VAULT_DIR="$dir" \
        OV0_VAULT_KEYCHAIN_PREFIX="${KC_PREFIX}$(basename "$dir")-" OV0_VAULT_LA_STUB=allow "$@" \
        "$HELPER_BIN" >>"$T/helper.log" 2>&1 &
    HELPER_PID=$!
    wait_for_socket "$SOCK" "$HELPER_PID"
}

# c <mode> [args] — one client op; output also appended to $OPLOG.
c() { "$CLIENT" "$SOCK" "$@" 2>&1 | tee -a "$OPLOG"; }
has() { [[ "$1" == *"$2"* ]]; }        # has <haystack> <needle>
first() { echo "$1" | head -1; }
# verdict <name> <evidence> — PASS iff no condition appended to $WHY.
verdict() {
    if [ -z "$WHY" ]; then record "$1" PASS "$2"; else fail "$1" "FAILED: $WHY| $2"; fi
}

echo "== Phase C gate (workdir $T)"

# --- 1. helper test suite -------------------------------------------------------
echo "   (full helper suite; vault op tests run the production Argon2id tuple)"
if (cd "$SRC_TAURI" && cargo test -p source-vault-helper --quiet) >"$T/tests.log" 2>&1; then
    TOTAL=$(grep -oE '[0-9]+ passed' "$T/tests.log" | awk '{s+=$1} END {print s}')
    FAILED=$(grep -oE '[0-9]+ failed' "$T/tests.log" | awk '{s+=$1} END {print s+0}')
    check "helper test suite (units, CR/RG, XV, IPC, vault ops)" "$TOTAL tests, $FAILED failed" [ "$FAILED" -eq 0 ]
else
    fail "helper test suite (units, CR/RG, XV, IPC, vault ops)" "see $T/tests.log"
    tail -5 "$T/tests.log"
fi

# --- 2. file-length audit -----------------------------------------------------------
if (cd "$ROOT" && node scripts/check-file-lengths.mjs) >"$T/lengths.log" 2>&1; then
    record "file-length audit (≤350 lines)" PASS "all files within cap"
else
    fail "file-length audit (≤350 lines)" "$(tail -3 "$T/lengths.log" | tr '\n' ' ')"
fi

# --- 3. debug helper build + sign ------------------------------------------------------
if bash "$ROOT/scripts/build-helper.sh" debug >"$T/build.log" 2>&1; then
    # shellcheck disable=SC1091
    . "$SRC_TAURI/target/helper-bundle/debug/build.env"
    record "debug helper build+sign+verify" PASS "OU $TEAM_OU, deep-strict + DR + no entitlements"
else
    fail "debug helper build+sign+verify" "see $T/build.log"
    tail -20 "$T/build.log"
    echo "FATAL: cannot continue without a signed helper"; exit 1
fi
HELPER_BIN="$BUNDLE/Contents/MacOS/source-vault-helper"
TS="$([[ "$IDENTITY" == *"Apple Development"* ]] && echo --timestamp=none || echo "")"
CLIENT="$T/legit"
cp "$SRC_TAURI/target/debug/vault-test-client" "$CLIENT"
codesign --force $TS --sign "$IDENTITY" -i com.racker.zero "$CLIENT" 2>"$T/sign.log"

# --- 4. release build: debug overrides compiled out ----------------------------------------
# Unsigned: build-helper.sh refuses release without the production team
# identity. This check proves the release binary carries no override path.
if (cd "$SRC_TAURI" && cargo build -q -p source-vault-helper --release --bin source-vault-helper) >"$T/release.log" 2>&1; then
    LEAKS=$(strings "$SRC_TAURI/target/release/source-vault-helper" | grep -oE 'OV0_VAULT_[A-Z_]+' | sort -u | tr '\n' ' ')
    check "release helper: all OV0_VAULT_* overrides compiled out" "${LEAKS:-none present}" [ -z "$LEAKS" ]
else
    fail "release helper: all OV0_VAULT_* overrides compiled out" "release build failed, see $T/release.log"
fi

SOCK="$T/s"; OPLOG="$T/ops.log"

# --- 5–9. lifecycle, reverse capture check, RK kind, prefs, panel events -----------------
# Events arrive on the connection that issued the op (one app-class slot),
# so each flow's EVENT= lines land in the section's op log.
OPLOG="$T/ops-lifecycle.log"
start_helper "$T/v1" OV0_VAULT_PANEL_SCRIPT="submit:$MP" || fail "E2E lifecycle" "helper did not start"
OUT_SETUP=$(c setup); OUT_UNLOCK=$(c unlock-mp)
OUT_ADD=$(c add-login "Gate Login" "gate@example.test" example.test "$PW1")
REF=$(echo "$OUT_ADD" | sed -n 's/^REF=//p')
OUT_LIST=$(c list)
OUT_UPD=$(c update "$REF" password "$PW2")
OUT_REV=$(c reveal "$REF" allow)
OUT_DENY=$(c reveal "$REF" deny)
OUT_RK=$(c unlock-kind-expect-unknown); RC_RK=$?
OUT_AL4=$(c set-autolock 4); OUT_AL5=$(c set-autolock 5)
OUT_DEL=$(c delete "$REF"); OUT_LIST2=$(c list)
stop_helper

WHY=""
has "$OUT_SETUP" "OP_OK state=locked" || WHY+="setup:$(first "$OUT_SETUP") "
has "$OUT_UNLOCK" "OP_OK state=unlocked" || WHY+="unlock:$(first "$OUT_UNLOCK") "
[ -n "$REF" ] || WHY+="add:$(first "$OUT_ADD") "
has "$OUT_LIST" '"Gate Login"' || WHY+="list-missing-title "
has "$OUT_LIST" "$PW1" && WHY+="list-leaked-secret "
has "$OUT_UPD" "OP_OK" || WHY+="update:$(first "$OUT_UPD") "
has "$OUT_REV" "\"password\":\"$PW2\"" || WHY+="reveal:$(first "$OUT_REV") "
has "$OUT_DEL" "OP_OK" || WHY+="delete:$(first "$OUT_DEL") "
has "$OUT_LIST2" 'LIST=[]' || WHY+="list-after-delete:$(echo "$OUT_LIST2" | grep LIST=) "
verdict "E2E lifecycle: setup→unlock→add→list→update→reveal→delete" \
    "ref=$REF; list metadata-only; reveal=v2; list empty after delete"

WHY=""
has "$OUT_DENY" "OP_ERROR=CAPTURE_UNSAFE" || WHY+="deny:$(first "$OUT_DENY") "
has "$OUT_DENY" "synthetic-gate-login" && WHY+="secret-in-deny-output "
has "$OUT_DENY" '"event":"capture_unsafe"' || WHY+="no-capture_unsafe-event "
verdict "capture_check reverse flow fails closed (CS-06 analog)" \
    "$(first "$OUT_DENY"); $(echo "$OUT_DENY" | grep -o '"event":"capture_unsafe"[^}]*}')"

check "unknown unlock kind → UNKNOWN_OP" "$OUT_RK" [ $RC_RK -eq 0 ]

WHY=""
has "$OUT_AL4" "OP_ERROR=INVALID_INPUT" || WHY+="4:$(first "$OUT_AL4") "
has "$OUT_AL5" "OP_OK" || WHY+="5:$(first "$OUT_AL5") "
verdict "set_auto_lock_minutes: 4 rejected, 5 accepted" "4→$(first "$OUT_AL4"); 5→$(first "$OUT_AL5")"

WHY=""
PANEL_EVENTS=$(grep -o '"event":"secure_panel_visible"[^}]*' "$OPLOG" | sort -u | tr '\n' ' ')
echo "$OUT_SETUP" | grep -q '"visible":true' || WHY+="setup-no-visible:true "
echo "$OUT_SETUP" | grep -q '"visible":false' || WHY+="setup-no-visible:false "
has "$OUT_SETUP" "Create Master Password" || WHY+="setup-no-title "
has "$OUT_UNLOCK" '"visible":false' || WHY+="unlock-no-bracket "
verdict "panel events: secure_panel_visible true/false with title (UI-01)" "$PANEL_EVENTS"

# --- 10. change master password across helper restarts ------------------------------------
OPLOG="$T/ops-changemp.log"
start_helper "$T/v2" OV0_VAULT_PANEL_SCRIPT="submit:$MP,$MP_NEW"
c setup >/dev/null; c unlock-mp >/dev/null
RW_REF=$(c add-login "Rewrap Canary" "rw@example.test" example.test "$PW1" | sed -n 's/^REF=//p')
OUT_CHG=$(c change-mp)
start_helper "$T/v2" OV0_VAULT_PANEL_SCRIPT="submit:$MP"
OUT_OLD=$(c unlock-mp)
start_helper "$T/v2" OV0_VAULT_PANEL_SCRIPT="submit:$MP_NEW"
OUT_NEW=$(c unlock-mp); OUT_RW=$(c reveal "$RW_REF" allow)
stop_helper
WHY=""
[ -n "$RW_REF" ] || WHY+="canary-add-failed "
has "$OUT_CHG" "OP_OK" || WHY+="change:$(first "$OUT_CHG") "
has "$OUT_OLD" "OP_ERROR=WRONG_CREDENTIAL" || WHY+="old:$(first "$OUT_OLD") "
has "$OUT_NEW" "state=unlocked" || WHY+="new:$(first "$OUT_NEW") "
has "$OUT_RW" "\"password\":\"$PW1\"" || WHY+="canary-reveal:$(first "$OUT_RW") "
verdict "change_master_password: old MP dead, new MP opens, data kept" \
    "change→$(first "$OUT_CHG"); old→$(first "$OUT_OLD"); new→$(first "$OUT_NEW"); canary intact"

# --- 11. presence denial -----------------------------------------------------------------
OPLOG="$T/ops-presence.log"
start_helper "$T/v3" OV0_VAULT_PANEL_SCRIPT="submit:$MP" OV0_VAULT_LA_STUB=deny
c setup >/dev/null; c unlock-mp >/dev/null
OUT_LA=$(c add-login "Denied" "d@example.test" example.test "$PW1"); OUT_LA_LIST=$(c list)
stop_helper
WHY=""
has "$OUT_LA" "OP_ERROR=PRESENCE_DENIED" || WHY+="add:$(first "$OUT_LA") "
has "$OUT_LA_LIST" 'LIST=[]' || WHY+="something-written "
verdict "LA presence denial blocks mutation" "$(first "$OUT_LA"); $(echo "$OUT_LA_LIST" | grep LIST=)"

# --- 12. auto-lock ------------------------------------------------------------------------
# The watcher connects AFTER unlock so it holds the app slot when the
# tick thread fires the timeout lock.
OPLOG="$T/ops-autolock.log"
start_helper "$T/v4" OV0_VAULT_PANEL_SCRIPT="submit:$MP" OV0_VAULT_AUTO_LOCK_SECS=3
c setup >/dev/null; c unlock-mp >/dev/null
"$CLIENT" "$SOCK" watch-events 8 >"$T/events-al.log" 2>&1
OUT_ST=$(c state); stop_helper
WHY=""
grep -q '"event":"locked"' "$T/events-al.log" || WHY+="no-locked-event "
grep -q '"reason":"timeout"' "$T/events-al.log" || WHY+="reason-not-timeout "
has "$OUT_ST" "STATE=locked" || WHY+="state:$OUT_ST "
verdict "auto-lock fires after idle window (3 s test override)" \
    "$(grep -o '"event":"locked"[^}]*' "$T/events-al.log" | head -1); post-state $OUT_ST"

# --- 13. live panel probe (UI-01/02/03) ---------------------------------------------------
OPLOG="$T/ops-panel.log"
PROBE="$T/probe.json"
start_helper "$T/v5" OV0_VAULT_PANEL_SCRIPT=autoshow OV0_VAULT_PANEL_PROBE="$PROBE"
UI_START=$SECONDS; OUT_UI=$(c setup)
stop_helper
PROBE_BODY=$(cat "$PROBE" 2>/dev/null || echo "no probe written")
WHY=""
[ -s "$PROBE" ] || WHY+="no-probe(panel-never-shown) "
grep -q '"field_class":"NSSecureTextField"' "$PROBE" 2>/dev/null || WHY+="not-secure-field "
has "$OUT_UI" "OP_ERROR=PANEL_CANCELLED" || WHY+="setup:$(first "$OUT_UI") "
has "$OUT_UI" '"visible":true' || WHY+="no-visible:true "
has "$OUT_UI" '"visible":false' || WHY+="no-visible:false "
[ ! -e "$T/v5/header.json" ] || WHY+="partial-vault-written "
[ $((SECONDS - UI_START)) -le 10 ] || WHY+="dismissal-took-$((SECONDS - UI_START))s "
# Focus/secure-input (UI-02) is recorded, not asserted: the gate client is
# never the active app, so it cannot yield focus to the helper the way
# SOURCE does (macOS 14 cooperative activation). UI-02 is closed by the
# in-app manual check in docs/security/phase-c-verification.md.
verdict "live panel shown + dismissed, native secure fields (UI-01/03)" "$PROBE_BODY"

# --- 13a. lock preempts a live panel (C.1) --------------------------------------------------
if (cd "$SRC_TAURI" && OV0_VAULT_APPKIT_TESTS=1 cargo test -q -p source-vault-helper --test lock_preempts_panel) >"$T/lockp.log" 2>&1 \
    && grep -q "lock_preempts_panel: PASS" "$T/lockp.log"; then
    record "explicit lock preempts live panel, same connection (C.1)" PASS "$(grep -o 'locked event[^;]*;[^;]*' "$T/lockp.log" | head -1)"
else
    fail "explicit lock preempts live panel, same connection (C.1)" "$(grep -E 'FAIL|panicked|timed out' "$T/lockp.log" | head -2 | tr '\n' ' ')"
fi

# --- 13b. SIGTERM: signed client attached + broken stderr (C.1) -------------------------------
# stderr goes to a pipe whose reader exits after the first line (the
# `tauri dev`-parent-killed incident); every later log write hits EPIPE.
BP="$T/bp"; mkdir -p "$BP"
( env OV0_VAULT_SOCKET_PATH="$BP/s" OV0_VAULT_DIR="$BP/v" OV0_VAULT_KEYCHAIN_PREFIX="${KC_PREFIX}bp-" \
    "$HELPER_BIN" 2>&1 >/dev/null & echo $! >"$BP/pid"; wait ) | head -1 >/dev/null &
for _ in $(seq 100); do [ -S "$BP/s" ] && break; sleep 0.05; done
BP_PID=$(cat "$BP/pid" 2>/dev/null)
"$CLIENT" "$BP/s" hold 60 >/dev/null 2>&1 & BP_HOLD=$!
sleep 1; kill -TERM "$BP_PID" 2>/dev/null; BP_START=$SECONDS; BP_ALIVE=1
for _ in $(seq 100); do kill -0 "$BP_PID" 2>/dev/null || { BP_ALIVE=0; break; }; sleep 0.1; done
[ $BP_ALIVE -eq 1 ] && kill -9 "$BP_PID" 2>/dev/null
kill "$BP_HOLD" 2>/dev/null; wait "$BP_HOLD" 2>/dev/null
check "SIGTERM with client attached + broken stderr exits (C.1)" \
    "$([ $BP_ALIVE -eq 0 ] && echo "exited in $((SECONDS - BP_START))s" || echo "still alive after 10s")" [ $BP_ALIVE -eq 0 ]

# --- 14. main app ---------------------------------------------------------------------------
if (cd "$SRC_TAURI" && cargo check -q -p SOURCE && cargo test -q -p SOURCE --lib -- capture_exclusions vault_client) >"$T/main.log" 2>&1; then
    record "main app: cargo check + capture wiring/decision tests" PASS "$(grep -oE '[0-9]+ passed' "$T/main.log" | tail -1)"
else
    fail "main app: cargo check + capture wiring/decision tests" "see $T/main.log"
    tail -5 "$T/main.log"
fi

# --- 15. frontend ----------------------------------------------------------------------------
if (cd "$ROOT" && npm run build) >"$T/npm.log" 2>&1; then
    record "frontend build (npm run build)" PASS "tsc + vite green"
else
    fail "frontend build (npm run build)" "$(tail -3 "$T/npm.log" | tr '\n' ' ')"
fi

# --- 16. supply chain ------------------------------------------------------------------------
OUT=$(bash "$ROOT/scripts/audit-deps.sh" 2>&1); RC=$?
[ $RC -eq 0 ] && record "supply chain (audit, vet, helper deps, rsa absence)" PASS "$(echo "$OUT" | grep -E 'dependency count|rsa' | tr '\n' '; ')" \
              || fail "supply chain (audit, vet, helper deps, rsa absence)" "$(echo "$OUT" | tail -3 | tr '\n' ' ')"

# --- 17. prior-phase regression ----------------------------------------------------------------
if [ "${PHASE_C_SKIP_REGRESSION:-0}" = "1" ]; then
    fail "Phase B gate regression (incl. Phase A)" "SKIPPED via PHASE_C_SKIP_REGRESSION=1 — not a gate run of record"
else
    echo "   (Phase B regression: full suite again + Phase A + 10 min fuzz smoke)"
    OUT=$(bash "$ROOT/scripts/phase-b-gate.sh" 2>&1); RC=$?
    [ $RC -eq 0 ] && record "Phase B gate regression (incl. Phase A)" PASS "$(echo "$OUT" | tail -1)" \
                  || fail "Phase B gate regression (incl. Phase A)" "$(echo "$OUT" | grep FAIL | head -2 | tr '\n' ' ')"
fi

# --- summary -------------------------------------------------------------------------------------
echo
FAILS=0
for r in "${RESULTS[@]}"; do
    case "$r" in *"|FAIL|"*) FAILS=$((FAILS+1)) ;; esac
done
if [ "$FAILS" -eq 0 ]; then
    echo "PHASE C GATE: PASS (${#RESULTS[@]} checks)"
    exit 0
else
    echo "PHASE C GATE: FAIL ($FAILS of ${#RESULTS[@]} checks failed)"
    exit 1
fi
