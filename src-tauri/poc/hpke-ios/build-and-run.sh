#!/usr/bin/env bash
# Phase E0 (c): build the on-device HPKE/Secure-Enclave PoC and run it on a
# connected, unlocked iPhone. Requires xcodegen + Xcode signing. PoC only:
# nothing here is part of any product build.
set -euo pipefail
cd "$(dirname "$0")"
cp ../hpke-se/vectors/rfc9180-p256-sha256-chacha20poly1305.json App/vector.json
xcodegen generate
xcodebuild -project HpkeSePoC.xcodeproj -scheme HpkeSePoC \
    -destination 'generic/platform=iOS' -allowProvisioningUpdates build
DEVICE=$(xcrun devicectl list devices | awk '/iPhone/ {print $(NF-3); exit}')
APP=$(find ~/Library/Developer/Xcode/DerivedData/HpkeSePoC-*/Build/Products/Debug-iphoneos -maxdepth 1 -name '*.app' | head -1)
xcrun devicectl device install app --device "$DEVICE" "$APP"
xcrun devicectl device process launch --device "$DEVICE" test.synthetic.ov0.hpkepoc
echo "Tap Run on the device; uninstall afterwards with:"
echo "  xcrun devicectl device uninstall app --device $DEVICE test.synthetic.ov0.hpkepoc"
