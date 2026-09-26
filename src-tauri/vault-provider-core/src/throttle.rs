//! Provider-wide MP-class recovery throttle (spec v0.4 §11.5; RK class is
//! exempt, owner decision on SEC-B3). Slots are create-only objects in
//! the ops store, so adding instances never multiplies the guess rate:
//! at most `L` MP-class verifications can fail per vault per window.

use vault_proto::crypto::hex;
use vault_proto::errors::ErrorCode;

use crate::Provider;

pub struct Slot(String);

fn prefix(vid: &[u8; 16], window: u64) -> String {
    format!("v2/ratelimit/{}/recovery-mp/{window}/", hex::encode(vid))
}

/// Steps 1–2: all L slots taken → `429`; else reserve the lowest free one.
pub fn reserve(p: &Provider, vid: &[u8; 16], now: u64) -> Result<Slot, ErrorCode> {
    let window = now / p.cfg.throttle_window;
    let pre = prefix(vid, window);
    let unavailable = |_| ErrorCode::BackupUnavailable;
    if p.ops.list_prefix(&pre).map_err(unavailable)?.len() >= p.cfg.throttle_slots as usize {
        return Err(ErrorCode::RecoveryThrottled);
    }
    for k in 0..p.cfg.throttle_slots {
        let key = format!("{pre}{k}");
        if p.ops.create(&key, b"").map_err(unavailable)?.is_some() {
            return Ok(Slot(key));
        }
    }
    Err(ErrorCode::RecoveryThrottled)
}

/// Step 3, success: release the slot (a legitimate recovery consumes none).
pub fn release(p: &Provider, slot: &Slot) {
    let _ = p.ops.delete(&slot.0);
}
