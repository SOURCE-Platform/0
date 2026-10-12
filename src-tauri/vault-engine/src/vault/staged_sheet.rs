//! A key rotation behind a new Recovery Key sheet, staged first (spec v0.5
//! §22.10; plan F.2d step 5): the rotation is fully staged with the store
//! out of the core and no mutex held; then the sheet; then — with the core
//! mutex held, so a lock waits for it — only the commit. A lock during the
//! staging or the sheet discards the staging, and nothing is committed.
//!
//! **Which sheet counts.** A marker naming the next key generation is
//! written before the sheet is shown and removed once the commit lands. If
//! the app dies or the vault locks after the sheet appeared, the marker
//! outlives a rotation that never committed: `get_state` reports
//! `rk_sheet_void` ("the key on that sheet does not work; your earlier
//! one still does") until the user dismisses it. A marker whose generation
//! the vault reached is a sheet that went live, and is dropped silently.
//! The user's own cancel promised nothing and leaves no marker.

use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard};

use super::rk_ops::show_sheet;
use super::{lock_core, Deps, RecoverySheet, VaultCore};
use crate::crypto::secret::SecretBytes;
use crate::errors::ErrorCode;
use crate::state::VaultState;
use crate::storage::rotation::Gate;
use crate::storage::store::write_atomic;
use crate::storage::VaultStore;

pub const MARKER: &str = "rk_sheet.pending";

pub enum Outcome<'a, T> {
    /// Committed; the core mutex is still held and the caller installs
    /// the new store and key.
    Committed(MutexGuard<'a, VaultCore>, T),
    /// Nothing committed; the old vault is back in the core (unless a lock
    /// landed meanwhile).
    Cancelled,
    Failed(ErrorCode),
}

/// Run `rotate` (which must pass the gate to its rotation) for `sheet`.
pub fn rotate_with_sheet<'a, T>(
    core: &'a Arc<Mutex<VaultCore>>,
    deps: &Deps,
    sheet: &RecoverySheet,
    rotate: impl FnOnce(VaultStore, &SecretBytes<32>, Gate<'_>) -> Result<T, ErrorCode>,
) -> Outcome<'a, T> {
    let (store, vk, dir, next) = {
        let mut c = lock_core(core);
        if c.state != VaultState::Authorizing {
            return Outcome::Failed(ErrorCode::BadState);
        }
        let (Some(store), Some(vk)) = (c.store.take(), c.vk.take()) else {
            return Outcome::Failed(ErrorCode::BadState);
        };
        let (dir, next) = (store.dir.clone(), store.header.vk_generation + 1);
        (store, vk, dir, next)
    };
    let mut held: Option<MutexGuard<'a, VaultCore>> = None;
    let result = {
        let mut gate = || -> bool {
            if lock_core(core).state != VaultState::Authorizing {
                return false; // locked while staging: no sheet was shown
            }
            if write_atomic(&dir.join(MARKER), next.to_string().as_bytes()).is_err() {
                return false;
            }
            if !show_sheet(deps, sheet) {
                if lock_core(core).state == VaultState::Authorizing {
                    let _ = std::fs::remove_file(dir.join(MARKER)); // the user's own cancel
                }
                return false;
            }
            let c = lock_core(core);
            if c.state != VaultState::Authorizing {
                return false;
            }
            held = Some(c);
            true
        };
        rotate(store, &vk, &mut gate)
    };
    let mut c = held.unwrap_or_else(|| lock_core(core));
    match result {
        Ok(t) => {
            let _ = std::fs::remove_file(dir.join(MARKER));
            Outcome::Committed(c, t)
        }
        Err(ErrorCode::PanelCancelled) => {
            if c.state == VaultState::Authorizing {
                c.store = VaultStore::open(&dir).ok();
                c.vk = Some(vk);
            }
            Outcome::Cancelled
        }
        Err(e) => Outcome::Failed(e),
    }
}

/// Whether a sheet shown for generation > `vk_generation` never went live
/// (the marker of a sheet that did is removed here).
pub fn void(dir: &Path, vk_generation: u32) -> bool {
    let Some(next) = std::fs::read_to_string(dir.join(MARKER)).ok().and_then(|s| s.trim().parse::<u32>().ok()) else {
        return false;
    };
    if vk_generation >= next {
        let _ = std::fs::remove_file(dir.join(MARKER));
        return false;
    }
    true
}

/// The user saw the "that sheet is void" notice.
pub fn dismiss(dir: &Path) {
    let _ = std::fs::remove_file(dir.join(MARKER));
}
