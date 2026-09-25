//! Item CRUD ops (§1.5): `list_items`, `add_item`, `update_item`,
//! `delete_item`. Every mutation passes one LA presence check via
//! `gate::presence_gate` (§13.3). `reveal` lives in `gate.rs` with its
//! §14.4 capture check.
//!
//! Wire shape: record fields arrive either nested under `"fields"` or
//! flat on the frame (the Phase C management UI sends flat). Both are
//! the same field bag to the whitelist below; `op`/`kind`/`ref` keys are
//! ignored by it and forbidden-key rejection (cvv & co.) still applies.

use std::sync::{Arc, Mutex};

use serde_json::{json, Value};

use super::gate::{finish_authorized, presence_gate};
use super::{lock_core, Deps, OpOutcome, VaultCore};
use crate::crypto::secret::SecretBytes;
use crate::errors::ErrorCode;
use crate::state::VaultState;
use crate::storage::records::{self, CardRecord, LoginRecord, UrlEntryWire};
use crate::storage::store::now_epoch;
use crate::storage::VaultStore;

/// §1.5 `list_items`: metadata only, UNLOCKED required (§13.2).
pub fn list_items(core: &Arc<Mutex<VaultCore>>) -> OpOutcome {
    let c = lock_core(core);
    if c.state != VaultState::Unlocked {
        return OpOutcome::err(ErrorCode::BadState);
    }
    let (Some(store), Some(vk)) = (c.store.as_ref(), c.vk.as_ref()) else {
        return OpOutcome::err(ErrorCode::Internal);
    };
    match store.list_records(vk) {
        Ok(items) => OpOutcome::ok(json!({"items": items})),
        Err(e) => OpOutcome::err(e),
    }
}

/// The record field bag: nested `fields` object if present, else the
/// frame itself.
fn fields_of(frame: &Value) -> Option<Value> {
    frame
        .get("fields")
        .cloned()
        .or_else(|| frame.is_object().then(|| frame.clone()))
}

/// §1.5 `add_item`: one record's fields cross IPC once, main→helper only.
/// No capture check here — this op releases nothing (§14.4 binds on
/// display-class releases; the add form's own capture duties are §14.2
/// main-side).
pub fn add_item(core: &Arc<Mutex<VaultCore>>, frame: &Value, deps: &Deps) -> OpOutcome {
    let Some(kind_tag) = frame
        .get("kind")
        .and_then(Value::as_str)
        .and_then(records::kind_tag)
    else {
        return OpOutcome::err(ErrorCode::InvalidInput);
    };
    let Some(fields) = fields_of(frame) else {
        return OpOutcome::err(ErrorCode::InvalidInput);
    };
    if records::reject_forbidden_fields(&fields).is_err() {
        return OpOutcome::err(ErrorCode::InvalidInput);
    }
    if let Err(o) = presence_gate(core, deps, "Source Vault: add item") {
        return o;
    }
    finish_authorized!(core, deps, move |store: &mut VaultStore,
                                         vk: &SecretBytes<32>| {
        let (plaintext, meta, warnings) = build_record(kind_tag, &fields, None)?;
        let r = store.add_record(vk, kind_tag, plaintext.as_bytes(), meta.as_bytes())?;
        let mut extra = json!({"ref": r});
        if !warnings.is_empty() {
            extra["warnings"] = json!(warnings);
        }
        Ok(extra)
    })
}

/// §1.5 `update_item`: full-field edit of one record. Absent fields keep
/// their stored values (merge over the tip); password changes append to
/// history helper-side (§8.1: old password appended, cap 10).
pub fn update_item(core: &Arc<Mutex<VaultCore>>, frame: &Value, deps: &Deps) -> OpOutcome {
    let Some(r) = frame.get("ref").and_then(Value::as_str) else {
        return OpOutcome::err(ErrorCode::InvalidInput);
    };
    let Some(fields) = fields_of(frame) else {
        return OpOutcome::err(ErrorCode::InvalidInput);
    };
    if records::reject_forbidden_fields(&fields).is_err() {
        return OpOutcome::err(ErrorCode::InvalidInput);
    }
    if let Err(o) = presence_gate(core, deps, "Source Vault: update item") {
        return o;
    }
    finish_authorized!(core, deps, move |store: &mut VaultStore,
                                         vk: &SecretBytes<32>| {
        let tip = store.read_tip(vk, r)?;
        let old: Value =
            serde_json::from_slice(&tip.plaintext).map_err(|_| ErrorCode::RecordCorrupt)?;
        let (plaintext, meta, warnings) = build_record(tip.kind_tag, &fields, Some(&old))?;
        let created_at = old
            .get("created_at")
            .and_then(Value::as_u64)
            .unwrap_or_else(now_epoch);
        store.write_successor(
            vk,
            r,
            tip.kind_tag,
            tip.schema_version,
            plaintext.as_bytes(),
            meta.as_bytes(),
            created_at,
        )?;
        let mut extra = json!({});
        if !warnings.is_empty() {
            extra["warnings"] = json!(warnings);
        }
        Ok(extra)
    })
}

/// §1.5 `delete_item`: tombstone revision (§3.2); the record stays in the
/// graph as a deleted tip.
pub fn delete_item(core: &Arc<Mutex<VaultCore>>, frame: &Value, deps: &Deps) -> OpOutcome {
    let Some(r) = frame.get("ref").and_then(Value::as_str) else {
        return OpOutcome::err(ErrorCode::InvalidInput);
    };
    if let Err(o) = presence_gate(core, deps, "Source Vault: delete item") {
        return o;
    }
    finish_authorized!(core, deps, move |store: &mut VaultStore,
                                         vk: &SecretBytes<32>| {
        store.tombstone(vk, r)?;
        Ok(json!({}))
    })
}

/// Build (plaintext, meta, warnings) from the field bag. When `old` is
/// present, editable fields merge over it (update); otherwise a fresh
/// record is created. Only whitelisted editable keys are taken from the
/// wire; `created_at`/`password_history` are helper-managed.
pub(super) fn build_record(
    kind_tag: u8,
    fields: &Value,
    old: Option<&Value>,
) -> Result<(String, String, Vec<&'static str>), ErrorCode> {
    match kind_tag {
        records::KIND_LOGIN => build_login(fields, old),
        records::KIND_CARD => build_card(fields, old),
        _ => Err(ErrorCode::InvalidInput),
    }
}

fn field<'f>(new: &'f Value, old: Option<&'f Value>, key: &str) -> Option<&'f Value> {
    new.get(key).or_else(|| old.and_then(|o| o.get(key)))
}

fn as_str(v: Option<&Value>) -> Option<&str> {
    v.and_then(Value::as_str)
}

/// URLs from the wire. Shorthands `"host": "example.test"` and
/// `"hosts": ["..."]` (what the Phase C management UI sends) map to
/// exact-match entries; the canonical §8.1 `"urls": [{host, match,
/// allow_http}]` array is also accepted. Absent everywhere → the stored
/// record's entries on update; empty on create (validation then errors).
fn wire_urls(fields: &Value, old: Option<&Value>) -> Result<Vec<UrlEntryWire>, ErrorCode> {
    let exact = |host: String| UrlEntryWire {
        host,
        match_mode: "exact".to_string(),
        allow_http: false,
    };
    if let Some(host) = as_str(field(fields, None, "host")) {
        return Ok(vec![exact(host.to_string())]);
    }
    if let Some(hosts) = field(fields, None, "hosts") {
        let hosts: Vec<String> =
            serde_json::from_value(hosts.clone()).map_err(|_| ErrorCode::InvalidInput)?;
        return Ok(hosts.into_iter().map(exact).collect());
    }
    match field(fields, old, "urls") {
        Some(v) => serde_json::from_value(v.clone()).map_err(|_| ErrorCode::InvalidInput),
        None => Ok(vec![]),
    }
}

fn build_login(
    fields: &Value,
    old: Option<&Value>,
) -> Result<(String, String, Vec<&'static str>), ErrorCode> {
    let now = now_epoch();
    let urls = wire_urls(fields, old)?;
    let mut rec = LoginRecord {
        schema: records::SCHEMA_LOGIN.to_string(),
        title: as_str(field(fields, old, "title"))
            .ok_or(ErrorCode::InvalidInput)?
            .to_string(),
        username: as_str(field(fields, old, "username"))
            .unwrap_or_default()
            .to_string(),
        password: as_str(field(fields, old, "password"))
            .ok_or(ErrorCode::InvalidInput)?
            .to_string(),
        urls,
        notes: as_str(field(fields, old, "notes"))
            .unwrap_or_default()
            .to_string(),
        created_at: old
            .and_then(|o| o.get("created_at"))
            .and_then(Value::as_u64)
            .unwrap_or(now),
        updated_at: now,
        password_history: vec![],
    };
    if let Some(old) = old {
        let old_password = old.get("password").and_then(Value::as_str).unwrap_or("");
        let mut history: Vec<records::HistoryEntry> = old
            .get("password_history")
            .cloned()
            .and_then(|h| serde_json::from_value(h).ok())
            .unwrap_or_default();
        if old_password != rec.password {
            let changed_at = old
                .get("updated_at")
                .and_then(Value::as_u64)
                .unwrap_or(now);
            history.push(records::HistoryEntry {
                password: old_password.to_string(),
                changed_at,
            });
        }
        rec.password_history = history;
    }
    records::validate_login(&mut rec)?;
    let hosts: Vec<String> = rec.urls.iter().map(|u| u.host.clone()).collect();
    let meta = records::meta_json(records::KIND_LOGIN, &rec.title, &rec.username, &hosts);
    let plaintext = serde_json::to_string(&rec).map_err(|_| ErrorCode::Internal)?;
    Ok((plaintext, meta.to_string(), vec![]))
}

fn build_card(
    fields: &Value,
    old: Option<&Value>,
) -> Result<(String, String, Vec<&'static str>), ErrorCode> {
    let now = now_epoch();
    let billing = match field(fields, old, "billing_address") {
        Some(v) if !v.is_null() => {
            Some(serde_json::from_value(v.clone()).map_err(|_| ErrorCode::InvalidInput)?)
        }
        _ => None,
    };
    let mut rec = CardRecord {
        schema: records::SCHEMA_CARD.to_string(),
        label: as_str(field(fields, old, "label"))
            .or_else(|| as_str(field(fields, old, "title")))
            .ok_or(ErrorCode::InvalidInput)?
            .to_string(),
        number: as_str(field(fields, old, "number"))
            .ok_or(ErrorCode::InvalidInput)?
            .to_string(),
        expiry: as_str(field(fields, old, "expiry"))
            .ok_or(ErrorCode::InvalidInput)?
            .to_string(),
        cardholder: as_str(field(fields, old, "cardholder"))
            .unwrap_or_default()
            .to_string(),
        billing_address: billing,
        notes: as_str(field(fields, old, "notes"))
            .unwrap_or_default()
            .to_string(),
        created_at: old
            .and_then(|o| o.get("created_at"))
            .and_then(Value::as_u64)
            .unwrap_or(now),
        updated_at: now,
    };
    let luhn_warning = records::validate_card(&mut rec)?;
    let meta = records::meta_json(records::KIND_CARD, &rec.label, "", &[]);
    let plaintext = serde_json::to_string(&rec).map_err(|_| ErrorCode::Internal)?;
    let warnings = if luhn_warning { vec!["luhn"] } else { vec![] };
    Ok((plaintext, meta.to_string(), warnings))
}
