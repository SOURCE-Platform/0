# SOURCE Vault — status and how to finish it

**Paused on 2026-10-18 by the owner.** The vault is built and reviewed but
**not finished**: do not store real passwords, cards or notes in it. The
Vault tab shows a warning that links here. This document is for whoever
picks the work up — a person or an agent — and is meant to be enough on
its own.

## 1. In plain words

SOURCE Vault is SOURCE's own password manager: a separate, code-signed
helper on the Mac that holds the encrypted vault, a separate SOURCE Vault
iPhone app, a Chrome extension that fills logins, an encrypted backup on a
provider, and an importer for Dashlane exports. Everything is built and has
been through one long security review; the review's fixes are in. What is
left is mostly **checking and paperwork that needs the owner or real
hardware**, plus a few paid accounts — not new features.

Why build it at all when Apple Passwords exists: the owner wants full
control over the experience, and the vault is meant to join the coming
**multimodal identity system** in SOURCE. Keep that direction in mind when
resuming; nothing below blocks it.

## 2. What exists

| Area | Where | State |
|---|---|---|
| Specification (normative) | `docs/security/credential-vault-implementation-spec.md` (v0.6; §23 describes what was actually built) | current |
| Security architecture | `docs/security/credential-vault-security-architecture.md` | current |
| Final review: every finding and what happened to it | `docs/security/final-review.md` | current — **read this second** |
| Phase notes (what each phase built, what it carried) | `docs/security/phase-*-design.md`, `phase-*-verification.md` | current |
| Mac helper (holds the vault) | `src-tauri/vault-helper/` | built |
| Shared vault engine (Mac + iPhone) | `src-tauri/vault-engine/`, `src-tauri/vault-proto/` | built |
| iPhone app (SOURCE Vault) | `ios/SourceVault/`, engine via `src-tauri/vault-ffi/` | built, simulator-tested |
| Backup provider | `src-tauri/vault-provider-core/`, `vault-coordinator/` | built; not deployed |
| Chrome extension + its connector | `browser-extension/`, `src-tauri/vault-nm-host/` | built; not published |
| Dashlane import, secure notes | `src-tauri/vault-engine/src/import/`, Vault tab | built |
| Mac UI | `src/components/vault/`, `src/lib/vault*.ts` | built |

Phases A–F (helper, encryption, recovery, devices, backup), F.2 (iPhone
engine, Mac⇄iPhone sync, adding a replacement Mac from the iPhone), G
(approve on iPhone), H (Chrome), I (Dashlane import, secure notes) are
done.

## 3. Owner decisions already made (do not reopen)

All in the spec and in `final-review.md`; the ones that most affect what
is left:

- Reviews happen once at the end, not per step (2026-10-16). The final
  review ran (2026-10-17); one short re-review of its fixes ran
  (2026-10-18, results in `final-review.md`).
- A damaged item stops every key change and is named; nothing is set
  aside silently (2026-10-17).
- The browser fills **logins only** in v1; cards are reveal-only.
- Secure Notes are in v1. The Public Suffix List is vendored, no crate.
- The old vault code in SOURCE Mobile is retired; SOURCE Vault is the only
  iPhone vault.
- A second key change waits for the first to reach the backup ("sync
  first"). Recovery finishes before it locks. A Mac without Touch ID asks
  for the master password every time.

## 4. What is left, in order

### Step 1 — what the re-review left open (code, ~1–2 weeks)
The full list, with file names and suggested fixes, is the "Re-review
(2026-10-18)" section at the end of `final-review.md`. In short, in order:

1. **A damaged item can block removing a lost device for good.** The app
   names the item, but deleting or restoring it does not clear the block.
   *Ask the owner first* which remedy they want (repair from the backup's
   copy, or remove the damaged entries when the user deletes the item),
   then build it with a test that removal works afterwards.
2. **Program-identity check can be fooled by connect-then-exec.** Re-check
   the connecting program's identity on every message, both in the helper
   and in the Chrome connector.
3. **Spreadsheet (CSV) reader edge cases** after a malformed or huge row.
4. **Spec text** that still describes pre-build designs (list in
   `final-review.md`).
5. **Gate script honesty** — a few IDs are credited too broadly; a few
   required checks are not in the gate yet.
6. Smaller items, and the `rustls` advisory that fails `audit-deps.sh`.

Do them one at a time, each with a test; no review in between (owner
decision 2026-10-16); one bounded re-review of all of them at the end.

### Step 2 — the owner's one-minute Chrome check (NM-05 / NM-06)
Decides whether the Chrome connector can verify *who launched it* (layer 3
of spec §9.2). The probe is in `src-tauri/poc/nm-probe/`. In a throwaway
Chrome profile: `chrome://extensions` → Developer mode → Load unpacked →
`src-tauri/poc/nm-probe/extension`; the probe writes `/tmp/nm-probe.log`.
Record the verdict in `docs/security/phase-h-design.md`, then either build
layer 3 (`vault-nm-host`) or reduce the §9.2 claims (spec §16.10) — that
closes NM-01.

### Step 3 — evidence only a person or a device can give
`scripts/vault-id-gate.py` lists them as `MANUAL`; each needs a short note
in `docs/security/evidence/<ID>.md` (date, device, what was done, what was
seen; template: `evidence/MT-02.md`):

- **Real iPhones:** IO-01…07, PV-01, EV-03, AU-04 (a stolen iPhone with
  its passcode cannot open the vault), and the A15 floor on an older
  model.
- **Screen capture on the Mac:** CS-01…05 over every vault screen,
  including the newer ones (import, notes, join, approvals, the breach
  checklist).
- **Helper panels:** UI-02, UI-04; one real print of the recovery sheet
  (§19 row 29).

### Step 4 — paid accounts and release packaging (owner infrastructure)
- An **Apple Developer ID** (paid) for signing and notarizing SOURCE.app
  with the helper and `source-vault-nm-host` inside it (§19 row 21); the
  release bundle does not embed them yet.
- **Chrome Web Store** publication of the extension; put its id in
  `vault-nm-host/src/broker.rs` `PRODUCTION_ID` (release builds write no
  Chrome registration until then).
- **Hosting the backup provider** (see `vault-provider-deployment.md`,
  `vault-provider-hosting-options.md`) and a real APNs key for iPhone
  wake-ups.
- The independent review of the envelope path (§19 row 27).

### Step 5 — the full gate run (Phase J, spec §19)
```bash
bash scripts/phase-j-gate.sh --map-only
```
shows coverage without running anything (2026-10-18: 251 of 272 IDs; the
rest are Step 2–3 items and the stress run, which needs `--stress`). The real run, once:
```bash
bash scripts/phase-j-gate.sh --stress
```
It runs each test binary once, serially, with timeouts, and fails on any
missing, failed or skipped test or missing evidence note. Also run the
older gates `scripts/phase-a-gate.sh` … `phase-f-gate.sh` (signed builds;
they show real windows) and `scripts/audit-deps.sh`.
Known snag: the app crate's own unit tests (`cargo test -p SOURCE --lib`,
needed for UI-01/03, PW-06/12, CS-08) aborted in the dev environment on
2026-10-17 (SIGABRT under `scripts/dev-run-signed.sh`) — investigate first.

### Step 6 — the first real credential
Only when every §19 row is green. Remove `VaultUnfinishedBanner` from
`src/components/vault/VaultPage.tsx` at the same time.

## 5. Working rules (learned the hard way)

- **Synthetic data only** until Step 6 — never open a real Dashlane export
  or real passwords.
- **Disk:** the Mac fills up. A full app build is ~8.5 GB; prefer
  `cargo check -p SOURCE`, delete `src-tauri/target/debug` after big test
  runs, never leave a second target directory behind.
- **Tests:** always under `timeout`, one test binary at a time (a parallel
  run once deadlocked for two hours unnoticed); the FFI tests use a serial
  guard.
- **Commits:** stage files by explicit path — another agent works in the
  same repository.
- Keep every source file at or under 350 lines
  (`node scripts/check-file-lengths.mjs`).
- The owner wants plain-language updates; test IDs (PS-09, RE-11, …) are
  catalogue labels from spec §16/§22.16, not phases — explain them.
