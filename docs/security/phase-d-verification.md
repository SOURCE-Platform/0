# Phase D Verification Report — Recovery Layer

Date: 2026-09-19. Spec: `credential-vault-implementation-spec.md` v0.3
with the dated recovery-order correction (Phase C.1), §18 Phase D.

Scope honored: Recovery Key generation, 24-word display/entry,
helper-owned RK UI, recovery-sheet rendering/printing, MP set/change
flows, VK rotation engine, recovery-epoch construction/verification,
corrected total-loss ordering, FsBackupStore rehearsals, historical
snapshot test, freshness/checkpoint logic, and the RC/RF/FR/BK tests
that run without a production provider.
Not started: Secure Enclave enrollment, production HPKE/envelopes,
iPhone enrollment/approval, production backup service/HTTP provider,
Chrome extension, Dashlane import, passkeys, real credentials.
`FsBackupStore` is a test/gate backend only; the app has no backup path.
Argon2id tuple unchanged; no keyboard tap added.

Status: **PHASE D GATE: PASS (12 checks)**, run of record 2026-09-19, including the Phase C gate (19/19), which runs Phase B (6/6) and Phase A (11/11) and the 10-minute TLV fuzz. One new blocker (§10) must be decided before Phase E.

## What shipped (helper, every file ≤ 350 lines)

| Area | Files |
|---|---|
| Rotation engine | `storage/rotation.rs`, `storage/rotation_journal.rs`, `storage/revision_rows.rs`, `storage/import_log.rs` |
| Registry | `registry/chain.rs` (§4.4 rules 1–8, §4.6), `registry/build.rs`, `registry/device.rs`, `registry/file.rs` |
| Backup format + rehearsal store | `backup/manifest.rs` (SignedManifest), `backup/index.rs`, `backup/object.rs` (§3.7), `backup/finalize.rs` (§11.8 body), `backup/snapshot.rs`, `backup/fs_store.rs`, `backup/fs_recovery.rs` |
| Recovery | `recovery/total_loss.rs` (corrected order), `recovery/creds.rs` (locators, `cred_mp`/`cred_rk`), `recovery/sheet.rs` (§11.7 checkpoint) |
| Ops + UI | `vault/create.rs` (setup writes both wraps), `vault/rk_ops.rs`, `vault/recovery_ops.rs`, `panel/rk_sheet.rs` (display + print window), RK entry panel |

Main app: `vault_unlock_with_recovery_key`, `vault_rotate_recovery_key`,
`vault_reset_master_password` (status only, no arguments); Vault tab
buttons "Use Recovery Key", "Replace Recovery Key", "Forgot master
password".

## 1. Tests and gate

`npm run gate:phase-d`, run of record 2026-09-19 on this machine:

| # | Check | Evidence |
|---|---|---|
| 1 | Phase D tests | **32 passed, 0 failed** — `vault_rotation` 5, `registry_epoch` 7, `recovery_total_loss` 5, `recovery_finalize` 7, `recovery_trusted` 3, `vault_rk_ops` 5 |
| 2 | full helper suite (A–D) | **146 passed, 0 failed** |
| 3 | file-length audit | all files ≤ 350 lines |
| 4 | debug helper build + sign + verify | OU 9RGW34CMA2, deep-strict + DR |
| 5 | release: debug overrides compiled out | no `OV0_VAULT_*` string (incl. the new `OV0_VAULT_SHEET_SCRIPT`) |
| 6 | signed E2E over the real socket | setup shows RK → RK unlock → add → `rotate_recovery_key` (`vk_generation` 1→2) → record intact → new RK unlocks |
| 7 | RK / MP absent from IPC | longest run of BIP-39 words: 4 in client frames, 1 in helper log (an RK is 24); MP absent |
| 8 | UI-04 live RK window | `NSWindowSharingNone`, capture bracket with RK title, on screen, no file/PDF, unacknowledged window committed nothing; **print leg not exercised — no printer configured** |
| 9 | main-app IPC surface lint | no RK/MP-bearing argument |
| 10 | main app + frontend | `cargo check` + `npm run build` green |
| 11 | supply chain | audit + vet green; 96 helper deps (< 120); no rsa |
| 12 | Phase C gate regression | PASS (19) — incl. Phase B PASS (6), Phase A 11/11, 10 min fuzz clean |

Phase C test expectations updated for Phase D behavior (not weakened):
`begin_recovery_unlock {kind:"rk"}` is now implemented, so OP-04 and gate
check 7 use a genuinely unknown kind (`"device"`) for `UNKNOWN_OP`, and
OP-04 adds the new ops to the bad-state matrix; OP-01 expects the RK
window's panel events after MP creation.

## 2. Recovery Key: generation, display, entry, print

- **Generation:** 32 bytes OsRng (`random_secret`) inside the helper; BIP-39
  24-word encoding in-house (Phase B, reference vectors).
- **Display:** `panel/rk_sheet.rs` — helper-owned AppKit window,
  `NSWindowSharingNone` (macOS excludes it from screen capture), bracketed
  by `secure_panel_visible` (title "Source Vault — Recovery Key") so Source's
  own capture suppression is up for the window's whole life, including the
  print dialog (the print operation runs modally *inside* the window's
  session; `visible:false` is emitted only after the window is closed).
- **Commit rule:** nothing is committed until the user presses "I've saved
  it". `setup_vault` with an unacknowledged window removes the vault;
  `rotate_recovery_key` with an unacknowledged window changes nothing
  (tests `refused_rk_sheet_at_setup_leaves_no_vault`, `rotate_recovery_key_op`;
  gate check 8).
- **Entry:** native `NSSecureTextField` panel; normalize + wordlist +
  checksum offline before use (§2.4). Bad checksum → `RECOVERY_KEY_INVALID`,
  wrong key → `WRONG_CREDENTIAL`, both leave the vault LOCKED.
- **Print:** `NSPrintOperation` straight from an in-memory `NSView` (the
  window's "Print…" button); no PDF or file is written by the helper.
  Gate check 8 shows the real window and verifies: capture bracket opened
  with the RK title, window on screen, no PDF anywhere under the run dir /
  user temp / user cache, no file in the helper's `TMPDIR`, and the
  unacknowledged window committed nothing. **The print leg itself was not
  exercised on this Mac: no printer is configured, and macOS refuses any
  print job (with a modal alert) until one is.** When a printer exists the
  gate runs a non-interactive print from the in-memory view with the job
  cancelled after rendering (`NSPrintCancelJob`). Capture suppression
  through a real print dialog follows structurally (the dialog runs inside
  the window's modal session, before `visible:false`), but has not been
  observed live.
- **Honest limits:** AppKit copies the words into `NSString`/label storage
  the helper cannot zeroize (labels are cleared on close, the Rust copy is
  `Zeroizing`). When the user prints for real, macOS's print system may
  spool the job (CUPS) outside the helper — no erasure is claimed for
  spooled print data. The window copy tells the user not to save a PDF,
  but the standard print dialog still offers "Save as PDF", which the
  helper cannot remove — a user who chooses it writes a plaintext PDF.

## 3. RK/MP plaintext never crosses IPC

- No op, response, or event field carries MP, RK bytes, RK words, or VK:
  new app commands take no arguments; the helper answers status codes.
- `tests/vault_rk_ops.rs` records every response and event across setup,
  RK unlock, rotation, and MP reset and asserts: no MP string, no run of
  the shown RK words, no `words`/`mnemonic`/`recovery_key`/`rk`/`vk`
  field. A frame that *claims* to carry words is ignored (the panel is the
  only input).
- Gate check 7 scans every frame the signed client received plus the
  helper log for runs of consecutive BIP-39 words (an RK is 24).
- Gate check 9 lints the Tauri command file and `src/lib/vault.ts` for any
  RK/MP-bearing argument.
- Test output never prints secrets (assertions compare in memory).

## 4. VK rotation and crash recovery

`storage/rotation.rs` (tests `tests/vault_rotation.rs`):
- new VK, `vk_generation + 1`; every revision (history included) re-sealed;
  content-committed `rev_hash`es re-derived parents-first and remapped in
  `parent_revs`, `record_tips`, `record_conflicts`;
- every revision opens under the new VK and **none** under the old one;
- MP wrap re-sealed (same MP) or fresh (new MP); RK wrap sealed for the kept
  or new RK, or removed; the new state refuses the old RK;
- `import_log` fingerprints recomputed under the new VK-derived key in the
  same transaction; identities preserved; old-key fingerprints differ;
- **crash matrix:** failure injected after DB staged, wraps staged,
  header staged, commit marker, first rename — the vault always reopens
  entirely old (before the marker) or entirely new (after), with MP and RK
  wraps matching the accepted state and no staged file left;
- **historical copies:** a pre-rotation copy of the directory still opens
  with the old RK (BK-10 at storage level; RC-06/07 do it via the backup
  store's retained generation).

## 5. recovery_epoch tamper / substitution / replay

`tests/registry_epoch.rs` (7 tests):
- RG-17: a valid epoch installs exactly the proof-bound device; the next
  entry verifies under its `sign_pub`, a look-alike key reusing its
  `device_id` does not.
- RG-08/RG-13…15: altering any bound field — `vault_id`, `prev_hash`,
  `manifest_hash`, `prior_epoch`, `epoch`, `device_id`, `sign_pub`,
  `agree_pub`, `platform`, `device_name`, `recovery_nonce`, `enrolled_at` —
  fails the proof *and* the chain. A proof keyed by any other VK fails.
- Wrong vault and wrong epoch are rejected even with otherwise valid
  proofs; an epoch cannot re-install an existing `device_id`.
- RG-09/RG-16: an epoch bound to a manifest the verifier has superseded →
  `MANIFEST_ROLLBACK`; a verbatim or re-linked replay is rejected.
- A verifier that lacks the bound VK never accepts the epoch on trust.
- RG-03/04/05/07/11: forged revocation, truncation, fork, self-signed
  enroll, revoked authorizer — all rejected.

## 6. FsBackupStore scenario results

Two simulated devices (software P-256 rehearsal identities), synthetic
vault with edits and a tombstone, production Argon2id tuple.

| Test | Scenario | Result |
|---|---|---|
| RC-03 | both devices lost, MP kept (user also has RK) | contents equal; `vk_generation` +1 exactly once; head advanced; new device credential active; epoch verifies; MP and kept RK open the fresh VK |
| RC-03b | MP-only recovery | a new RK is issued (see §9); old RK dead, new RK recovers |
| RC-04 | both devices lost, RK kept | new MP required and set; old MP dead; RK still works; contents equal |
| RC-05 | MP forgotten, trusted device | re-wrap without rotation; old MP dead locally and at the provider; RK unaffected |
| RC-06 / RC-07 | RK lost / suspected stolen | new RK + VK rotation; old RK refused on current state; retained previous generation still decrypts with old RK (BK-10) |
| RF-01, RF-08 | finalize | atomic; head/registry/credential installed together; everything opens only under the fresh VK; no second rotation |
| RF-02 | stale expected head | `FINALIZE_CONFLICT`; nothing changes; local rotated state discarded |
| RF-03, RF-04 | structurally invalid body, foreign signer | rejected; head unchanged |
| RF-05 | replay | byte-identical → stored result; different body → conflict |
| RF-06 | crash mid-finalize | old head authoritative; retry succeeds |
| RF-07, BK-13, BK-14 | after recovery | new device publishes; recovery credential cannot publish/re-register; revoked credential refused |
| FR-01 | preview | generation, date, item count available before completion |
| FR-02 | sheet checkpoint | matches / newer / different vault / fork evidence classified |
| FR-03 | stale-but-valid manifest served | recovery completes showing the served generation; a newer sheet shows "older than sheet"; a device with newer history rejects the stale epoch |
| BK-16 | re-registration | MP change re-registers MP locator/credential; RK change re-registers RK's; old ones fail |
| — | wrong MP / RK / email | `WRONG_CREDENTIAL`, no decryption result |

Not exercised (outside Phase D): RC-01/02/08 (device revocation needs
Phase E identities), BK-01…09/11/12/15 provider behaviors, BA-*.

## 7. Freshness and recovery-sheet behavior

The sheet shows `vault_id`, manifest generation, and an 8-hex registry
head prefix (§11.7). The recovery engine exposes the served state's
generation, date, and item count before completion and a comparison with
the sheet. No rollback *detection* is claimed on a fresh device; the stale
binding surfaces later on any device that saw newer history (FR-03). No
recovery wizard UI exists yet in the app (the fresh-device flow needs a
production provider, Phase F); the engine and its non-secret preview are
ready for it.

## 8. Dependency and audit changes

None. `objc2-app-kit` gains the `NSPrintOperation` and `NSPrintInfo`
feature flags (same crate and version, already audited); no lockfile
change, no new `cargo vet` entries, helper dependency count unchanged.

## 9. Deviations from v0.3

1. **Rotation crash safety** uses a roll-forward/roll-back journal
   (`rotation.commit` marker) instead of "re-run rotation from scratch":
   re-running needs the new VK, which a crash would lose.
2. **Revision-hash remap:** re-sealing changes every content-committed
   `rev_hash`; the engine remaps parents/tips/conflicts (spec silent).
3. **Backup object:** the reserved `flags` byte carries the tombstone bit;
   a fixed trailer carries `schema_version` and timestamps (needed to
   reopen the record AAD; §3.7 omits them).
4. **Object index** also names header, registry, and wrap objects
   (content-addressed), so the signed manifest authenticates them; the
   header carries salts a recovering device needs.
5. **FsBackupStore auth** models credential presentation by hash; §11.4
   HMAC signing, nonces, and the replay cache are Phase F (BK-12, BA-* not
   run).
6. **Rehearsal device identities** are software P-256 keys; Secure
   Enclave identities and the local genesis entry remain Phase E, so the
   app's own vault is not yet connected to any backup.
7. **RK rotation asks for the current MP** (re-sealing `password.wrap`
   under the new VK needs PK).
8. **MP-only total-loss recovery issues a new RK** unless the user also
   enters their RK: re-wrapping the RK needs RK bytes. Spec says "RK
   unchanged unless the user requests replacement".
9. **RK-path recovery requires a new MP:** the MP wrap cannot be rebuilt
   without PK. Spec says "MP unchanged material".
10. **`change_master_password {mode:"reset"}`** is a new op mode for
    scenario 5 on this Mac (the catalog has no op for it).
11. **Trusted-device ops don't publish or re-register** in the app (no
    provider); those provider calls are exercised in the rehearsals.
12. **Scenario 7 product copy** (cross-device banners, audit view) is not
    built; the mechanics are tested.
13. **Setup requires an acknowledged RK**; a dismissed window removes the
    new vault.

## Findings fixed during the gate

- **Intermittent Phase B test** `cr11_secret_vec_zeroizes_on_drop`
  (unchanged since Phase B) failed 2 of 30 runs and failed the first
  Phase D gate run inside the Phase A regression. It read a freed buffer
  and required every byte past 16 to be zero; another test thread can
  reuse the freed block in between. The secret itself never survived.
  The check now requires that no 8-byte fragment of the secret survives
  anywhere in the block (0 failures in 60 runs); a missing wipe still
  fails it.
- Gate script issues found on the first run (not product code): a
  malformed test condition, a print leg that cannot run without a
  configured printer (macOS shows a modal alert), and an IPC lint that
  matched user-facing copy.

## 10. New blocker

**Second total-loss recovery.** After one recovery, the registry holds a
`recovery_epoch` whose proof is keyed by the *pre-recovery* VK. A later
fresh device only ever learns the current VK, so it cannot verify that
older proof, and strict verification refuses the chain
(`second_recovery_blocked_pending_spec_decision`). Accepting unverifiable
epochs on trust would let a provider inject devices. Options for a spec
decision before Phase E: (a) persist the old VK's proof key under the new
VK so later devices can verify historical epochs, (b) have the recovered
device countersign/checkpoint the epoch once it holds the new VK, or (c)
treat pre-epoch history as verified by the epoch's successor signature.
Phase E must not start until this is decided.

Open item (not a blocker, owner decision): the standard macOS print
dialog's PDF menu ("Save as PDF", "Open in Preview") lets a user write the
RK sheet to a plaintext file. The helper creates no file itself; hiding or
restricting that menu would need a custom print panel.

Carried forward: cross-device Argon2id calibration (pre-Phase E); CS-03
re-verification when keystroke capture exists.

## Reproduce

```bash
npm run gate:phase-d
```

Requires the debug signing identity and cargo-fuzz + nightly (Phase B
regression). Shows the real Recovery Key window for about half a second.
