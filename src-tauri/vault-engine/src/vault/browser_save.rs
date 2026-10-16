//! Saving from the browser (spec §9.8; Phase H).
//!
//! - `browser_save_new {origin, tab_url, username, password, title?}` —
//!   UNLOCKED; the page's origins must agree; a login already saved for
//!   this host and username is never overwritten here: the answer is
//!   `{needs_update: ref}` and the update path below applies.
//! - `browser_save_update {ref, origin, tab_url, password, method}` — never
//!   applied on the extension's say-so (§9.8, v0.2 finding 9). `local`: a
//!   presence prompt rendered by the OS names the account and the site
//!   (never a password), then the new revision (old password → history);
//!   `iphone`: a §6.5 `update_password` approval, then
//!   `browser_save_update_finish {approval_id, ...}`.

use std::sync::{Arc, Mutex};

use serde_json::{json, Value};

use super::browser_ops::page;
use super::gate::{finish_authorized, presence_gate};
use super::{lock_core, Deps, OpOutcome, VaultCore};
use crate::browser::matching::{best, Verdict};
use crate::browser::origin::Origin;
use crate::crypto::hex;
use crate::crypto::secret::SecretBytes;
use crate::errors::ErrorCode;
use crate::state::VaultState;
use crate::storage::records::{self, LoginRecord};
use crate::storage::store::now_epoch;
use crate::storage::VaultStore;

fn run(f: impl FnOnce() -> Result<Value, ErrorCode>) -> OpOutcome {
    f().map_or_else(OpOutcome::err, OpOutcome::ok)
}

fn text<'a>(frame: &'a Value, key: &str) -> Result<&'a str, ErrorCode> {
    frame.get(key).and_then(Value::as_str).filter(|s| !s.is_empty() && s.len() <= 4096).ok_or(ErrorCode::InvalidInput)
}

/// The login saved for this page's host and username, if any.
fn existing(store: &VaultStore, vk: &SecretBytes<32>, page: &Origin, username: &str) -> Result<Option<String>, ErrorCode> {
    for item in store.list_records(vk)? {
        let (Some(r), Some("login")) = (item["ref"].as_str(), item["kind"].as_str()) else { continue };
        let Ok(tip) = store.read_tip(vk, r) else { continue };
        let Ok(rec) = serde_json::from_slice::<LoginRecord>(&tip.plaintext) else { continue };
        if rec.username == username && best(page, &rec.urls) == Verdict::Eligible {
            return Ok(Some(r.to_string()));
        }
    }
    Ok(None)
}

pub fn browser_save_new(core: &Arc<Mutex<VaultCore>>, frame: &Value, deps: &Deps) -> OpOutcome {
    let prepared = (|| {
        let page = page(frame)?;
        let (username, password) = (text(frame, "username")?.to_string(), text(frame, "password")?.to_string());
        let title = frame.get("title").and_then(Value::as_str).filter(|t| !t.is_empty()).unwrap_or(&page.host).chars().take(200).collect::<String>();
        let c = lock_core(core);
        if c.state != VaultState::Unlocked {
            return Err(ErrorCode::BadState);
        }
        if let Some(r) = existing(c.store.as_ref().ok_or(ErrorCode::BadState)?, c.vk.as_ref().ok_or(ErrorCode::BadState)?, &page, &username)? {
            return Ok(Err(r)); // §9.8: an existing match is an update
        }
        Ok(Ok(json!({ "op": "add_item", "kind": "login", "title": title, "username": username, "password": password, "urls": [{ "host": page.host, "match": "exact", "allow_http": false }] })))
    })();
    match prepared {
        Err(e) => OpOutcome::err(e),
        Ok(Err(r)) => OpOutcome::ok(json!({ "needs_update": r })),
        Ok(Ok(item)) => super::items::add_item(core, &item, deps),
    }
}

fn apply(store: &mut VaultStore, vk: &SecretBytes<32>, r: &str, password: &str) -> Result<Value, ErrorCode> {
    let tip = store.read_tip(vk, r)?;
    if tip.kind_tag != records::KIND_LOGIN {
        return Err(ErrorCode::InvalidInput);
    }
    let old: Value = serde_json::from_slice(&tip.plaintext).map_err(|_| ErrorCode::RecordCorrupt)?;
    let (plaintext, meta, _) = super::items::build_record(tip.kind_tag, &json!({ "password": password }), Some(&old))?;
    let created_at = old.get("created_at").and_then(Value::as_u64).unwrap_or_else(now_epoch);
    store.write_successor(vk, r, tip.kind_tag, tip.schema_version, plaintext.as_bytes(), meta.as_bytes(), created_at)?;
    Ok(json!({}))
}

/// The account named in the confirmation, and that the ref belongs here.
fn account(core: &Arc<Mutex<VaultCore>>, page: &Origin, r: &str) -> Result<String, ErrorCode> {
    let c = lock_core(core);
    if c.state != VaultState::Unlocked {
        return Err(ErrorCode::BadState);
    }
    let (store, vk) = (c.store.as_ref().ok_or(ErrorCode::BadState)?, c.vk.as_ref().ok_or(ErrorCode::BadState)?);
    let rec: LoginRecord = serde_json::from_slice(&store.read_tip(vk, r)?.plaintext).map_err(|_| ErrorCode::RecordCorrupt)?;
    if best(page, &rec.urls) != Verdict::Eligible {
        return Err(ErrorCode::ApprovalMismatch); // only a login saved for this site
    }
    Ok(rec.username)
}

pub fn browser_save_update(core: &Arc<Mutex<VaultCore>>, frame: &Value, deps: &Deps) -> OpOutcome {
    let checked = (|| {
        let page = page(frame)?;
        let r = text(frame, "ref")?.to_string();
        let password = text(frame, "password")?.to_string();
        let user = account(core, &page, &r)?;
        Ok::<_, ErrorCode>((page, r, password, user))
    })();
    let (page, r, password, user) = match checked {
        Ok(v) => v,
        Err(e) => return OpOutcome::err(e),
    };
    match frame.get("method").and_then(Value::as_str) {
        Some("local") => {
            let reason = format!("Source Vault: update the saved password for {user} on {}?", page.host);
            if let Err(o) = presence_gate(core, deps, &reason) {
                return o;
            }
            finish_authorized!(core, deps, move |store: &mut VaultStore, vk: &SecretBytes<32>| apply(store, vk, &r, &password))
        }
        Some("iphone") => run(|| {
            let rid = crate::storage::revisions::uuid_bytes(&r).ok_or(ErrorCode::InvalidInput)?;
            let (id, exp) = crate::approval::request(&page.text(), vault_proto::approval::Action::UpdatePassword, Some(rid), now_epoch())?;
            deps.events.emit(json!({ "event": "approval_pending", "request_id": hex::encode(id) }));
            Ok(json!({ "approval_id": hex::encode(id), "exp": exp }))
        }),
        _ => OpOutcome::err(ErrorCode::InvalidInput),
    }
}

/// After the iPhone approved the update: the same checks, then apply once.
pub fn browser_save_update_finish(core: &Arc<Mutex<VaultCore>>, frame: &Value) -> OpOutcome {
    run(|| {
        let page = page(frame)?;
        let r = text(frame, "ref")?.to_string();
        let password = text(frame, "password")?;
        let approval = frame.get("approval_id").and_then(Value::as_str).and_then(hex::decode_array::<16>).ok_or(ErrorCode::InvalidInput)?;
        account(core, &page, &r)?;
        // The approval must be for exactly this site, action and login.
        let rid = crate::storage::revisions::uuid_bytes(&r).ok_or(ErrorCode::InvalidInput)?;
        if !crate::approval::bound(approval, &page.text(), vault_proto::approval::Action::UpdatePassword, Some(rid)) {
            return Err(ErrorCode::ApprovalMismatch);
        }
        match crate::approval::status(approval, now_epoch()) {
            Some("approved") => {}
            Some("pending") => return Ok(json!({ "pending": true })),
            _ => return Err(ErrorCode::PresenceDenied),
        }
        crate::approval::take(approval)?;
        let mut c = lock_core(core);
        let c = &mut *c;
        apply(c.store.as_mut().ok_or(ErrorCode::BadState)?, c.vk.as_ref().ok_or(ErrorCode::BadState)?, &r, password)
    })
}
