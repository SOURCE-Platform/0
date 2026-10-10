//! Master-password adoption (spec §2.7, F.2d; review SEC-I2 of 99760e0):
//! a Mac whose agreement key was discarded ("password every time") rotates
//! its vault key and publishes, then has its directory restored from an
//! older backup (§22.14). Catching up needs the served key, which no
//! envelope of its own can open — the master password opens the served
//! `wrap_mp` instead, through the secure panel. A wrong password adopts
//! nothing. Through the real op dispatcher and the main-process
//! coordinator, the provider core in process. Synthetic data only.

#[path = "../../vault-helper/tests/vault_fx/mod.rs"]
mod vault_fx;
mod mfx;

use serde_json::{json, Value};
use vault_coordinator::flows::Flows;
use vault_coordinator::{Helper, HttpResponse, Transport, TransportError};
use vault_fx::{submitted, Fx, MP};

const HANDLE: &str = "synthetic-adoption@example.test";

struct FxHelper<'a>(&'a Fx);
impl Helper for FxHelper<'_> {
    fn op(&self, frame: Value) -> Result<Value, String> {
        Ok(self.0.op(frame))
    }
}

struct CloudTransport<'a>(&'a mfx::Cloud);
impl Transport for CloudTransport<'_> {
    fn send(&self, _origin: &str, method: &str, path: &str, auth: Option<&str>, body: &[u8]) -> Result<HttpResponse, TransportError> {
        let r = self.0.send(method, path, auth, body);
        Ok(HttpResponse { status: r.status, body: r.body, date: Some(mfx::now()) })
    }
}

fn copy_dir(from: &std::path::Path, to: &std::path::Path) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap().flatten() {
        let target = to.join(e.file_name());
        if e.path().is_dir() {
            copy_dir(&e.path(), &target);
        } else if e.file_name() != "helper.sock" {
            std::fs::copy(e.path(), target).unwrap();
        }
    }
}

/// A password-only Mac, published at generation 1, rotated and published
/// again, then restored to the generation-1 copy of its directory.
fn restored_after_own_rotation(tag: &str) -> (mfx::Cloud, Fx) {
    let cloud = mfx::Cloud::new(tag);
    std::env::set_var("OV0_VAULT_SE_BIOMETRY", "absent");
    let fx = vault_fx::fx();
    fx.push_panel(submitted(MP));
    let setup = fx.op(json!({ "op": "setup_vault", "handle": HANDLE }));
    std::env::remove_var("OV0_VAULT_SE_BIOMETRY");
    assert_eq!(setup["ok"], true, "{setup}");
    let transport = CloudTransport(&cloud);
    let mac = Flows { helper: &FxHelper(&fx), transport: &transport };
    assert_eq!(mac.run_publication(&setup["publication"]).unwrap()["committed"], true);
    assert!(vault_helper::device::SeDevice::load(&fx.dir).unwrap().agreement_discarded());
    assert_eq!(vault_fx::unlock(&fx, MP)["ok"], true);
    vault_fx::add_login(&fx);
    assert_eq!(mac.backup_now().unwrap()["committed"], true);
    let snapshot = std::env::temp_dir().join(format!("ov0-mpadopt-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&snapshot);
    copy_dir(&fx.dir, &snapshot);
    // A new key (rotation) published by this same Mac.
    fx.push_panel(submitted(MP));
    assert_eq!(fx.op(json!({ "op": "rotate_recovery_key" }))["ok"], true);
    assert_eq!(mac.backup_now().unwrap()["committed"], true);
    // Its directory restored from before the rotation.
    let _ = fx.core.lock().unwrap().lock(vault_helper::vault::LockReason::Explicit);
    for e in std::fs::read_dir(&fx.dir).unwrap().flatten() {
        let _ = if e.path().is_dir() { std::fs::remove_dir_all(e.path()) } else { std::fs::remove_file(e.path()) };
    }
    copy_dir(&snapshot, &fx.dir);
    let _ = std::fs::remove_dir_all(&snapshot);
    *fx.core.lock().unwrap() = vault_helper::vault::VaultCore::boot(fx.dir.clone());
    assert_eq!(vault_fx::unlock(&fx, MP)["ok"], true, "the restored copy opens with its own wrap");
    assert!(fx.core.lock().unwrap().behind, "older than its own floor: read-only (§22.14)");
    (cloud, fx)
}

fn adopt_prompts(fx: &Fx) -> usize {
    fx.panel.seen.lock().unwrap().iter().filter(|r| **r == vault_helper::vault::secure_ui::PanelRequest::MpAdopt).count()
}

fn code(r: &Result<Value, vault_coordinator::Failure>) -> String {
    match r {
        Err(vault_coordinator::Failure::Helper(c)) => c.clone(),
        other => format!("{other:?}"),
    }
}

/// MA-01: started by the user, the "Apply a Security Change" panel asks
/// once; the password opens the served wrap and the Mac catches up.
#[test]
fn a_password_only_mac_catches_up_with_its_master_password() {
    let _g = vault_fx::serial();
    let (cloud, fx) = restored_after_own_rotation("ok");
    let transport = CloudTransport(&cloud);
    let mac = Flows { helper: &FxHelper(&fx), transport: &transport };
    fx.push_panel(submitted(MP));
    let synced = mac.run_sync_by_user().unwrap();
    assert_eq!(synced["adopted_vk"], true, "{synced}");
    assert_eq!(adopt_prompts(&fx), 1, "one panel, titled for the flow");
    assert!(!fx.core.lock().unwrap().behind, "caught up");
    vault_fx::add_login(&fx); // authoring works again
    fx.remove_dir();
}

/// MA-02: a background sync never raises the panel.
#[test]
fn a_background_sync_never_asks() {
    let _g = vault_fx::serial();
    let (cloud, fx) = restored_after_own_rotation("bg");
    let transport = CloudTransport(&cloud);
    let mac = Flows { helper: &FxHelper(&fx), transport: &transport };
    assert_eq!(code(&mac.run_sync()), "MP_ADOPTION_REQUIRED");
    assert_eq!(adopt_prompts(&fx), 0);
    assert!(fx.core.lock().unwrap().behind);
    fx.remove_dir();
}

/// MA-03: a wrong password adopts nothing and drives the §15 backoff; a
/// cancel adopts nothing.
#[test]
fn a_wrong_password_or_a_cancel_adopts_nothing() {
    let _g = vault_fx::serial();
    let (cloud, fx) = restored_after_own_rotation("bad");
    let transport = CloudTransport(&cloud);
    let mac = Flows { helper: &FxHelper(&fx), transport: &transport };
    let before = fx.core.lock().unwrap().failed_attempts;
    fx.push_panel(submitted(b"not-the-synthetic-master-password"));
    assert_eq!(code(&mac.run_sync_by_user()), "WRONG_CREDENTIAL");
    assert_eq!(adopt_prompts(&fx), 1);
    assert_eq!(fx.core.lock().unwrap().failed_attempts, before + 1, "the backoff counted it");
    assert_eq!(fx.state(), vault_helper::state::VaultState::Unlocked, "the vault stays open");
    assert!(fx.core.lock().unwrap().behind);
    fx.push_panel(vault_helper::vault::secure_ui::PanelOutcome::Cancelled);
    assert_eq!(code(&mac.run_sync_by_user()), "PANEL_CANCELLED");
    assert!(fx.core.lock().unwrap().behind);
    fx.remove_dir();
}

/// MA-04: while behind, a served state that does not contain the head the
/// floor recorded is no anchor — refused with no panel at all.
#[test]
fn an_unanchored_state_raises_no_panel() {
    let _g = vault_fx::serial();
    let (cloud, fx) = restored_after_own_rotation("anchor");
    let mut floor = vault_helper::keychain::read_floor().unwrap();
    floor.registry_head = Some("ab".repeat(32));
    vault_helper::keychain::write_floor(&floor).unwrap();
    let transport = CloudTransport(&cloud);
    let mac = Flows { helper: &FxHelper(&fx), transport: &transport };
    fx.push_panel(submitted(MP));
    assert!(mac.run_sync_by_user().is_err());
    assert_eq!(adopt_prompts(&fx), 0, "no prompt before verification");
    fx.remove_dir();
}
