//! §22.14 SY-13 through the coordinator (review SEC-B2/B3, VER-B1/B2): a
//! restored older store opens read-only, the backup cycle syncs instead
//! of stopping at VAULT_BEHIND, and a real provider exchange catches it
//! up — without ever lowering the Keychain floor. While behind, a served
//! registry that lacks the head this Mac accepted is refused.
//! Synthetic data only.

#[path = "../../vault-helper/tests/vault_fx/mod.rs"]
mod vault_fx;
mod mfx;

use serde_json::{json, Value};
use vault_coordinator::flows::Flows;
use vault_coordinator::{Failure, Helper, HttpResponse, Transport, TransportError};
use vault_fx::{submitted, Fx};
use vault_helper::vault::LockReason;

struct FxHelper<'a>(&'a Fx);
impl Helper for FxHelper<'_> {
    fn op(&self, frame: Value) -> Result<Value, String> {
        Ok(self.0.op(frame))
    }
}

struct Net<'a>(&'a mfx::Cloud);
impl Transport for Net<'_> {
    fn send(&self, _origin: &str, method: &str, path: &str, auth: Option<&str>, body: &[u8]) -> Result<HttpResponse, TransportError> {
        let r = self.0.send(method, path, auth, body);
        Ok(HttpResponse { status: r.status, body: r.body, date: Some(mfx::now()) })
    }
}

fn lock(fx: &Fx) {
    fx.core.lock().unwrap().lock(LockReason::Explicit);
}

fn behind(fx: &Fx) -> bool {
    fx.core.lock().unwrap().behind
}

fn restore(fx: &mut Fx, snapshot: &std::path::Path) {
    lock(fx);
    std::fs::remove_dir_all(&fx.dir).unwrap();
    mfx::copy_tree(snapshot, &fx.dir);
    fx.reboot();
    assert_eq!(vault_fx::unlock(fx, vault_fx::MP)["ok"], true);
}

fn world(tag: &str) -> (mfx::Cloud, Fx) {
    let cloud = mfx::Cloud::new(tag);
    let fx = vault_fx::fx();
    fx.push_panel(submitted(vault_fx::MP));
    let setup = fx.op(json!({ "op": "setup_vault", "handle": format!("synthetic-{tag}@example.test") }));
    let net = Net(&cloud);
    let mac = Flows { helper: &FxHelper(&fx), transport: &net };
    assert_eq!(mac.run_publication(&setup["publication"]).unwrap()["committed"], true);
    assert_eq!(vault_fx::unlock(&fx, vault_fx::MP)["ok"], true);
    (cloud, fx)
}

fn snapshot(fx: &Fx, tag: &str) -> std::path::PathBuf {
    lock(fx);
    let snap = mfx::tmp(tag);
    mfx::copy_tree(&fx.dir, &snap);
    assert_eq!(vault_fx::unlock(fx, vault_fx::MP)["ok"], true);
    snap
}

fn count(fx: &Fx) -> usize {
    fx.op(json!({ "op": "list_items" }))["items"].as_array().unwrap().len()
}

/// The provider holds newer states than the restored copy.
#[test]
fn sy13_restored_copy_catches_up_through_a_newer_provider_state() {
    let _g = vault_fx::serial();
    let (cloud, mut fx) = world("sy13a");
    let net = Net(&cloud);
    vault_fx::add_login(&fx);
    assert_eq!(Flows { helper: &FxHelper(&fx), transport: &net }.backup_now().unwrap()["committed"], true);
    let snap = snapshot(&fx, "sy13a-snap");
    for _ in 0..3 {
        vault_fx::add_login(&fx);
        assert_eq!(Flows { helper: &FxHelper(&fx), transport: &net }.backup_now().unwrap()["committed"], true);
    }
    let floor_before = vault_helper::keychain::read_floor().unwrap();

    restore(&mut fx, &snap);
    assert!(behind(&fx), "restored copy opens read-only");
    assert_eq!(vault_fx::err_code(&fx.op(json!({"op": "delete_item", "ref": "x"}))), "VAULT_BEHIND");
    assert_eq!(count(&fx), 1);

    // The ordinary backup cycle: refused to publish, so it syncs instead.
    let out = Flows { helper: &FxHelper(&fx), transport: &net }.backup_now().unwrap();
    assert_eq!(out["behind"], true, "{out}");
    assert!(!behind(&fx), "caught up through the provider");
    assert_eq!(count(&fx), 4);
    let floor_after = vault_helper::keychain::read_floor().unwrap();
    assert!(floor_after.manifest_generation >= floor_before.manifest_generation, "floor never lowered");
    assert_eq!(floor_after.provider_generation, floor_before.provider_generation);
    vault_fx::add_login(&fx);
    assert_eq!(Flows { helper: &FxHelper(&fx), transport: &net }.backup_now().unwrap()["committed"], true);
    fx.remove_dir();
}

/// The restored copy lost only unpublished local edits: the provider has
/// nothing newer, and saying so is what catches it up.
#[test]
fn sy13_restored_copy_catches_up_when_the_provider_has_nothing_newer() {
    let _g = vault_fx::serial();
    let (cloud, mut fx) = world("sy13b");
    let net = Net(&cloud);
    vault_fx::add_login(&fx);
    assert_eq!(Flows { helper: &FxHelper(&fx), transport: &net }.backup_now().unwrap()["committed"], true);
    let snap = snapshot(&fx, "sy13b-snap");
    vault_fx::add_login(&fx); // never published
    lock(&fx);
    assert_eq!(vault_fx::unlock(&fx, vault_fx::MP)["ok"], true); // the floor saw it

    restore(&mut fx, &snap);
    assert!(behind(&fx));
    let out = Flows { helper: &FxHelper(&fx), transport: &net }.backup_now().unwrap();
    assert_eq!(out["behind"], true, "{out}");
    assert!(!behind(&fx));
    assert_eq!(count(&fx), 1, "the unpublished edit is gone with the old copy");
    fx.remove_dir();
}

/// SEC-B3: while behind, the restored copy's registry is not the anchor —
/// a served registry without the head this Mac accepted is refused.
#[test]
fn sy13_behind_refuses_a_registry_without_the_accepted_head() {
    let _g = vault_fx::serial();
    let (cloud, mut fx) = world("sy13c");
    let net = Net(&cloud);
    vault_fx::add_login(&fx);
    assert_eq!(Flows { helper: &FxHelper(&fx), transport: &net }.backup_now().unwrap()["committed"], true);
    let snap = snapshot(&fx, "sy13c-snap");
    vault_fx::add_login(&fx);
    assert_eq!(Flows { helper: &FxHelper(&fx), transport: &net }.backup_now().unwrap()["committed"], true);
    restore(&mut fx, &snap);
    assert!(behind(&fx));
    // This Mac accepted a head the provider's chain does not contain (as
    // after a revocation a malicious provider withholds).
    let mut f = vault_helper::keychain::read_floor().unwrap();
    f.registry_head = Some(vault_helper::crypto::hex::encode([0x5a; 32]));
    vault_helper::keychain::write_floor(&f).unwrap();
    match (Flows { helper: &FxHelper(&fx), transport: &net }).backup_now() {
        Err(Failure::Helper(code)) => assert_eq!(code, "SIGNATURE_INVALID"),
        other => panic!("expected a refusal, got {other:?}"),
    }
    assert!(behind(&fx), "nothing adopted, still read-only");
    assert_eq!(count(&fx), 1);
    fx.remove_dir();
}
