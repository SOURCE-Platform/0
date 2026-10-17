# Phase I — Dashlane import and Secure Notes (build notes, revision 1)

Spec §10 (§10.8 Choice A, owner decision 2026-10-16). Owner decision
2026-10-16: no review during development — reviewed in the final review.
**Synthetic data only:** no real export has been opened, parsed or used;
every fixture comes from `scripts/gen-dashlane-fixtures.py` (fixed seed,
`fixture-…` passwords, `.test` hosts, published test card numbers).

## Built

| Piece | Where | Notes |
|---|---|---|
| `secure_note@1` | `vault-engine/src/storage/note_record.rs`, `records.rs` (`KIND_NOTE = 3`), `vault/items.rs` (`build_note`), `vault/gate.rs` | title (NFC, 1–200 chars) is the only metadata; body ≤ 64 KiB, only in the record ciphertext; reveal returns `{body}`; never a browser candidate (candidates and fills are login-only) |
| CSV reader (§10.1, §10.3) | `vault-engine/src/import/csv.rs` | streaming, one record at a time; strict or lenient chosen once per file from the first 100 records; invalid UTF-8 refuses the row (no lossy conversion); field > 64 KiB or row > 256 KiB skipped |
| Dialects (§10.2) | `import/dialect.rs` | credentials → login, payments → card, secure notes → note; header case-insensitive and trimmed; also accepts `cc_number`, `account_holder`, split `expiration_month`/`expiration_year`; CVV columns (`cvv`, `cvc`, `security_code`, `code`) are never read; `otpSecret`/`otpUrl` counted "re-enter manually" |
| ZIP (§10.1) | `import/zip.rs`, flate2 | central directory parsed from one already-open handle; ZIP64, > 64 members, encrypted members and methods other than stored/deflate refused; a member's output is cut at its declared size (≤ 1 GiB); reads `credentials.csv`, `payments.csv`, `securenotes.csv`; others reported by name |
| The run (§10.3–§10.5) | `vault/import_run.rs`, `storage/store_batch.rs` | row → map → the same record builder as `add_item` → seal → wipe; duplicates as HMAC fingerprints against `import_log` **and** every live item; sealed revisions and their `import_log` rows committed 5000 per transaction with one manifest flip; the report holds counts, categories, file names and row numbers only |
| `import_dashlane {path}` (§10.4) | `vault/import_ops.rs` | absolute path, opened `O_NOFOLLOW`, regular file checked on the open handle, not inside the vault directory, ≤ 2 GiB; refused before any prompt; one presence check for the whole import; Mac only (not in `IOS_OPS`) |
| Mac app | `src-tauri/src/app/commands/vault_import.rs`, `src/platform/trash.rs`, `src/components/vault/ImportDashlaneCard.tsx`, `AddNoteForm.tsx` | the file picker runs in Rust, so page code never picks the path; report with "Copy report"; §10.6 honesty copy and "Move the export to the Trash" (NSFileManager, the user can still put it back); "Add note" |
| iPhone app | `ios/SourceVault/SourceVault/Views.swift`, `AppModel.swift` | list shows "Secure note"/"Payment card"; "Show note" reveals the body |
| Tests | `vault-helper/tests/dashlane_import.rs`, unit tests in each module | every §10.7 fixture class; idempotent re-import; vault-item duplicate keeps the vault's item; path refusals without a prompt; presence denied imports nothing; 10^5 rows in 16.5 s optimized (on demand, `--ignored`) |

## Deliberate differences from the spec text (for the final review)

1. **CSV parser.** §10.1 names the `csv` crate. The reader is written here
   instead (≈130 lines, no new dependency; the plan was explained to the
   owner on 2026-10-16). Same behaviour: RFC 4180, strict/lenient chosen
   once per file, streaming.
2. **Identity encoding.** §10.3 writes `u8(kind) ‖ host ‖ username`. Each
   part is length-prefixed (u32 big-endian) so `("a.b","c")` and
   `("a.bc","")` cannot collide. Notes: `u8(kind) ‖ title ‖ SHA-256(body)`.
3. **Duplicates against the vault.** The duplicate set starts from
   `import_log` plus every live, readable item (decrypted once, compared
   as fingerprints, wiped). A conflicted or damaged item is not compared.
4. **Batched commits.** One flip per row rewrote the manifest's full
   object list each time (13 ms/row, quadratic). Rows are sealed at once —
   only ciphertext waits — and committed 5000 at a time; a failing batch
   rolls back with its `import_log` rows.
5. **ZIP CRC** is not checked: rows are imported as they stream, so a
   failure at the end could not undo them. A damaged deflate stream ends
   the member with a read error.
6. **"Savable text" report** is "Copy report" (clipboard), not a file.

## Not yet (carried to the final review)

- `cargo fuzz` target `fuzz_dashlane_row` (§10.7).
- The import holds the vault for its whole run (16.5 s at 10^5 rows); a
  Lock request waits for it.
- Editing a note's body (add and reveal exist; `update_item` accepts a
  note, the UI does not offer it yet).
- The Mac import card and note form were type-checked and built, not yet
  clicked through in the running app; the iPhone changes were built for
  the simulator, not run.
- CR/IM/CS test-catalogue rows and the §8 record-type table for
  `secure_note@1`.
