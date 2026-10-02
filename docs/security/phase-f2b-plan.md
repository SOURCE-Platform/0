# Phase F.2b — one vault engine on the iPhone: plan

Date: 2026-10-02. Spec: v0.5 candidate §22.2 (F2-D1, one Rust engine;
FFI never-crosses list), §22.3 (F2-D2, separate SOURCE Vault app),
§22.10 (iOS platform rules), §22.17 milestone F.2b. Internal working
plan; the code it produces goes through the usual milestone review.

## Where things stand

- `vault-proto` (wire types, verification, the peer codec) already
  builds for `aarch64-apple-ios` with no change (checked 2026-10-02).
- `vault-helper` mixes the platform-neutral vault core with macOS-only
  pieces. macOS-only today: `panel/*` (AppKit secure panel),
  `vault/secure_ui.rs`, `la.rs` and `notify.rs` (objc2 macOS
  frameworks), `ipc/*` and `ffi/security.rs` (the helper's socket and
  code-signing peer checks), `main.rs`, and the Swift bridge linkage in
  `build.rs` (macOS static library).
- Everything else — `storage/`, `sync/`, `peer/`, `crypto/`,
  `registry/`, `backup/`, `recovery/`, `device/` (behind its SE calls),
  `keychain.rs` (Security.framework calls that exist on iOS too), and the
  op logic in `vault/` (already written against the `Deps` traits for
  panel, presence and events) — is the engine.

## Steps

1. **Extract `vault-engine`** (new workspace crate, no objc2/AppKit):
   move the platform-neutral modules out of `vault-helper` unchanged, and
   put the three platform services behind traits the engine receives:
   `SecureEnclave` (create keys, sign a digest the engine built, HPKE-open
   an engine-held envelope with a Touch ID / Face ID reason — the §22.2
   audited crossing (a)), `SecureEntry` (MP/RK entry and the RK sheet —
   crossings (b), (c)), and `Presence`. `vault-helper` becomes the macOS
   shell: IPC, AppKit panel, LA, the Swift bridge, and the engine. No
   behaviour change; the full helper and vault-tests suites must pass
   unchanged (this is the one place a broad run is warranted).
2. **iOS build of the engine**: `aarch64-apple-ios` and the simulator
   targets; `cargo vet` scope extended to them (§17.3); dependency count
   reported. Expected friction: `rusqlite` bundled SQLite (fine on iOS),
   `getrandom` (fine), Keychain item classes (`WhenUnlockedThisDeviceOnly`,
   own access group).
3. **C ABI and catalogue (§22.2)**: a small `vault-ffi` crate exporting
   only the catalogued entry points; `panic = "abort"`; a lint (like
   UI-05) failing on any extra exported symbol (FFI-01). No entry point
   returns VK, PK, `RK_bytes`, `sk_c`, or signs a caller digest.
4. **SOURCE Vault app shell** (separate Xcode target/app per F2-D2): its
   own bundle id and Keychain access group, no agent/audio code; Swift
   implementations of the three services (Face ID-bound agreement key per
   §22.4, signing key without biometry, secure text entry, capture
   hiding); QR pairing that stores the vault pin and the `peer_endpoint`
   token separately from any SOURCE Mobile pairing.
5. **First materialization** (§22.10 order) from the enrollment bundle,
   then the peer client (F.2c phone side) against the Mac's
   `/v1/vault/peer`.
6. **CryptoKit-only Swift test target** for XV-TLV, XV-PEER and
   XV-HPKE-SE vectors (independent of the engine), and the device tests
   asserted by name (IO-01…07, AU-04).

## What needs the owner

- Running the app on the physical iPhone: signing with the free Apple
  account (profiles last 7 days) and re-pairing the phone with SOURCE
  Vault.
- The two-minute Touch ID check on the Mac (from the Touch ID decision).

## Risks

- Step 1 is a large mechanical move; it is done in one commit with no
  logic changes and verified by the unchanged test suites before any
  iOS work builds on it.
- App size and build time grow with the Rust static library.

## Progress

- **Step 1 done (2026-10-03).** `vault-engine` holds `backup`, `crypto`,
  `device`, `enroll`, `errors`, `keychain`, `peer`, `recovery`,
  `registry`, `state`, `storage`, `sync`, `vault` and `test_support`,
  moved with `git mv` and no logic change; the §2.12 bridge build moved
  with them. `vault-helper` keeps the macOS shell (`ipc`, `panel`, `la`,
  `notify`, `log`, `ops`, `ffi`, `main`, the gate binaries) and re-exports
  the engine under its old paths, so the main app, `vault-tests` and every
  helper test compile unchanged. Finding: the engine already had no
  macOS-only dependency — the op logic was written against the `Deps`
  traits — and the bridge (CryptoKit, LocalAuthentication, Security) is
  the same on iOS, so the planned `SecureEnclave` / `SecureEntry` traits
  are not needed for the move; the iPhone app links the same bridge.
  The helper's dependency list shrank to what the shell itself uses
  (crypto and SQLite crates moved to the engine; three are test-only).

