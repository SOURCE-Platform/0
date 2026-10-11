//! MA-08 (spec §2.7; review SEC-I8 of F.2d step 1): the case that made
//! SEC-B1 a blocker. A password-only Mac's directory is restored from
//! before it enrolled a second device; since then it rotated its key, and
//! the second device published on top — so the served head is signed by a
//! device the restored registry does not know. The gate verifies the
//! served state itself (not the restored registry), so the user-started
//! sync asks once, and the Mac catches up. Synthetic data only.

#[path = "../../vault-helper/tests/vault_fx/mod.rs"]
mod vault_fx;
mod mfx;

use serde_json::{json, Value};
use vault_coordinator::flows::Flows;
use vault_coordinator::{Helper, HttpResponse, Transport, TransportError};
use vault_fx::{submitted, Fx, MP};

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

/// Run `f` with the Fx Mac's unlocked vault seen as a simulated Mac (the
/// simulator's enrollment helper), then hand the vault back.
fn as_simulated(fx: &Fx, f: impl FnOnce(&mut mfx::Mac)) {
    let mut c = fx.core.lock().unwrap();
    let root = mfx::tmp("ma08-view");
    let mut view = mfx::Mac { root, dir: fx.dir.clone(), dev: vault_helper::device::SeDevice::load(&fx.dir).unwrap(), store: c.store.take(), vk: c.vk.take(), rk: None };
    f(&mut view);
    c.store = view.store.take();
    c.vk = view.vk.take();
    c.header = c.store.as_ref().map(|s| s.header.clone());
    // The view borrows this Mac's identity: its drop would delete the
    // Mac's own Secure Enclave keys. Remove only its scratch root.
    let _ = std::fs::remove_dir_all(&view.root);
    std::mem::forget(view);
}

#[test]
fn a_head_signed_by_a_device_the_restored_registry_lacks_still_adopts() {
    let _g = vault_fx::serial();
    let cloud = mfx::Cloud::new("ma08");
    let transport = CloudTransport(&cloud);
    std::env::set_var("OV0_VAULT_SE_BIOMETRY", "absent");
    let fx = vault_fx::fx();
    fx.push_panel(submitted(MP));
    let setup = fx.op(json!({ "op": "setup_vault", "handle": "synthetic-ma08@example.test" }));
    std::env::remove_var("OV0_VAULT_SE_BIOMETRY");
    assert_eq!(setup["ok"], true, "{setup}");
    let mac = Flows { helper: &FxHelper(&fx), transport: &transport };
    assert_eq!(mac.run_publication(&setup["publication"]).unwrap()["committed"], true);
    assert_eq!(vault_fx::unlock(&fx, MP)["ok"], true);
    vault_fx::add_login(&fx);
    assert_eq!(mac.backup_now().unwrap()["committed"], true);
    // The snapshot: before the second device exists.
    let snapshot = mfx::tmp("ma08-snapshot");
    copy_dir(&fx.dir, &snapshot);
    // Enroll B; rotate; B joins and publishes on top.
    let mut b = mfx::Mac::new("ma08-b");
    as_simulated(&fx, |a| a.enroll(&cloud, &b));
    fx.push_panel(submitted(MP));
    assert_eq!(fx.op(json!({ "op": "rotate_recovery_key" }))["ok"], true);
    assert_eq!(mac.backup_now().unwrap()["committed"], true);
    let vid = fx.core.lock().unwrap().store.as_ref().unwrap().header.vault_id.0;
    b.join(&cloud, vid);
    b.add("from-b");
    b.publish(&cloud).unwrap();
    // Restore the Mac from the snapshot.
    let _ = fx.core.lock().unwrap().lock(vault_helper::vault::LockReason::Explicit);
    for e in std::fs::read_dir(&fx.dir).unwrap().flatten() {
        let _ = if e.path().is_dir() { std::fs::remove_dir_all(e.path()) } else { std::fs::remove_file(e.path()) };
    }
    copy_dir(&snapshot, &fx.dir);
    let _ = std::fs::remove_dir_all(&snapshot);
    *fx.core.lock().unwrap() = vault_helper::vault::VaultCore::boot(fx.dir.clone());
    assert_eq!(vault_fx::unlock(&fx, MP)["ok"], true);
    assert!(fx.core.lock().unwrap().behind);
    // The served head is B's; the restored registry has never heard of B.
    fx.push_panel(submitted(MP));
    let synced = mac.run_sync_by_user().unwrap();
    assert_eq!(synced["adopted_vk"], true, "{synced}");
    let prompts = fx.panel.seen.lock().unwrap().iter().filter(|r| **r == vault_helper::vault::secure_ui::PanelRequest::MpAdopt).count();
    assert_eq!(prompts, 1);
    assert!(!fx.core.lock().unwrap().behind, "caught up");
    let titles = fx.op(json!({ "op": "list_items" }))["items"].to_string();
    assert!(titles.contains("from-b"), "B's change arrived: {titles}");
    fx.remove_dir();
}
