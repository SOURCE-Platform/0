//! The Dashlane import run (spec §10.3–§10.5; Phase I), inside one
//! authorized window: file → CSV stream → row → map → the same record
//! builder `add_item` uses → AEAD seal → INSERT → wipe the row. No list of
//! records, no temp file, no preview vault.
//!
//! Duplicates (§10.3): the identity is built from the *validated* record
//! (so a manual item and an imported row compare alike) and compared only
//! as an HMAC fingerprint. The set starts from `import_log` plus every
//! live item in the vault; a duplicate keeps the vault's item.
//!
//! - login: `u8(kind) ‖ host ‖ username` (first URL entry's host);
//! - card: `u8(kind) ‖ label ‖ number`;
//! - note: `u8(kind) ‖ title ‖ SHA-256(body)` (§10.8 Choice A).
//!
//! Each part is length-prefixed (u32 BE), so `("a.b","c")` and
//! `("a.bc","")` differ — the spec's `‖` read without ambiguity.
//!
//! The report names counts, categories, file names and row numbers —
//! never a value, username, title or URL (§10.5).

use std::collections::{BTreeMap, HashSet};
use std::fs::File;
use std::io::BufRead;

use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use zeroize::Zeroize;

use super::items::build_record;
use crate::crypto::secret::SecretBytes;
use crate::errors::ErrorCode;
use crate::import::csv::{choose_mode, Reader, Row};
use crate::import::dialect::{Dialect, Kind, Mapped};
use crate::import::zip;
use crate::storage::revisions::RevisionRow;
use crate::storage::{import_log, records, VaultStore};

/// Skipped-row details kept for the report (row numbers beyond this are
/// still counted).
const MAX_ROW_NOTES: usize = 1000;
/// ZIP members read (§10.1, §10.8 Choice A); anything else is reported.
const READ_MEMBERS: [&str; 3] = ["credentials.csv", "payments.csv", "securenotes.csv"];
/// Sealed records committed per transaction (`store_batch.rs`).
const BATCH: usize = 5000;

#[derive(Default)]
pub struct Report {
    imported: u64,
    duplicates: u64,
    malformed: u64,
    unsupported: BTreeMap<String, u64>,
    cvv_discarded: u64,
    rows: Vec<Value>,
    /// The user locked the vault mid-import (§13.3): the rest is unread.
    stopped_by_lock: bool,
}

impl Report {
    fn skip(&mut self, file: &str, row: u64, why: &str) {
        self.malformed += 1;
        if self.rows.len() < MAX_ROW_NOTES {
            self.rows.push(json!({"file": file, "row": row, "why": why}));
        }
    }

    fn unsupported(&mut self, category: &str, n: u64) {
        *self.unsupported.entry(category.to_string()).or_default() += n;
    }

    /// §10.6 (final review SPEC-I5): something in the export did not come
    /// in — a skipped row, an unsupported column or member, a damaged
    /// member. The app then withholds the delete offer.
    pub fn keep_export(&self) -> bool {
        self.malformed > 0 || !self.unsupported.is_empty() || self.stopped_by_lock
    }

    pub fn to_json(&self, ms: u128) -> Value {
        json!({
            "keep_export": self.keep_export(),
            "stopped": if self.stopped_by_lock { Value::from("LOCKED") } else { Value::Null },
            "imported": self.imported, "duplicates": self.duplicates, "skipped_malformed": self.malformed,
            "skipped_unsupported": self.unsupported, "cvv_discarded": self.cvv_discarded,
            "skipped_rows": self.rows, "rows_truncated": self.malformed as usize > self.rows.len(), "duration_ms": ms as u64,
        })
    }
}

fn part(out: &mut Vec<u8>, b: &[u8]) {
    out.extend_from_slice(&(b.len() as u32).to_be_bytes());
    out.extend_from_slice(b);
}

/// The §10.3 identity of a validated record's plaintext.
pub(crate) fn identity(kind_tag: u8, rec: &Value) -> Vec<u8> {
    let s = |k: &str| rec.get(k).and_then(Value::as_str).unwrap_or("").as_bytes();
    let mut out = vec![kind_tag];
    match kind_tag {
        records::KIND_LOGIN => {
            let host = rec.pointer("/urls/0/host").and_then(Value::as_str).unwrap_or("");
            part(&mut out, host.as_bytes());
            part(&mut out, s("username"));
        }
        records::KIND_CARD => {
            part(&mut out, s("label"));
            part(&mut out, s("number"));
        }
        _ => {
            part(&mut out, s("title"));
            part(&mut out, &Sha256::digest(s("body")));
        }
    }
    out
}

/// Zero every string in a field bag before it is dropped.
fn wipe(v: &mut Value) {
    match v {
        Value::String(s) => s.zeroize(),
        Value::Array(a) => a.iter_mut().for_each(wipe),
        Value::Object(o) => o.values_mut().for_each(wipe),
        _ => {}
    }
}

pub struct Run<'a> {
    store: &'a mut VaultStore,
    vk: &'a SecretBytes<32>,
    seen: HashSet<[u8; 32]>,
    /// Sealed, not yet committed: ciphertext only.
    pending: Vec<RevisionRow>,
    pending_log: Vec<([u8; 32], Vec<u8>)>,
    pub report: Report,
}

impl<'a> Run<'a> {
    /// The duplicate set: `import_log` plus every live, readable item.
    pub fn new(store: &'a mut VaultStore, vk: &'a SecretBytes<32>) -> Result<Run<'a>, ErrorCode> {
        let mut seen: HashSet<[u8; 32]> = import_log::all(&store.conn)?.into_iter().collect();
        for item in store.list_records(vk)? {
            let Some(r) = item.get("ref").and_then(Value::as_str) else { continue };
            if item.get("conflicted").is_some() || item.get("corrupt").is_some() {
                continue;
            }
            let Ok(tip) = store.read_tip(vk, r) else { continue };
            let Ok(mut rec) = serde_json::from_slice::<Value>(&tip.plaintext) else { continue };
            let mut id = identity(tip.kind_tag, &rec);
            seen.insert(import_log::fingerprint(vk, &store.header, &id)?);
            id.zeroize();
            wipe(&mut rec);
        }
        Ok(Run { store, vk, seen, pending: Vec::new(), pending_log: Vec::new(), report: Report::default() })
    }

    /// Commit what is sealed so far (one transaction, one flip).
    fn flush(&mut self) -> Result<(), ErrorCode> {
        let log = std::mem::take(&mut self.pending_log);
        let revs = std::mem::take(&mut self.pending);
        self.store.commit_new_batch(&revs, |tx| log.iter().try_for_each(|(fp, ct)| import_log::insert(tx, fp, ct)))?;
        // Counted only once committed: a failure never overstates it.
        self.report.imported += revs.len() as u64;
        Ok(())
    }

    /// One file: a single CSV, or a Dashlane ZIP.
    pub fn file(&mut self, f: &File) -> Result<(), ErrorCode> {
        self.read_file(f)?;
        self.flush()
    }

    fn read_file(&mut self, f: &File) -> Result<(), ErrorCode> {
        if !zip::looks_like_zip(f) {
            return self.csv("csv", &|| zip::rewind(f).map(|g| Box::new(std::io::BufReader::new(g)) as Box<dyn BufRead>));
        }
        let Some(members) = zip::members(f) else { return Err(ErrorCode::InvalidInput) };
        for m in &members {
            if self.report.stopped_by_lock {
                break;
            }
            if m.name.is_empty() {
                continue; // a folder entry
            }
            if !READ_MEMBERS.contains(&m.name.as_str()) {
                let known = m.name.len() <= 40 && m.name.bytes().all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b));
                self.report.unsupported(&format!("member {}", if known { m.name.as_str() } else { "(other)" }), 1);
                continue;
            }
            // The whole member is checked before any of its rows is read.
            if !zip::verify_member(f, m) {
                self.report.unsupported(&format!("damaged {}", m.name), 1);
                continue;
            }
            self.csv(&m.name.clone(), &|| zip::open_member(f, m))?;
        }
        Ok(())
    }

    fn csv(&mut self, name: &str, open: &dyn Fn() -> Option<Box<dyn BufRead>>) -> Result<(), ErrorCode> {
        let Some(sample) = open() else {
            self.report.unsupported(&format!("unreadable {name}"), 1);
            return Ok(());
        };
        let mode = choose_mode(sample);
        let Some(input) = open() else { return Ok(()) };
        let mut reader = Reader::new(input, mode);
        let dialect = match reader.next_row() {
            Some(Row::Fields(h)) => Dialect::detect(&h),
            Some(Row::Failed) => {
                self.report.unsupported(&format!("damaged {name}"), 1);
                return Ok(());
            }
            _ => None,
        };
        let Some(dialect) = dialect else {
            self.report.unsupported(&format!("unrecognized columns in {name}"), 1);
            return Ok(());
        };
        let mut n = 1u64;
        while let Some(row) = reader.next_row() {
            // §13.3: a lock is waiting — stop here; what is committed stays.
            if super::LOCK_REQUESTED.load(std::sync::atomic::Ordering::SeqCst) {
                self.report.stopped_by_lock = true;
                return Ok(());
            }
            n += 1;
            let mut fields = match row {
                Row::Fields(fields) => fields,
                Row::Bad => {
                    self.report.skip(name, n, "malformed");
                    continue;
                }
                Row::Failed => {
                    // Never the end of the file: the rest of it is unread.
                    self.report.unsupported(&format!("damaged {name} (stopped at row {n})"), 1);
                    break;
                }
            };
            if fields.iter().all(|f| f.trim().is_empty()) {
                continue; // a blank line
            }
            self.row(&dialect, name, n, &fields)?;
            fields.iter_mut().for_each(|f| f.zeroize());
        }
        Ok(())
    }

    fn row(&mut self, d: &Dialect, name: &str, n: u64, fields: &[String]) -> Result<(), ErrorCode> {
        let mut item = match d.map(fields) {
            Mapped::Item(v) => v,
            Mapped::Skip(why) => {
                self.report.skip(name, n, why);
                return Ok(());
            }
        };
        let kind_tag = match d.kind {
            Kind::Login => records::KIND_LOGIN,
            Kind::Card => records::KIND_CARD,
            Kind::Note => records::KIND_NOTE,
        };
        let built = build_record(kind_tag, &item, None);
        wipe(&mut item);
        let Ok((mut plaintext, mut meta, _warnings)) = built else {
            self.report.skip(name, n, "invalid");
            return Ok(());
        };
        let mut rec: Value = serde_json::from_str(&plaintext).map_err(|_| ErrorCode::Internal)?;
        let mut id = identity(kind_tag, &rec);
        wipe(&mut rec);
        let fp = import_log::fingerprint(self.vk, &self.store.header, &id)?;
        let fresh = self.seen.insert(fp);
        let result = if fresh {
            self.store.seal_new(self.vk, kind_tag, plaintext.as_bytes(), meta.as_bytes()).and_then(|rev| {
                self.pending_log.push(import_log::seal_identity(self.vk, &self.store.header, &id)?);
                self.pending.push(rev);
                Ok(())
            })
        } else {
            Ok(())
        };
        plaintext.zeroize();
        meta.zeroize();
        id.zeroize();
        result?;
        if d.has_otp(fields) {
            self.report.unsupported("otpSecret (re-enter manually)", 1);
        }
        if d.has_cvv() {
            self.report.cvv_discarded += 1;
        }
        if self.pending.len() >= BATCH {
            self.flush()?;
        }
        if !fresh {
            self.report.duplicates += 1;
        }
        Ok(())
    }
}
