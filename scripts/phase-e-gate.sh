#!/usr/bin/env bash
# Phase E gate (credential-vault-implementation-spec.md v0.3.1 §18 Phase
# E): device identity + enrollment.
#
#   1. Phase E helper tests: Secure Enclave identity + envelopes (DV/EV),
#      §5 enrollment end to end (EN-01…EN-08), revocation + mandatory VK
#      rotation (RC-01/RC-02), §2.8 device-envelope unlock (DU-01…DU-06),
#      cross-language vectors incl. XV-ENROLL
#   2. full helper suite (Phases A–E, no fail-fast)
#   3. file-length audit, plus the §2.12 bridge cap (≤ 200 lines)
#   4. debug helper build + sign + verify (now links the Swift bridge)
#   5. release helper: every OV0_VAULT_* debug override compiled out
#   6. signed E2E over the real socket: setup writes a genesis entry
#      signed by this Mac's Secure Enclave key → list_devices →
#      begin_enrollment mints a 26-character single-use secret →
#      cancel_enrollment leaves no registry trace
#   7. no VK / envelope plaintext in any IPC frame or helper log
#   8. main-app IPC surface: no RK/MP-bearing command argument (UI-05)
#   9. main app cargo check + frontend build
#  10. supply chain — including that Phase E added no Rust crypto crate
#      (HPKE comes from Apple CryptoKit through the §2.12 bridge)
#  11. iPhone: SourceMobile builds, its XV-ENROLL vectors agree with the
#      Mac's derivations, and the revocation-status rules hold
#  12. Phase D gate regression (which runs C, B and A)
#
# Synthetic vaults and synthetic credentials only.
set -uo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
SRC_TAURI="$ROOT/src-tauri"
MOBILE="${SOURCE_MOBILE_DIR:-$HOME/Documents/source mobile/SourceMobile}"
T="$(mktemp -d /tmp/vhgate-e.XXXXXX)"
KC_PREFIX="ov0gatee-$$-"
RESULTS=()
MP="synthetic-gate-e-master-password-0001"

cleanup() {
    [ -n "${HELPER_PID:-}" ] && kill "$HELPER_PID" 2>/dev/null
    for svc in com.racker.zero.vault.state com.racker.zero.vault.helper-prefs; do
        security delete-generic-password -s "${KC_PREFIX}e1-${svc}" >/dev/null 2>&1
    done
    # The gate vault owns Secure Enclave keys; destroy them with it.
    TAG=$(python3 -c "import json,sys;print(json.load(open('$T/e1/device.json'))['key_tag'])" 2>/dev/null)
    if [ -n "${TAG:-}" ]; then
        for role in signing agreement; do
            security delete-generic-password -s "com.racker.zero.vault.se-$role" -a "$TAG" >/dev/null 2>&1
        done
    fi
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

echo "== Phase E gate (workdir $T)"
cd "$SRC_TAURI" || exit 1

# --- 1. Phase E tests ---------------------------------------------------------------------------
E_TESTS="--test device_identity --test enrollment --test revocation --test device_unlock --test xv_vectors"
# shellcheck disable=SC2086
cargo test -p source-vault-helper --no-fail-fast $E_TESTS >"$T/e-tests.log" 2>&1
E_PASS=$(grep -E "^test result" "$T/e-tests.log" | awk '{s+=$4} END {print s+0}')
E_FAIL=$(grep -E "^test result" "$T/e-tests.log" | awk '{s+=$6} END {print s+0}')
check "Phase E tests (SE identity, envelopes, enrollment, revocation)" "$E_PASS passed, $E_FAIL failed" \
    [ "$E_FAIL" -eq 0 -a "$E_PASS" -gt 0 ]

# --- 2. full helper suite -----------------------------------------------------------------------
cargo test -p source-vault-helper --no-fail-fast >"$T/suite.log" 2>&1
S_PASS=$(grep -E "^test result" "$T/suite.log" | awk '{s+=$4} END {print s+0}')
S_FAIL=$(grep -E "^test result" "$T/suite.log" | awk '{s+=$6} END {print s+0}')
check "full helper suite (Phases A–E)" "$S_PASS passed, $S_FAIL failed" [ "$S_FAIL" -eq 0 -a "$S_PASS" -gt 0 ]

# --- 3. file lengths + §2.12 bridge cap ---------------------------------------------------------
BRIDGE="$SRC_TAURI/vault-apple-crypto/Sources/VaultAppleCrypto/Bridge.swift"
BRIDGE_LINES=$(wc -l < "$BRIDGE" | tr -d ' ')
if (cd "$ROOT" && node scripts/check-file-lengths.mjs) >"$T/lengths.log" 2>&1 && [ "$BRIDGE_LINES" -le 200 ]; then
    record "file-length audit (≤350) + §2.12 bridge (≤200)" PASS "bridge $BRIDGE_LINES lines"
else
    fail "file-length audit (≤350) + §2.12 bridge (≤200)" "bridge $BRIDGE_LINES lines; $(tail -3 "$T/lengths.log" | tr '\n' ' ')"
fi

# --- 4. debug helper build + sign ---------------------------------------------------------------
if bash "$ROOT/scripts/build-helper.sh" debug >"$T/build.log" 2>&1; then
    # shellcheck disable=SC1091
    . "$SRC_TAURI/target/helper-bundle/debug/build.env"
    record "debug helper build+sign+verify (with Swift bridge)" PASS "OU $TEAM_OU, deep-strict + DR"
else
    fail "debug helper build+sign+verify (with Swift bridge)" "see build.log"; tail -20 "$T/build.log"; exit 1
fi
HELPER_BIN="$BUNDLE/Contents/MacOS/source-vault-helper"
TS="$([[ "$IDENTITY" == *"Apple Development"* ]] && echo --timestamp=none || echo "")"
CLIENT="$T/legit"
cp "$SRC_TAURI/target/debug/vault-test-client" "$CLIENT"
codesign --force $TS --sign "$IDENTITY" -i com.racker.zero "$CLIENT" 2>"$T/sign.log"

# --- 5. release: overrides compiled out ---------------------------------------------------------
if cargo build -q -p source-vault-helper --release --bin source-vault-helper >"$T/release.log" 2>&1; then
    LEAKS=$(strings "$SRC_TAURI/target/release/source-vault-helper" | grep -oE 'OV0_VAULT_[A-Z_]+' | sort -u | tr '\n' ' ')
    check "release helper: all OV0_VAULT_* overrides compiled out" "${LEAKS:-none present}" [ -z "$LEAKS" ]
else
    fail "release helper: all OV0_VAULT_* overrides compiled out" "release build failed"
fi

SOCK="$T/s"; OPLOG="$T/ops.log"

# --- 6. signed E2E: SE genesis, devices, enrollment session -------------------------------------
start_helper "$T/e1" OV0_VAULT_PANEL_SCRIPT="submit:$MP" \
    || fail "E2E device identity + enrollment session" "helper did not start"
OUT_SETUP=$(c setup)
OUT_UNLOCK=$(c unlock-mp)
OUT_DEV=$(c raw '{"op":"list_devices"}')
OUT_BEGIN=$(c raw '{"op":"begin_enrollment","fp":"aa55aa55aa55aa55aa55aa55aa55aa55aa55aa55aa55aa55aa55aa55aa55aa55"}')
OUT_CANCEL=$(c raw '{"op":"cancel_enrollment"}')
stop_helper
SECRET=$(echo "$OUT_BEGIN" | sed -n 's/^RESP=//p' | python3 -c "import sys,json;print(json.load(sys.stdin).get('secret',''))" 2>/dev/null)
ENTRIES=$(grep -c tlv "$T/e1/registry.json" 2>/dev/null || echo 0)
SELF_DEV=$(python3 -c "
import json,sys
d=json.load(open('$T/e1/device.json'))
print(len(bytes.fromhex(d['sign_pub'])), len(bytes.fromhex(d['agree_pub'])), d['sign_pub']!=d['agree_pub'])" 2>/dev/null)
WHY=""
has "$OUT_SETUP" "OP_OK state=locked" || WHY+="setup:$(first "$OUT_SETUP") "
has "$OUT_UNLOCK" "OP_OK state=unlocked" || WHY+="unlock:$(first "$OUT_UNLOCK") "
[ "$ENTRIES" = "1" ] || WHY+="registry-entries=$ENTRIES "
[ "$SELF_DEV" = "65 65 True" ] || WHY+="device.json=$SELF_DEV "
has "$OUT_DEV" '"self":true' || WHY+="list_devices:$(first "$OUT_DEV") "
[ "${#SECRET}" = "26" ] || WHY+="secret-len=${#SECRET} "
has "$OUT_CANCEL" "OP_OK" || WHY+="cancel:$(first "$OUT_CANCEL") "
[ "$(grep -c tlv "$T/e1/registry.json" 2>/dev/null || echo 0)" = "1" ] || WHY+="cancelled-session-left-a-trace "
verdict "E2E: SE genesis entry, list_devices, enrollment session" \
    "registry entries $ENTRIES; device keys 65/65 distinct; secret ${#SECRET} chars; cancel leaves no trace"

# --- 7. no VK / envelope plaintext on the wire ---------------------------------------------------
WHY=""
grep -qE '"vk"|"device_backup_cred"' "$OPLOG" "$T/helper.log" && WHY+="key-field-in-transcript "
grep -q "$MP" "$OPLOG" "$T/helper.log" && WHY+="MP-in-transcript "
verdict "no VK / backup credential / MP in any IPC frame or log" "transcript scanned for vk, device_backup_cred, MP"

# --- 8. main-app IPC surface ---------------------------------------------------------------------
BAD=$(grep -nE '\b(words|mnemonic|recovery_key|rk_bytes|master_password|passphrase|vk|device_backup_cred)\s*[:=?]' \
    "$SRC_TAURI/src/app/commands/vault.rs" "$ROOT/src/lib/vault.ts" | grep -vE '^\S+:[0-9]+:\s*(//|\*|/\*\*)' | head -3)
check "main-app IPC surface: no key-bearing command argument (UI-05)" "${BAD:-none}" [ -z "$BAD" ]

# --- 9. main app + frontend ----------------------------------------------------------------------
if cargo check -q -p SOURCE >"$T/main.log" 2>&1 && (cd "$ROOT" && npm run build) >"$T/fe.log" 2>&1; then
    record "main app cargo check + frontend build" PASS "green"
else
    fail "main app cargo check + frontend build" "$(grep -E '^error' "$T/main.log" "$T/fe.log" | head -2 | tr '\n' ' ')"
fi

# --- 10. supply chain ----------------------------------------------------------------------------
OUT=$(bash "$ROOT/scripts/audit-deps.sh" 2>&1); RC=$?
HPKE_CRATE=$(cd "$SRC_TAURI" && cargo tree -p source-vault-helper 2>/dev/null | grep -cE '^\s*[├└|`-]*\s*hpke ')
if [ $RC -eq 0 ] && [ "$HPKE_CRATE" = "0" ]; then
    record "supply chain (audit, vet, no new crypto crate for HPKE)" PASS \
        "$(echo "$OUT" | grep -E 'dependency count' | tr '\n' '; ')hpke crate absent (Apple CryptoKit via §2.12 bridge)"
else
    fail "supply chain (audit, vet, no new crypto crate for HPKE)" "hpke=$HPKE_CRATE; $(echo "$OUT" | tail -3 | tr '\n' ' ')"
fi

# --- 11. iPhone: build + cross-language vectors ---------------------------------------------------
if [ -d "$MOBILE" ] && command -v xcodegen >/dev/null 2>&1; then
    (cd "$MOBILE" && xcodegen generate) >"$T/ios-gen.log" 2>&1
    SIM=$(xcrun simctl list devices available 2>/dev/null | grep -oE 'iPhone [0-9]+[a-zA-Z]*( Pro( Max)?)?' | head -1)
    if (cd "$MOBILE" && xcodebuild test -project SourceMobile.xcodeproj -scheme SourceMobile \
            -destination "platform=iOS Simulator,name=$SIM" \
            -only-testing:SourceMobileTests CODE_SIGNING_ALLOWED=NO) >"$T/ios.log" 2>&1; then
        # xcodebuild prints "Test run with N tests ... passed"; count from
        # that rather than per-test lines, whose leading glyph is
        # multi-byte and does not match a single-character pattern.
        IOS_PASS=$(sed -nE 's/.*Test run with ([0-9]+) tests.*passed.*/\1/p' "$T/ios.log" | tail -1)
        if [ -z "$IOS_PASS" ] || [ "$IOS_PASS" -eq 0 ]; then
            fail "iPhone: XV-ENROLL vectors + revocation-status rules" \
                "xcodebuild succeeded but reported no test count — evidence unusable"
        else
            record "iPhone: XV-ENROLL vectors + revocation-status rules" PASS "$IOS_PASS tests on $SIM"
        fi
    else
        fail "iPhone: XV-ENROLL vectors + revocation-status rules" "$(grep -E 'error:|failed' "$T/ios.log" | head -2 | tr '\n' ' ')"
    fi
else
    fail "iPhone: XV-ENROLL vectors + revocation-status rules" "SKIPPED — SourceMobile repo or xcodegen not present"
fi

# --- 12. Phase D regression (includes C, B, A) ----------------------------------------------------
if [ "${PHASE_E_SKIP_REGRESSION:-0}" = "1" ]; then
    fail "Phase D gate regression (incl. C, B, A)" "SKIPPED — not a gate run of record"
else
    echo "   (Phase D regression, which runs Phase C, B and A)"
    OUT=$(bash "$ROOT/scripts/phase-d-gate.sh" 2>&1); RC=$?
    echo "$OUT" > "$T/phase-d.log"
    [ $RC -eq 0 ] && record "Phase D gate regression (incl. C, B, A)" PASS "$(echo "$OUT" | tail -1)" \
                  || fail "Phase D gate regression (incl. C, B, A)" "$(echo "$OUT" | grep FAIL | head -3 | tr '\n' ' ')"
fi

echo
FAILS=0
for r in "${RESULTS[@]}"; do case "$r" in *"|FAIL|"*) FAILS=$((FAILS+1)) ;; esac; done
if [ "$FAILS" -eq 0 ]; then echo "PHASE E GATE: PASS (${#RESULTS[@]} checks)"; exit 0; fi
echo "PHASE E GATE: FAIL ($FAILS of ${#RESULTS[@]} checks failed)"; exit 1
