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
    // The catch-up persists: a relock does not reopen read-only (VER-I10).
    lock(&fx);
    assert_eq!(vault_fx::unlock(&fx, vault_fx::MP)["ok"], true);
    assert!(!behind(&fx), "still caught up after a relock");
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
    lock(&fx);
    assert_eq!(vault_fx::unlock(&fx, vault_fx::MP)["ok"], true);
    assert!(!behind(&fx), "still caught up after a relock");
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

/// Re-review SEC-B1: a security change committed on this Mac but never
/// published, lost because an older copy was restored, is reported for
/// the user to redo — never dropped silently.
#[test]
fn sy13_a_lost_unpublished_security_change_is_reported() {
    let _g = vault_fx::serial();
    let (cloud, mut fx) = world("sy13d");
    let net = Net(&cloud);
    vault_fx::add_login(&fx);
    assert_eq!(Flows { helper: &FxHelper(&fx), transport: &net }.backup_now().unwrap()["committed"], true);
    let snap = snapshot(&fx, "sy13d-snap");
    // Recovery Key replaced while offline: committed locally, not published.
    fx.push_panel(submitted(vault_fx::MP));
    assert_eq!(fx.op(json!({ "op": "rotate_recovery_key", "suspected_theft": true }))["ok"], true);
    assert_eq!(fx.op(json!({ "op": "remote_update_status" }))["pending"], true);
    restore(&mut fx, &snap);
    assert!(behind(&fx));
    Flows { helper: &FxHelper(&fx), transport: &net }.backup_now().unwrap();
    assert!(!behind(&fx));
    let s = fx.op(json!({ "op": "remote_update_status" }));
    assert_eq!(s["lost_change"], json!(["rk_replacement"]), "{s}");
    // Redoing it clears the warning.
    fx.push_panel(submitted(vault_fx::MP));
    assert_eq!(fx.op(json!({ "op": "rotate_recovery_key", "suspected_theft": true }))["ok"], true);
    assert!(fx.op(json!({ "op": "remote_update_status" })).get("lost_change").is_none());
    fx.remove_dir();
}

/// VER-I9: while behind, a fork signed by a device the restored copy
/// trusts is refused as unverifiable — never COMPROMISED.
#[test]
fn sy13_behind_never_enters_compromised_on_an_offer() {
    use vault_helper::registry::device::DeviceIdentity;
    let _g = vault_fx::serial();
    let (cloud, mut fx) = world("sy13e");
    let net = Net(&cloud);
    vault_fx::add_login(&fx);
    assert_eq!(Flows { helper: &FxHelper(&fx), transport: &net }.backup_now().unwrap()["committed"], true);
    let snap = snapshot(&fx, "sy13e-snap");
    vault_fx::add_login(&fx);
    assert_eq!(Flows { helper: &FxHelper(&fx), transport: &net }.backup_now().unwrap()["committed"], true);
    let state = Flows { helper: &FxHelper(&fx), transport: &net }.call("state_get", None, b"", None).unwrap().body;
    restore(&mut fx, &snap);
    assert!(behind(&fx));
    // The served next generation, re-chained to a manifest the restored
    // copy never accepted and signed by this Mac's own (active) key.
    let mut v: serde_json::Value = serde_json::from_slice(&state).unwrap();
    let r = vault_helper::sync::remote::parse(&state).unwrap();
    let mut m = r.manifest.clone();
    m.prev_manifest_hash = [0x77; 32];
    let dev = vault_helper::device::SeDevice::load(&fx.dir).unwrap();
    let m = m.sign(&dev as &dyn DeviceIdentity).unwrap();
    let mb = m.encode();
    let digest = vault_proto::state::recovery_auth_digest(&r.recovery_auth).unwrap();
    use sha2::Digest;
    let commit = vault_proto::state::state_commit(&m.vault_id, m.generation, &sha2::Sha256::digest(&mb).into(), &sha2::Sha256::digest(&r.checkpoint_bytes).into(), &digest);
    v["manifest"] = json!(vault_proto::b64::encode(&mb));
    v["state_commit"] = json!(vault_helper::crypto::hex::encode(commit));
    let offer = fx.op(json!({ "op": "backup_state_offer", "state": v.to_string() }));
    assert_eq!(vault_fx::err_code(&offer), "SIGNATURE_INVALID", "{offer}");
    assert_eq!(fx.state(), vault_helper::state::VaultState::Unlocked, "not COMPROMISED");
    fx.remove_dir();
}
