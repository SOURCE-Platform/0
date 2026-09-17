# macOS Signing and Hardening Strategy

Status: active prerequisite for the credential vault (security architecture
§15.3–15.4). This document fixes the signing model that the future vault
helper's peer authentication will build on.

## Identities

| Build | Identity | Runtime | Purpose |
|---|---|---|---|
| `tauri dev` | ad-hoc / Apple Development | not hardened | local iteration |
| Local release bundle | Apple Development (team) | hardened | dogfooding on this Mac |
| Distribution | Developer ID Application | hardened + notarized | any other machine |

- `src-tauri/tauri.conf.json` now sets `hardenedRuntime: true` and
  `entitlements: entitlements.plist` for every bundled build.
- Production releases must switch `bundle.macOS.signingIdentity` to the
  team's **Developer ID Application** certificate and notarize with
  `xcrun notarytool` (Tauri supports `APPLE_CERTIFICATE`, `APPLE_ID`/
  `APPLE_PASSWORD`/`APPLE_TEAM_ID` or API-key env vars for this).
- The signing identity must be **stable** — vault helper peer authentication
  pins a designated requirement, and rotating identities would break it.

## Entitlements policy

The main app carries exactly three hardened-runtime entitlements
(`src-tauri/entitlements.plist`):

- `com.apple.security.device.audio-input` — microphone capture.
- `com.apple.security.device.camera` — gaze calibration.
- `com.apple.security.cs.disable-library-validation` — required only because
  Homebrew's FFmpeg/Leptonica/Tesseract dylibs are signed by Homebrew, not by
  us. Revisit if those libraries are ever vendored and self-signed.

Anything not on this list fails code review. In particular, the app must
never gain `com.apple.security.cs.disable-executable-page-protection`,
`allow-unsigned-executable-memory`, or `get-task-allow` outside debug builds.

Screen Recording and Accessibility/Input Monitoring are TCC permissions, not
entitlements; they are unaffected by this change.

## The future vault helper

The helper (security architecture §15.3) is a **separately signed, minimal
binary** inside the app bundle. Rules that follow from this signing model:

1. Same team identity as the main app, hardened runtime, **no**
   `disable-library-validation` entitlement (it links no third-party dylibs).
2. Peer authentication on its IPC endpoint uses the client's audit token →
   `SecCodeCopyGuestWithAttributes` → `SecCodeCheckValidity` against a
   designated requirement of the form
   `anchor apple generic and certificate leaf[subject.OU] = "9RGW34CMA2" and identifier "com.racker.zero"`.
3. Debug builds may relax the designated requirement to same-team instead of
   a pinned identifier, but never to "any process".
4. XPC is an acceptable transport but not required; a Unix-domain socket with
   the peer check above provides the same isolation boundary for this threat
   model.

## Verification

- `codesign --verify --deep --strict --verbose=2 <app>` after every release
  build.
- `spctl --assess --type execute -vv <app>` for notarized artifacts.
- `codesign -d --entitlements - <app>` must show exactly the three
  entitlements above.
