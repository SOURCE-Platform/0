#!/usr/bin/env bash
# Build, bundle, sign, and verify SourceVaultHelper.app (spec §18 Phase A).
#
# Produces src-tauri/target/helper-bundle/<profile>/SourceVaultHelper.app:
# an LSUIElement bundle with identifier com.racker.zero.vault-helper, signed
# with hardened runtime and NO entitlements (signing strategy doc: the
# helper must never carry disable-library-validation; it links no
# third-party dylibs).
#
# Identity selection:
#   1. $OV0_VAULT_SIGN_IDENTITY, if set;
#   2. an identity whose OU is the production team 9RGW34CMA2;
#   3. otherwise the first "Apple Development"/"Developer ID" identity,
#      which is only accepted for debug builds (same-team relaxation,
#      signing doc rule 3 — never "any process").
#
# When a non-production team OU is used, the compile-time env var
# OV0_VAULT_DEV_TEAM_OU is exported for the cargo build so the peer-auth
# designated requirements pin that OU (debug builds only; release refuses).
#
# Usage: scripts/build-helper.sh [debug|release]
set -euo pipefail

PROFILE="${1:-debug}"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
SRC_TAURI="$ROOT/src-tauri"
PROD_TEAM_OU="9RGW34CMA2"
HELPER_ID="com.racker.zero.vault-helper"

case "$PROFILE" in
    debug)   CARGO_PROFILE_ARGS=() ;;
    release) CARGO_PROFILE_ARGS=(--release) ;;
    *) echo "usage: $0 [debug|release]" >&2; exit 2 ;;
esac

# Leaf-certificate team OU for an identity name. The parenthetical in an
# identity's display name is the cert UID, NOT necessarily the team OU, so
# the OU must be read from the certificate itself.
identity_ou() {
    security find-certificate -c "$1" -p 2>/dev/null \
        | openssl x509 -noout -subject 2>/dev/null \
        | sed -E 's/.*OU=([^,]+).*/\1/'
}

# Prints "identity-name|team-ou".
find_identity() {
    if [ -n "${OV0_VAULT_SIGN_IDENTITY:-}" ]; then
        echo "$OV0_VAULT_SIGN_IDENTITY|$(identity_ou "$OV0_VAULT_SIGN_IDENTITY")"
        return
    fi
    local identities name ou first=""
    identities="$(security find-identity -v -p codesigning | sed -E 's/^ *[0-9]+\) [A-F0-9]+ "([^"]+)".*/\1/' | grep -E "Apple Development|Developer ID" || true)"
    while IFS= read -r name; do
        [ -z "$name" ] && continue
        ou="$(identity_ou "$name")"
        if [ "$ou" = "$PROD_TEAM_OU" ]; then
            echo "$name|$ou"
            return
        fi
        [ -z "$first" ] && first="$name|$ou"
    done <<< "$identities"
    echo "$first"
}

PICKED="$(find_identity)"
IDENTITY="${PICKED%|*}"
TEAM_OU="${PICKED##*|}"
if [ -z "$IDENTITY" ] || [ -z "$TEAM_OU" ] || [ "$TEAM_OU" = "$IDENTITY" ]; then
    echo "error: no codesigning identity found" >&2
    exit 2
fi

if [ "$TEAM_OU" != "$PROD_TEAM_OU" ]; then
    if [ "$PROFILE" = "release" ]; then
        echo "error: release builds require the production team identity (OU $PROD_TEAM_OU);" >&2
        echo "       only '$IDENTITY' (OU $TEAM_OU) is available." >&2
        exit 3
    fi
    echo "note: production identity OU $PROD_TEAM_OU not present; using development" >&2
    echo "      identity '$IDENTITY' (OU $TEAM_OU) for a DEBUG build only." >&2
    export OV0_VAULT_DEV_TEAM_OU="$TEAM_OU"
else
    unset OV0_VAULT_DEV_TEAM_OU 2>/dev/null || true
fi

# §2.11: release helpers are compiled panic=abort (process death is the
# zeroization of last resort) without debug info. panic is build-graph
# global in Cargo, so this rides on RUSTFLAGS of the release invocation
# rather than the workspace profile (which the main app shares).
if [ "$PROFILE" = "release" ]; then
    export RUSTFLAGS="${RUSTFLAGS:-} -C panic=abort -C debug=0"
fi

echo "==> building source-vault-helper ($PROFILE, OU $TEAM_OU)"
(cd "$SRC_TAURI" && cargo build -p source-vault-helper ${CARGO_PROFILE_ARGS[@]+"${CARGO_PROFILE_ARGS[@]}"})

BIN="$SRC_TAURI/target/$PROFILE/source-vault-helper"
BUNDLE_DIR="$SRC_TAURI/target/helper-bundle/$PROFILE"
BUNDLE="$BUNDLE_DIR/SourceVaultHelper.app"
rm -rf "$BUNDLE"
mkdir -p "$BUNDLE/Contents/MacOS"
cp "$BIN" "$BUNDLE/Contents/MacOS/source-vault-helper"
cp "$SRC_TAURI/vault-helper/Info.plist" "$BUNDLE/Contents/Info.plist"

SIGN_ARGS=(--force --options runtime --sign "$IDENTITY")
if echo "$IDENTITY" | grep -q "Apple Development"; then
    SIGN_ARGS+=(--timestamp=none) # dev cert: no network timestamp needed
fi
echo "==> signing $BUNDLE"
codesign "${SIGN_ARGS[@]}" "$BUNDLE"

echo "==> verifying signature"
codesign --verify --deep --strict "$BUNDLE"

DR="anchor apple generic and certificate leaf[subject.OU] = \"$TEAM_OU\" and identifier \"$HELPER_ID\""
echo "==> verifying designated requirement: $DR"
codesign --verify "-R=$DR" "$BUNDLE"

echo "==> verifying entitlements are absent"
if codesign -d --entitlements :- "$BUNDLE" 2>/dev/null | grep -q "<key>"; then
    echo "error: helper must carry no entitlements" >&2
    exit 4
fi

mkdir -p "$BUNDLE_DIR"
cat > "$BUNDLE_DIR/build.env" <<EOF
TEAM_OU="$TEAM_OU"
IDENTITY="$IDENTITY"
PROFILE="$PROFILE"
BUNDLE="$BUNDLE"
EOF

echo "OK: $BUNDLE"
echo "    identity: $IDENTITY"
echo "    team OU:  $TEAM_OU"
