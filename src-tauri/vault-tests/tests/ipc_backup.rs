//! The §1.5 provider ops end to end through the main-process coordinator
//! (spec v0.4 §1.3, §11.1): the helper's op dispatcher with scripted
//! panels, `vault-coordinator` doing main's part exactly as the app does,
//! and the provider core in process as its transport. Covers: setup's
//! `create` finishing while LOCKED, `backup_now`, sync, and total-loss
//! recovery on an empty machine. Synthetic data only.

#[path = "../../vault-helper/tests/vault_fx/mod.rs"]
mod vault_fx;
mod mfx;

use serde_json::{json, Value};
use vault_coordinator::flows::Flows;
use vault_coordinator::{Helper, HttpResponse, Transport, TransportError};
use vault_fx::{submitted, Fx};
use vault_helper::state::VaultState;

const HANDLE: &str = "synthetic-fixture@example.test";

struct FxHelper<'a>(&'a Fx);
impl Helper for FxHelper<'_> {
    fn op(&self, frame: Value) -> Result<Value, String> {
        Ok(self.0.op(frame))
    }
}

struct CloudTransport<'a>(&'a mfx::Cloud);
impl Transport for CloudTransport<'_> {
    fn send(&self, origin: &str, method: &str, path: &str, auth: Option<&str>, body: &[u8]) -> Result<HttpResponse, TransportError> {
        assert_eq!(origin, mfx::ORIGIN, "only the helper's allowlisted origin is contacted");
        let r = self.0.send(method, path, auth, body);
        Ok(HttpResponse { status: r.status, body: r.body, date: Some(mfx::now()) })
    }
}

#[test]
fn setup_publish_sync_and_recover_through_the_coordinator() {
    let _g = vault_fx::serial();
    let cloud = mfx::Cloud::new("coord");
    let transport = CloudTransport(&cloud);
    let fx = vault_fx::fx();
    let mac = Flows { helper: &FxHelper(&fx), transport: &transport };
    // Setup stages the `create`; it finishes while LOCKED (§11.3.2).
    fx.push_panel(submitted(vault_fx::MP));
    let setup = fx.op(json!({ "op": "setup_vault", "handle": HANDLE }));
    assert_eq!(setup["ok"], true, "{setup}");
    assert_eq!(fx.core.lock().unwrap().reported_state(), VaultState::BackingUp);
    assert_eq!(mac.run_publication(&setup["publication"]).unwrap()["committed"], true);
    assert_eq!(fx.state(), VaultState::Locked);
    // Unlock, add, publish, sync.
    assert_eq!(vault_fx::unlock(&fx, vault_fx::MP)["ok"], true);
    vault_fx::add_login(&fx);
    assert_eq!(mac.backup_now().unwrap()["committed"], true);
    assert_eq!(mac.run_sync().unwrap()["up_to_date"], true);
    // Nothing changed since: no empty generation is published.
    assert_eq!(mac.backup_now().unwrap()["nothing_to_publish"], true);

    // A fresh machine: total-loss recovery with the master password.
    let fresh = vault_fx::fx();
    let new_mac = Flows { helper: &FxHelper(&fresh), transport: &transport };
    fresh.push_panel(submitted(vault_fx::MP));
    let preview = new_mac.recovery_start(mfx::ORIGIN, HANDLE, "mp").unwrap();
    assert_eq!(fresh.state(), VaultState::Recovering);
    assert_eq!(preview["preview"]["item_count"], 1, "FR-01 before completion");
    assert_eq!(new_mac.recovery_finish().unwrap()["committed"], true);
    assert_eq!(fresh.state(), VaultState::Unlocked);
    assert_eq!(fresh.op(json!({ "op": "list_items" }))["items"].as_array().unwrap().len(), 1);
    // The original Mac is cut off (S-4): its reads get the generic 401.
    assert_eq!(mac.call("state_get", None, b"", None).unwrap().status, 401);
    fx.remove_dir();
    fresh.remove_dir();
}
