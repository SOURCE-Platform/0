# Final review — dispositions (2026-10-17)

The one long review the owner chose (2026-10-16: no reviews during
development, one before the first real credential). Frozen candidate:
`d59b94d`. Four independent reviewers in fresh contexts: security (core),
security (surfaces), spec consistency, implementation verification. The
lead checked every finding against the repository; this file records the
disposition of each. Fix commits: `b47bcd7`, `ff22921`, `620f4f8`,
`598c8c3`, `9a7ca36`, `413ca8b`, `f298db8`, `d6e9432` (and later ones
named below).

Legend: **Fixed** (commit, with a test unless noted) · **Spec** (spec
amendment, done or pending) · **Carried** (recorded, not fixed before the
first real credential, with the reason) · **Owner** (needs an owner
decision) · **Rejected** (finding not upheld, with the reason).

## Blockers

| ID | Finding | Disposition |
|---|---|---|
| VER-B1 / SPEC-I4 / core 3 | Browser saves bypass the §22.14 read-only rule | **Fixed** `b47bcd7` — the three ops are authoring ops; test `browser_saves_are_refused_while_behind` |
| VER-B2 / SPEC-I5 / SEC-O1 | Importer: oversized record swallowed the rest; read error taken as end of file; no damage detection | **Fixed** `b47bcd7` — record read to its real end with quotes tracked; `Row::Failed`; ZIP member CRC-32 and size checked before any row; damaged members reported; tests and fixtures `oversized-quoted.csv`, `damaged.zip` |
| SPEC-B4 (import_log half) | One unreadable `import_log` row blocks every rotation (and so revocation) | **Fixed** `620f4f8` — the row is dropped and counted; test `a_damaged_import_log_row_never_blocks_a_rotation` |
| SPEC-B4 (revision half) | One unreadable *revision* stops every rotation | **Owner** — see Q1 below |
| VER-B4 | Reverse-enrollment failure paths untested | **Fixed** `d6e9432` — RE-02, RE-05, RE-06 (pending revocation = pending rotation, not fresh), RE-07, RE-08. RE-04 (bundle negatives) and RE-10 are **Carried** to the gate work below; RE-12 contradicts design §11a and is removed from the design (Spec) |
| SPEC-B1 / VER-B3 | No F.2/G/H/I gate scripts; §19 IDs without tests; a gate run would pass vacuously | **Pending (in progress)** — gate scripts that assert every §19 ID by test name and fail on skips; the readiness table below lists what is missing |
| SPEC-B2 | Spec still says F.2e is unspecified | **Spec (pending)** |
| SPEC-B3 | §1.5 / §13.2 do not list the G/H/F.2e ops; ops 7/8 missing from §22.8 | **Spec (pending)**; the app class's access to `approval_request/take` and `push_wake_request` is recorded there |

## Important

| ID | Finding | Disposition |
|---|---|---|
| core 1 | Join cleanup could delete a vault's own SE keys via a planted tag | **Fixed** `ff22921` — keys deleted only by the session's own tag, never the root's; setup/recovery refused during a join; tests `renroll_cleanup.rs` |
| core 2 / VER-I2 | `renroll_finish` moved files in directory order | **Fixed** `ff22921` — `move_into_place` (header last), directory synced, boot sweep of a half move |
| SEC-I1 | Peer auth by pid: connect-then-exec of the signed app passed | **Fixed** `598c8c3` — audit token (`LOCAL_PEERTOKEN` → `kSecGuestAttributeAudit`) both directions; the signed-binary proof is phase-a-gate row 3/6/7/7b at the gate run |
| SPEC-I1 / VER-I3 | iPhone sheet did not show account or Mac | **Fixed** `9a7ca36` — account (title, username) when the phone's vault is open, the Mac's registry name always |
| SPEC-I10 / VER-I10 | Lock waited for the whole import | **Fixed** `620f4f8` — `LOCK_REQUESTED` checked between rows; test `a_lock_request_stops_the_import` |
| VER-I1 | Join bundle body read before the route proof | **Fixed** `9a7ca36` |
| VER-I4 | IPv4 literals through the PSL | **Fixed** `b47bcd7` |
| VER-I5 | Importer trimmed passwords and note bodies | **Fixed** `b47bcd7` |
| VER-I6 | BR-19 answered PRESENCE_DENIED; DA-02 NOT_AUTHORIZED untested | **Fixed** `b47bcd7`, `413ca8b` |
| VER-I7 | App class → `browser_*` refusal untested | **Fixed** `413ca8b` (`tests/ipc.rs`) |
| SPEC-I5 (delete offer) / VER-O7 | Trash offered even when items were left out | **Fixed** `b47bcd7` — `keep_export` withholds it in the UI and the command |
| SPEC-I2 | Phase G spec text stale (§7.1, AUTHORIZING, action 4, §2.9) | **Spec (pending)** |
| SPEC-I3 | Phase H spec text (§9.4 ASCII hosts, BR-15, XV-ORIGIN, iframes, op names, §9.9 rate limit scope) | **Spec (pending)**; rate limit is global (stricter than "per session") |
| SPEC-I6 | Phase I deliberate differences need amendments | **Spec (pending)** — drafts accepted as written by the reviewer, adjusted to the fixes (CRC now checked; keep_export) |
| SPEC-I7 / VER-I8 | flate2 family on bootstrap exemptions, not audited | **Owner** — see Q3 |
| SPEC-I8 | Gate script check 12 / UI-05 / PR-01 / EV-03 scope | **Pending** with the gate work |
| SPEC-I9 | Unsigned `GET /v1/vault/registry` route and SourceMobile's vault catch-up still live | **Owner** — see Q2 |
| VER-I9 | Spec not amended for what was built | **Spec (pending)** — same as SPEC-B2/B3/I2/I3/I6 |
| VER-I11 | Card fill and DA-10/DA-11 not built, not recorded | **Carried** — logins only in the browser for v1; recorded in `phase-h-design.md`; see Q4 |
| VER-I12 | nm-host links the whole helper crate (§17.2: serde only) | **Owner** — see Q3 |
| core 4 | A joined Mac has no exit if its enrollment never lands | **Carried** — rare (two phones racing); recovery path exists (delete the vault folder by hand); recorded |

## Minor (all fixed unless noted)

core 5 device name NFC/length (`f298db8`); core 6 lock sweeps join staging
(`ff22921`); VER-O1 save prompt names the site; VER-O3 no iPhone path
without an iPhone; VER-O8 `F_GETPATH`; VER-O10 popup ops not from content
scripts; VER-O11 simulator variable only in the Simulator; SPEC-O4 banner
wording; SPEC-O9 note body not selectable; SPEC-O6 stale comment
(`413ca8b`); VER-O4 NFC titles, local `allow_http` (`f298db8`; logins with
no URL are still skipped — §8.1 needs 1–20 URLs; the report says so and
the export is kept); VER-O5 fuzz lockfile (`f298db8`). **Carried:** SEC-O2
(an approved update's value is not attested — the owner-approved §9.8
model), SPEC-O5 (origin > 253 chars cannot use the iPhone path), SPEC-O8
(route proof is computable by main; no §15 backoff for a wrong MP at join),
VER-O2 (app-class approval ops), VER-O9 (syncs not held during authorize;
fails closed after the ACK), VER-O12 (length audit skips extension/fuzz),
VER-O13 "all-dialects" single CSV fixture, VER-O14 (`luhn_ok` placement),
§10.7's "≤ 2× allocation" fuzz invariant.

## Owner questions

- **Q1 (SPEC-B4, revision half).** A vault record that can no longer be
  decrypted (disk damage, tampering) makes every key change fail — including
  revoking a lost device. Options: (a) keep failing, but name the record
  and offer to restore it from the backup first (today: fails, unnamed);
  (b) the key change sets the unreadable record aside (it stays unreadable
  forever) and goes through.
- **Q2 (SPEC-I9).** The old phone-vault path in SourceMobile and the
  unsigned registry route on the Mac are still live; the spec says they
  were retired with F.2c. Retire them now (this touches the SourceMobile
  app, which the other agent also works on)?
- **Q3 (SPEC-I7, VER-I12).** Dependency rules: write review notes for the
  five deflate crates (and/or import Mozilla/Google cargo-vet audits), and
  either split nm-host into its own tiny crate or record the deviation.
- **Q4 (VER-I11).** Browser card autofill: not built. Keep it out of v1
  (cards stay reveal-only) or build it before the first real credential?
