#!/usr/bin/env bash
# Phase A gate (implementation spec §18, Phase A row). Runs every Phase A
# gate check end-to-end and prints a PASS/FAIL line per check; exits
# nonzero if any check fails.
#
#   1. crate test suite (framing, ops, state, peer-auth units, socket E2E)
#   2. build + sign + verify SourceVaultHelper.app (deep-strict, DR, no entitlements)
#   3. signed legit client accepted (app class), boot state UNINITIALIZED
#   4. helper restart with a vault header present leaves state LOCKED
#   5. lock op is honored
#   6. ad-hoc signed client clone is rejected
#   7. same-team client with a wrong identifier is rejected
#   8. ad-hoc signed helper clone is rejected by the client (reverse check)
#   9. idle exit: helper exits with zero clients (timeout override, debug only)
#  10. shutdown grace: SIGTERM exits within the 5 s grace despite an
#      authenticated client holding its connection
#  11. supply chain: scripts/audit-deps.sh (cargo audit, cargo vet,
#      helper dependency count, rsa absence in helper graph)
#
# Debug builds on machines without the production team identity use the
# OV0_VAULT_DEV_TEAM_OU same-team relaxation (signing doc rule 3); the
# production DR pin (9RGW34CMA2) is unchanged in the source constants.
set -uo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
SRC_TAURI="$ROOT/src-tauri"
T="$(mktemp -d /tmp/vhgate.XXXXXX)"
RESULTS=()

cleanup() {
    [ -n "${HELPER_PID:-}" ] && kill "$HELPER_PID" 2>/dev/null
    [ -n "${HOLD_PID:-}" ] && kill "$HOLD_PID" 2>/dev/null
    rm -rf "$T"
}
trap cleanup EXIT

record() { # name, PASS|FAIL, evidence
    RESULTS+=("$1|$2|$3")
    printf '%s  %-58s %s\n' "$2" "$1" "$3"
}

fail() { record "$1" FAIL "${2:-}"; }

wait_for_socket() { # path, pid
    for _ in $(seq 1 100); do
        [ -S "$1" ] && return 0
        kill -0 "$2" 2>/dev/null || return 1
        sleep 0.05
    done
    return 1
}

start_helper() { # socket, vault_dir, [idle_secs] -> sets HELPER_PID
    local env_extra=()
    [ -n "${3:-}" ] && env_extra=(OV0_VAULT_IDLE_SECS="$3")
    env OV0_VAULT_SOCKET_PATH="$1" OV0_VAULT_DIR="$2" ${env_extra[@]+"${env_extra[@]}"} \
        "$HELPER_BIN" >"$T/helper.log" 2>&1 &
    HELPER_PID=$!
    wait_for_socket "$1" "$HELPER_PID"
}

echo "== Phase A gate (workdir $T)"

# --- 1. crate tests -------------------------------------------------------
if (cd "$SRC_TAURI" && cargo test -p source-vault-helper --quiet) >"$T/tests.log" 2>&1; then
    N=$(grep -c "test result: ok" "$T/tests.log")
    record "crate tests (cargo test -p source-vault-helper)" PASS "$(grep -oE '[0-9]+ passed' "$T/tests.log" | awk '{s+=$1} END {print s" tests"}')"
else
    fail "crate tests (cargo test -p source-vault-helper)" "see $T/tests.log"
    tail -5 "$T/tests.log"
fi

# --- 2. build, bundle, sign, verify ---------------------------------------
if bash "$ROOT/scripts/build-helper.sh" debug >"$T/build.log" 2>&1; then
    # shellcheck disable=SC1091
    . "$SRC_TAURI/target/helper-bundle/debug/build.env"
    record "build+sign+verify SourceVaultHelper.app" PASS "OU $TEAM_OU, deep-strict + DR + no entitlements"
else
    fail "build+sign+verify SourceVaultHelper.app" "see $T/build.log"
    tail -20 "$T/build.log"
    echo "FATAL: cannot continue without a signed helper"; exit 1
fi
HELPER_BIN="$BUNDLE/Contents/MacOS/source-vault-helper"
TS="$([[ "$IDENTITY" == *"Apple Development"* ]] && echo --timestamp=none || echo "")"

# --- client signing variants ----------------------------------------------
cp "$SRC_TAURI/target/debug/vault-test-client" "$T/legit"
codesign --force $TS --sign "$IDENTITY" -i com.racker.zero "$T/legit" 2>"$T/sign.log"
cp "$SRC_TAURI/target/debug/vault-test-client" "$T/clone-adhoc"
codesign --force -s - "$T/clone-adhoc" 2>>"$T/sign.log"
cp "$SRC_TAURI/target/debug/vault-test-client" "$T/clone-wrongid"
codesign --force $TS --sign "$IDENTITY" -i com.example.impostor "$T/clone-wrongid" 2>>"$T/sign.log"

SOCK1="$T/s1"; VAULT1="$T/v1"

# --- 3. legit client accepted, boot UNINITIALIZED --------------------------
if start_helper "$SOCK1" "$VAULT1"; then
    OUT=$("$T/legit" "$SOCK1" state app 2>&1)
    PERMS="$(stat -f %Lp "$SOCK1")/$(stat -f %Lp "$VAULT1")"
    if echo "$OUT" | grep -q "STATE=uninitialized"; then
        record "signed client accepted; boot state UNINITIALIZED" PASS "socket/dir perms $PERMS"
    else
        fail "signed client accepted; boot state UNINITIALIZED" "$OUT"
    fi
else
    fail "signed client accepted; boot state UNINITIALIZED" "helper did not start"
fi

# --- 4. restart with vault header -> LOCKED --------------------------------
mkdir -p "$VAULT1"; echo '{}' > "$VAULT1/header.json"
kill "$HELPER_PID" 2>/dev/null; wait "$HELPER_PID" 2>/dev/null
# The exited helper leaves its socket file behind (no unlink-on-exit), and
# wait_for_socket would otherwise accept that stale file and let the client
# race the new helper's unlink+bind (observed as ECONNREFUSED/ENOENT flakes).
rm -f "$SOCK1"
if start_helper "$SOCK1" "$VAULT1"; then
    OUT=$("$T/legit" "$SOCK1" state app 2>&1)
    if echo "$OUT" | grep -q "STATE=locked"; then
        record "helper restart leaves state machine at LOCKED" PASS "$OUT"
    else
        fail "helper restart leaves state machine at LOCKED" "$OUT"
    fi
else
    fail "helper restart leaves state machine at LOCKED" "helper did not restart"
fi

# --- 5. lock op -------------------------------------------------------------
OUT=$("$T/legit" "$SOCK1" handshake app 2>&1)
if echo "$OUT" | grep -q "LOCK_STATE=locked" && echo "$OUT" | grep -q "RESULT=OK"; then
    record "lock op honored" PASS "$(echo "$OUT" | tr '\n' ' ')"
else
    fail "lock op honored" "$OUT"
fi

# --- 6. ad-hoc client clone rejected ----------------------------------------
OUT=$("$T/clone-adhoc" "$SOCK1" expect-reject-client app 2>&1); RC=$?
[ $RC -eq 0 ] && record "peer-auth rejects ad-hoc client clone" PASS "$OUT" \
              || fail "peer-auth rejects ad-hoc client clone" "$OUT"

# --- 7. wrong-identifier client rejected ------------------------------------
OUT=$("$T/clone-wrongid" "$SOCK1" expect-reject-client app 2>&1); RC=$?
[ $RC -eq 0 ] && record "peer-auth rejects wrong-identifier client (same team)" PASS "$OUT" \
              || fail "peer-auth rejects wrong-identifier client (same team)" "$OUT"
kill "$HELPER_PID" 2>/dev/null; wait "$HELPER_PID" 2>/dev/null

# --- 8. ad-hoc helper clone rejected by client (reverse check) ---------------
cp "$SRC_TAURI/target/debug/source-vault-helper" "$T/helper-adhoc"
codesign --force -s - "$T/helper-adhoc" 2>>"$T/sign.log"
SOCK2="$T/s2"
OV0_VAULT_SOCKET_PATH="$SOCK2" OV0_VAULT_DIR="$T/v2" "$T/helper-adhoc" >"$T/helper-adhoc.log" 2>&1 &
HELPER_PID=$!
if wait_for_socket "$SOCK2" "$HELPER_PID"; then
    OUT=$("$T/legit" "$SOCK2" expect-reject-server app 2>&1); RC=$?
    [ $RC -eq 0 ] && record "client rejects ad-hoc helper clone (reverse SecCode)" PASS "$OUT" \
                  || fail "client rejects ad-hoc helper clone (reverse SecCode)" "$OUT"
else
    fail "client rejects ad-hoc helper clone (reverse SecCode)" "impostor helper did not start"
fi
kill "$HELPER_PID" 2>/dev/null; wait "$HELPER_PID" 2>/dev/null

# --- 9. idle exit -------------------------------------------------------------
SOCK3="$T/s3"
start_helper "$SOCK3" "$T/v3" 2
DEADLINE=$((SECONDS + 10)); EXITED=1
while [ $SECONDS -lt $DEADLINE ]; do
    kill -0 "$HELPER_PID" 2>/dev/null || { EXITED=0; break; }
    sleep 0.2
done
[ $EXITED -eq 0 ] && record "idle exit with zero clients (30 min spec value; 2 s test override)" PASS "exited" \
                  || fail "idle exit with zero clients (30 min spec value; 2 s test override)" "still running"
wait "$HELPER_PID" 2>/dev/null

# --- 10. SIGTERM grace with attached authenticated client ----------------------
SOCK4="$T/s4"
if start_helper "$SOCK4" "$T/v4"; then
    "$T/legit" "$SOCK4" hold 30 app >"$T/hold.log" 2>&1 &
    HOLD_PID=$!
    sleep 0.5
    kill -TERM "$HELPER_PID" 2>/dev/null
    DEADLINE=$((SECONDS + 8)); EXITED=1
    while [ $SECONDS -lt $DEADLINE ]; do
        kill -0 "$HELPER_PID" 2>/dev/null || { EXITED=0; break; }
        sleep 0.1
    done
    if [ $EXITED -eq 0 ] && kill -0 "$HOLD_PID" 2>/dev/null; then
        record "SIGTERM exits within 5 s grace while client attached" PASS "client still attached at exit"
    else
        fail "SIGTERM exits within 5 s grace while client attached" "exited=$EXITED client_alive=$(kill -0 "$HOLD_PID" 2>/dev/null && echo yes || echo no)"
    fi
    kill "$HOLD_PID" 2>/dev/null; wait "$HOLD_PID" 2>/dev/null; wait "$HELPER_PID" 2>/dev/null
else
    fail "SIGTERM exits within 5 s grace while client attached" "helper did not start"
fi

# --- 11. supply chain ----------------------------------------------------------
OUT=$(bash "$ROOT/scripts/audit-deps.sh" 2>&1); RC=$?
[ $RC -eq 0 ] && record "supply chain (audit, vet, helper deps, rsa absence)" PASS "$(echo "$OUT" | grep -E 'dependency count|rsa' | tr '\n' '; ')" \
              || fail "supply chain (audit, vet, helper deps, rsa absence)" "$(echo "$OUT" | tail -3 | tr '\n' ' ')"

# --- summary ---------------------------------------------------------------------
echo
FAILS=0
for r in "${RESULTS[@]}"; do
    case "$r" in *"|FAIL|"*) FAILS=$((FAILS+1)) ;; esac
done
if [ "$FAILS" -eq 0 ]; then
    echo "PHASE A GATE: PASS (${#RESULTS[@]} checks)"
    exit 0
else
    echo "PHASE A GATE: FAIL ($FAILS of ${#RESULTS[@]} checks failed)"
    exit 1
fi
