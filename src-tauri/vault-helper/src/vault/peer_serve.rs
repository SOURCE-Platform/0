//! `peer_serve` (spec v0.5 §1.5, §22.8; wire annex A.2.2): one signed
//! peer request, relayed by main as opaque bytes. The helper verifies,
//! answers and signs everything itself. Inline request bodies ≤ 24 KiB; a
//! response body over 24 KiB is offered as a one-blob stream session
//! (`stream_read`). Served while LOCKED, UNLOCKED, BACKING_UP, SYNCING and
//! COMPROMISED; refusals are `{refused: 403 | 429 | 503}`, unsigned.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{json, Value};
use vault_proto::b64;

use super::{lock_core, OpOutcome, VaultCore};
use crate::crypto::hex;
use crate::device::SeDevice;
use crate::errors::ErrorCode;
use crate::peer::{ops, verify, Ctx};
use crate::state::VaultState;
use crate::storage::store::now_epoch;
use crate::storage::VaultStore;
use crate::sync::session::Transfer;

pub const INLINE: usize = 24 * 1024;
const FRESH: Duration = Duration::from_secs(15 * 60);

fn field(frame: &Value, k: &str) -> Result<Vec<u8>, ErrorCode> {
    frame.get(k).and_then(Value::as_str).and_then(b64::decode).ok_or(ErrorCode::InvalidInput)
}

fn refused(code: u16) -> Value {
    json!({ "refused": code })
}

pub fn peer_serve(core: &Arc<Mutex<VaultCore>>, frame: &Value) -> OpOutcome {
    let run = || -> Result<Value, ErrorCode> {
        let (tlv, sig, body) = (field(frame, "request_tlv")?, field(frame, "signature")?, field(frame, "body")?);
        if body.len() > INLINE {
            return Err(ErrorCode::InvalidInput); // larger bodies: peer_serve_begin
        }
        let mut guard = lock_core(core);
        let c = &mut *guard;
        let serving = matches!(c.state, VaultState::Locked | VaultState::Unlocked | VaultState::BackingUp | VaultState::Syncing | VaultState::Compromised);
        if !serving || c.unverified_key {
            return Ok(refused(503));
        }
        let dir = c.vault_dir.clone();
        let Ok(me) = SeDevice::load(&dir) else {
            return Ok(refused(503));
        };
        let locked = c.vk.is_none();
        let fresh = !locked && c.provider_checked.is_some_and(|t| t.elapsed() < FRESH);
        let compromised = c.state == VaultState::Compromised;
        let mut opened = None;
        let store: &mut VaultStore = match c.store.as_mut() {
            Some(s) => s,
            None => opened.insert(VaultStore::open(&dir)?),
        };
        // The floor check needs no key, so a LOCKED Mac applies it too.
        let behind = if locked { crate::vault::floor::behind(store).unwrap_or(true) } else { c.behind };
        let ctx = Ctx { dir, vault_id: store.header.vault_id.0, me, behind, compromised, locked, fresh };
        let now = now_epoch();
        let acc = match verify::authenticate(&ctx, store, &tlv, &sig, Some(&body), now) {
            Ok(a) => a,
            Err(r) => return Ok(refused(r.http())),
        };
        let signed = match ops::serve(&ctx, store, c.vk.as_ref(), &acc, &body, now) {
            Ok(s) => s,
            Err(r) => return Ok(refused(r.http())),
        };
        let mut out = json!({
            "response_tlv": b64::encode(&signed.response_tlv),
            "signature": b64::encode(&signed.signature),
        });
        if signed.body.len() <= INLINE {
            out["body"] = json!(b64::encode(&signed.body));
        } else {
            let sha = vault_proto::peer::body_hash(&signed.body);
            let size = signed.body.len();
            let t = Transfer::new(BTreeMap::from([(sha, signed.body)]));
            out["session"] = json!(hex::encode(t.id));
            out["stream"] = json!(hex::encode(sha));
            out["size"] = json!(size);
            c.provider.peer = Some(t);
        }
        Ok(out)
    };
    run().map_or_else(OpOutcome::err, OpOutcome::ok)
}
