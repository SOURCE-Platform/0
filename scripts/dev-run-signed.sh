#!/usr/bin/env bash
# Cargo runner for macOS dev builds (src-tauri/.cargo/config.toml).
#
# The vault helper only talks to a client signed by our team with the app
# identifier com.racker.zero (spec §1.4 peer auth). `tauri dev` / `cargo run`
# produce an ad-hoc signed SOURCE binary, which the helper correctly
# rejects, so the Vault tab would show "helper not available". This runner
# signs the SOURCE dev binary with the local Apple Development identity and
# the app identifier, then execs it. Every other binary (tests, tools) runs
# unchanged. Signing failures fall through to running the binary as-is.
set -uo pipefail

BIN="$1"
if [ "$(basename "$BIN")" = "SOURCE" ] && command -v codesign >/dev/null 2>&1; then
    IDENTITY="${OV0_DEV_SIGN_IDENTITY:-$(security find-identity -v -p codesigning 2>/dev/null \
        | grep -m1 'Apple Development' | awk '{print $2}')}"
    if [ -n "$IDENTITY" ]; then
        codesign --force --timestamp=none --sign "$IDENTITY" -i com.racker.zero "$BIN" 2>/dev/null \
            || echo "dev-run-signed: codesign failed; running ad-hoc (Vault tab will be unavailable)" >&2
    else
        echo "dev-run-signed: no Apple Development identity; running ad-hoc" >&2
    fi
fi
exec "$@"
