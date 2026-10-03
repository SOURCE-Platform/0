#!/bin/bash
# Build vault-ffi for the SDK Xcode is building for, and put the static
# library where the app links it (spec v0.5 §22.2). Release app builds get
# a release engine — never a debug one (its debug-only overrides).
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../../.." && pwd)"
case "${PLATFORM_NAME:-iphonesimulator}" in
    iphoneos) TARGET=aarch64-apple-ios ;;
    iphonesimulator) TARGET=aarch64-apple-ios-sim ;;
    *) echo "unsupported platform ${PLATFORM_NAME}" >&2; exit 1 ;;
esac
PROFILE=debug
FLAGS=()
if [ "${CONFIGURATION:-Debug}" = "Release" ]; then PROFILE=release; FLAGS=(--release); fi
export PATH="$HOME/.cargo/bin:$PATH"
# Xcode's environment targets iOS; cargo picks its own SDK per target. The
# bundled SQLite must target the app's floor, not the SDK's (review VER-I6).
unset SDKROOT
export IPHONEOS_DEPLOYMENT_TARGET=17.0
(cd "$ROOT/src-tauri" && cargo build --locked -p vault-ffi --target "$TARGET" ${FLAGS[@]+"${FLAGS[@]}"})
OUT="${PROJECT_DIR:-$ROOT/ios/SourceVault}/build/rust/${PLATFORM_NAME:-iphonesimulator}"
mkdir -p "$OUT"
cp "$ROOT/src-tauri/target/$TARGET/$PROFILE/libvault_ffi.a" "$OUT/libvault_ffi.a"
