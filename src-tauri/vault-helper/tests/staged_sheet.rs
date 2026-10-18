//! Spec v0.5 §22.10 (plan F.2d step 5): a rotation behind a new Recovery
//! Key sheet is fully staged before the sheet appears; then only the
//! commit follows. A lock or a cancel while the sheet is up commits
//! nothing, and the vault says plainly that the sheet's key is void; an
//! acknowledged sheet goes live. Synthetic data only.

mod vault_fx;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use serde_json::json;
use vault_fx::*;
use vault_helper::storage::rotation_journal::next_path;
use vault_helper::vault::LockReason;

fn generation(fx: &Fx) -> u32 {
    fx.core.lock().unwrap().header.as_ref().unwrap().vk_generation
}

/// `get_state`, answered by the server layer (and the iPhone's FFI).
fn state(fx: &Fx) -> serde_json::Value {
    fx.core.lock().unwrap().state_answer()
}

fn rotate(fx: &Fx) -> serde_json::Value {
    fx.push_panel(submitted(MP));
    fx.op(json!({"op": "rotate_recovery_key"}))
}

/// ST-03 (ROTATING_KEYS): a lock during the key change commits nothing.
#[test]
fn staged_before_the_sheet_and_a_lock_during_it_voids_the_sheet() {
    let _g = serial();
    let fx = fx();
    setup_and_unlock(&fx);
    let before = generation(&fx);
    let staged = Arc::new(AtomicBool::new(false));
    let (seen, core, db) = (staged.clone(), fx.core.clone(), next_path(&fx.dir, "vault.db"));
    *fx.panel.on_sheet.lock().unwrap() = Some(Box::new(move || {
        seen.store(db.exists(), Ordering::SeqCst);
        let _ = core.lock().unwrap().lock(LockReason::Explicit); // the app left the foreground
    }));
    let r = rotate(&fx);
    assert_eq!(err_code(&r), "PANEL_CANCELLED", "{r}");
    assert!(staged.load(Ordering::SeqCst), "the rotation was staged before the sheet");
    assert!(!next_path(&fx.dir, "vault.db").exists(), "the staging is discarded");
    *fx.panel.on_sheet.lock().unwrap() = None;
    assert_eq!(unlock(&fx, MP)["ok"], true);
    assert_eq!(generation(&fx), before, "nothing committed");
    assert_eq!(state(&fx)["rk_sheet_void"], true);
    assert_eq!(fx.op(json!({"op": "dismiss_rk_sheet_notice"}))["ok"], true);
    assert!(state(&fx).get("rk_sheet_void").is_none());
    fx.remove_dir();
}

#[test]
fn a_users_cancel_voids_the_sheet_and_keeps_the_vault_open() {
    let _g = serial();
    let fx = fx();
    setup_and_unlock(&fx);
    let before = generation(&fx);
    *fx.panel.refuse_sheet.lock().unwrap() = true;
    assert_eq!(err_code(&rotate(&fx)), "PANEL_CANCELLED");
    assert_eq!(generation(&fx), before);
    // Shown and not live: the notice says so (chunk-2 reviews SEC-B4 / VER-B1).
    assert_eq!(state(&fx)["rk_sheet_void"], true);
    assert_eq!(fx.op(json!({"op": "list_items"}))["ok"], true, "the vault is open as before");
    add_login(&fx);
    fx.remove_dir();
}

#[test]
fn an_acknowledged_sheet_goes_live() {
    let _g = serial();
    let fx = fx();
    setup_and_unlock(&fx);
    let before = generation(&fx);
    let r = rotate(&fx);
    assert_eq!(r["ok"], true, "{r}");
    assert_eq!(generation(&fx), before + 1);
    assert!(!fx.dir.join("rk_sheet.pending").exists());
    assert!(state(&fx).get("rk_sheet_void").is_none());
    add_login(&fx);
    // Unpublished, it waits: a second rotation would skip a generation the
    // provider never accepts (§11.4), so it is refused before any prompt.
    let asked = fx.la.calls.load(std::sync::atomic::Ordering::SeqCst);
    assert_eq!(err_code(&rotate(&fx)), "ROTATION_PENDING");
    assert_eq!(generation(&fx), before + 1);
    assert_eq!(fx.panel.queue.lock().unwrap().len(), 1, "no password was asked for");
    assert_eq!(fx.la.calls.load(std::sync::atomic::Ordering::SeqCst), asked, "no presence prompt");
    fx.panel.queue.lock().unwrap().clear();
    fx.remove_dir();
}

/// Never shown while the screen is recorded or mirrored (§22.10; chunk-2
/// reviews SEC-B3 / VER-B2): refused before any marker, nothing committed.
#[test]
fn the_sheet_is_never_shown_while_the_screen_is_captured() {
    let _g = serial();
    let mut fx = fx();
    setup_and_unlock(&fx);
    let (before, shown) = (generation(&fx), fx.panel.sheets.lock().unwrap().len());
    fx.set_capture(Arc::new(Capture { suppressed: false, calls: std::sync::atomic::AtomicUsize::new(0) }));
    assert_eq!(err_code(&rotate(&fx)), "CAPTURE_UNSAFE");
    assert_eq!(fx.panel.sheets.lock().unwrap().len(), shown, "no sheet was shown");
    assert_eq!(generation(&fx), before);
    assert!(state(&fx).get("rk_sheet_void").is_none(), "no sheet, so no notice");
    fx.remove_dir();
}
