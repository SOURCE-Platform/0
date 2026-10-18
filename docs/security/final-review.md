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
| SPEC-B4 (revision half) | One unreadable *revision* stops every rotation | **Fixed per owner Q1** `4b5fa7d` — stops before any prompt, names the items (`damaged`, `damaged_records` event), the app says to restore or delete them; nothing set aside; test `a_damaged_record_stops_a_key_change_and_is_named`; spec §23.7 |
| VER-B4 | Reverse-enrollment failure paths untested | **Fixed** `d6e9432` — RE-02, RE-05, RE-06 (pending revocation = pending rotation, not fresh), RE-07, RE-08. RE-04 (bundle negatives) and RE-10 are **Carried** to the gate work below; RE-12 contradicts design §11a and is removed from the design (Spec) |
| SPEC-B1 / VER-B3 | No F.2/G/H/I gate scripts; §19 IDs without tests; a gate run would pass vacuously | **Fixed (gates) / in progress (tests)** — `scripts/vault-gate-map.py`, `scripts/vault-id-gate.py`, `phase-{f2,g,h,i,j}-gate.sh`: every required ID by test name, failing on missing/failed/ignored tests and on missing evidence notes (`docs/security/evidence/`). Tests added for IM-01…06, DA-12/SC-02, PR-03, BR-15, NM-02…04, RE-02/04/05/06/07/08/10/11, RG-01/02/06/18, AU-05, PA-06/07, EV-05, FR-03, PS-02/03, PW-05, ST-03/04, CS-06/08, RC-06, UI-01/03/05. `--map-only` shows what remains |
| RG-06 (found while writing tests) | The registry loader never compared the file with the header's recorded head: an older registry file put back was believed | **Fixed** — `registry::log::read_state` refuses a file that does not contain the header's head (`REGISTRY_TRUNCATED`); test `rg06_an_older_registry_file_is_refused`; registry-changing flows re-run green |
| SPEC-B2 | Spec still says F.2e is unspecified | **Spec** — corrected in place; §23.1/§23.8 (v0.6) |
| SPEC-B3 | §1.5 / §13.2 do not list the G/H/F.2e ops; ops 7/8 missing from §22.8 | **Spec** — §23.1 catalogue, renames, ops 7/8 (UNLOCKED and COMPROMISED); app-class approval ops a recorded residual |

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
| SPEC-I2 | Phase G spec text stale (§7.1, AUTHORIZING, action 4, §2.9) | **Spec** — §7.1 rewritten; §23.3 strings |
| SPEC-I3 | Phase H spec text (§9.4 ASCII hosts, BR-15, XV-ORIGIN, iframes, op names, §9.9 rate limit scope) | **Spec** — corrected in place; §23.5 |
| SPEC-I6 | Phase I deliberate differences need amendments | **Spec** — §10.1 in place, §23.4, §23.6, §17.1 rows |
| SPEC-I7 / VER-I8 | flate2 family on bootstrap exemptions, not audited | **Fixed per owner Q3** `08550cd`, `b421174` — written notes, pure-Rust backend pinned and guarded, ZIP path fuzzed, Google/Mozilla sets imported |
| SPEC-I8 | Gate script check 12 / UI-05 / PR-01 / EV-03 scope | **Pending** with the gate work |
| SPEC-I9 | Unsigned `GET /v1/vault/registry` route and SourceMobile's vault catch-up still live | **Fixed per owner Q2** `08532a0`; SourceMobile `357c5c8` (its repo, not pushed) |
| VER-I9 | Spec not amended for what was built | **Spec** — v0.6 amendment §23 |
| VER-I11 | Card fill and DA-10/DA-11 not built, not recorded | **Owner Q4: logins only in v1** — spec §23.5 |
| VER-I12 | nm-host links the whole helper crate (§17.2: serde only) | **Fixed per owner Q3** `0b7b586` + `c527924` — own crate, `serde_json` only |
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

## Owner questions (answered 2026-10-17: Q1 stop and name; Q2 remove both; Q3 fix both properly; Q4 logins only)

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

## Re-review (2026-10-18) — bounded, of the fixes `d59b94d..45c4330`

Three reviewers (security, verification, spec) checked the fixes only.
Closed: core 1, 2, 6; VER-B1; RG-06; SPEC-B4 (import_log half); VER-I1;
SPEC-I1; class↔identifier binding; nm-host split; SPEC-I9; the Trash gating;
VER-O3/O8/O10; SPEC-I7/VER-I8/VER-I12. Fixed during the re-review: a sleep or
screen lock now stops an import too (SEC-I2/VER-I2 — the tick thread no longer
waits on a busy core, and sets `LOCK_REQUESTED`); the breach checklist no
longer claims the cutoff before upload (SEC-O3); the gate now requires a
`Result:` line in evidence notes, treats UI-03 as manual, and fails Phase I/J
without the 10⁵-row stress run (VER-B1/B2 in part).

**Open when the owner paused the vault (2026-10-18)** — in the order to do
them; details in each reviewer's report quoted in the session, summarized in
`vault-status-and-next-steps.md` §4 Step 1:

1. **Damaged record still blocks revocation for good** (SEC-B1 / spec SPEC-B1;
   SPEC-B4's revision half NOT closed). `rotation::unreadable` scans every
   revision, history included; delete and restore only add rows, so the named
   item can never be cleared from the app. Needs an owner decision on the
   remedy: repair the damaged revision from the provider's copy of the same
   `revision_id` (checked against the signed index's blob hash), or remove a
   named damaged item's unreadable revisions when the user deletes it. Then a
   test: damage → remedy → `revoke_device` succeeds; and the iPhone message
   names the items.
2. **Connect-then-exec still passes peer authentication** (SEC-I1 NOT closed).
   macOS reports the audit token of the socket's last user, not the
   connector; a client can connect, fork, exec the signed app, and let the
   unsigned child talk. Fix: keep the verified token and re-check it after
   every frame read (helper) and every reply read (nm-host); add a
   connect→hello→exec gate row. §23.2's claim is too strong until then.
3. **CSV reader edge cases** (VER-I1): after a record goes bad, quote state is
   tracked with `field.is_empty()` while `field` is no longer taken at commas
   — a stray quote or a >256 KiB record of several fields can yield a phantom
   row or swallow the rest. Fix: an explicit `at_field_start` flag, take/clear
   `field` at every comma; unit tests for the three cases in the report.
4. **Spec text** (spec re-review I1–I7, O1–O4): §13.1–§13.3 AUTHORIZING and
   §6.B vs §7.1; §1.4 pid wording; §1.5 `unlock` "or phone approval"; §9.3
   schema; §23.1 push rows, `authorize_abort`, approval op states; ops 7/8 in
   §22.8 and the wire annex; §17.1 flate2 "in helper: yes"; §23.7 "every key
   change" vs `setup_retry_handle` and adoption; the serving states of ops
   7/8 and what a lock does to pending approvals; RE rows (§22.13, §23.8);
   MT-02's row vs the retirement; `ov0/meta` in §2.9; §3.7 kind 3.
5. **Gate honesty** (VER-B2, VER-I4, spec I6): file-header IDs are credited to
   every test in a file (BR-08/BR-15 in `browser_rules.rs`, UI-05 in
   `vault_rk_ops.rs`); MA-01…10, XV-TLV and XV-HPKE-SE are not gated;
   `phase-j-gate.sh` covers only the ID rows of §19; IM-01's XV-HKDF part;
   untested fixes: the half-finished `renroll_finish` boot sweep, the lock
   race in `renroll_complete`, the join proof-before-body.
6. **Smaller:** SEC-O1 (read the header before the registry so an interrupted
   rotation never looks like a rollback); SEC-O2 (a duplicate whose password
   differs should keep the export); VER-O1 (count dropped `import_log` rows);
   VER-O2 (RE-02/RE-04 assert which check refused); VER-O4 (page-supplied
   username in the Touch ID text); `scripts/audit-deps.sh` fails on
   RUSTSEC-2026-0346…0349 (rustls via the app's `axum-server`, not the
   helper).
