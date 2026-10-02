//! §1.5 `resolve_conflict` (spec v0.4 §3.2): the one op that collapses a
//! record's heads. `{ref, chosen_rev, edits?, acknowledge_tamper?}` —
//! `chosen_rev` (hex `revision_id`) names the head whose content is kept;
//! `edits` optionally merges fields over it (same whitelist as
//! `update_item`). A frozen record additionally needs
//! `acknowledge_tamper: true` (§3.2 "Freeze": fresh presence plus an
//! explicit acknowledgement). Presence is checked once, before any write.

use std::sync::{Arc, Mutex};

use serde_json::{json, Value};

use super::gate::{finish_authorized, presence_gate};
use super::items::build_record;
use super::{lock_core, Deps, OpOutcome, VaultCore};
use crate::crypto::secret::SecretBytes;
use crate::errors::ErrorCode;
use crate::state::VaultState;
use crate::storage::rev_state;
use crate::storage::revisions::{get_row, heads};
use crate::storage::store_records::Resolution;
use crate::storage::VaultStore;

pub fn resolve_conflict(core: &Arc<Mutex<VaultCore>>, frame: &Value, deps: &Deps) -> OpOutcome {
    let Some(r) = frame.get("ref").and_then(Value::as_str) else {
        return OpOutcome::err(ErrorCode::InvalidInput);
    };
    let Some(chosen) = frame
        .get("chosen_rev")
        .and_then(Value::as_str)
        .and_then(crate::crypto::hex::decode_array::<32>)
    else {
        return OpOutcome::err(ErrorCode::InvalidInput);
    };
    let edits = frame.get("edits").filter(|e| !e.is_null()).cloned();
    if let Some(e) = &edits {
        if !e.is_object() || crate::storage::records::reject_forbidden_fields(e).is_err() {
            return OpOutcome::err(ErrorCode::InvalidInput);
        }
    }
    let ack = frame.get("acknowledge_tamper").and_then(Value::as_bool) == Some(true);
    // Refuse a frozen record without the acknowledgement before prompting;
    // a frozen record gets its own tamper-specific prompt (SEC-O4).
    let frozen;
    {
        let c = lock_core(core);
        if c.state != VaultState::Unlocked {
            return OpOutcome::err(ErrorCode::BadState);
        }
        let Some(store) = c.store.as_ref() else {
            return OpOutcome::err(ErrorCode::Internal);
        };
        frozen = match rev_state::is_frozen(&store.conn, r) {
            Ok(true) if !ack => return OpOutcome::err(ErrorCode::ConflictPending),
            Ok(f) => f,
            Err(e) => return OpOutcome::err(e),
        };
    }
    let prompt = if frozen {
        "Source Vault: accept a possibly tampered item"
    } else {
        "Source Vault: resolve conflict"
    };
    if let Err(o) = presence_gate(core, deps, prompt) {
        return o;
    }
    finish_authorized!(core, deps, move |store: &mut VaultStore,
                                         vk: &SecretBytes<32>| {
        let Some(edits) = &edits else {
            store.resolve(vk, r, Resolution::Chosen(chosen), ack)?;
            return Ok(json!({}));
        };
        // `resolve` checks that `chosen` is a current head of `r`; the row
        // is read here only to merge the edits over it.
        let row = get_row(&store.conn, &chosen)?.ok_or(ErrorCode::InvalidInput)?;
        if row.record_id != r || row.deleted || !heads(&store.conn, r)?.contains(&chosen) {
            return Err(ErrorCode::InvalidInput);
        }
        let old: Value = serde_json::from_slice(&store.open_row(vk, &row)?).map_err(|_| ErrorCode::RecordCorrupt)?;
        let (plaintext, meta, warnings) = build_record(row.kind_tag, edits, Some(&old))?;
        drop(old);
        let plaintext = zeroize::Zeroizing::new(plaintext);
        store.resolve(
            vk,
            r,
            Resolution::Edited {
                base: chosen,
                kind_tag: row.kind_tag,
                schema_version: row.schema_version,
                plaintext: plaintext.as_bytes(),
                meta: meta.as_bytes(),
            },
            ack,
        )?;
        let mut extra = json!({});
        if !warnings.is_empty() {
            extra["warnings"] = json!(warnings);
        }
        Ok(extra)
    })
}
