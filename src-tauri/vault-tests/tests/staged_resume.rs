//! SG-01 end to end (§22.11, review VER-I1): a publication staged before
//! a helper restart is resumed without the VK and commits at the
//! provider while LOCKED; the pending change clears and the on-disk
//! staging is gone. Synthetic data only.

#[path = "../../vault-helper/tests/vault_fx/mod.rs"]
mod vault_fx;
mod mfx;

use serde_json::{json, Value};
use vault_coordinator::flows::Flows;
use vault_coordinator::{Helper, HttpResponse, Transport, TransportError};
use vault_fx::{submitted, Fx};
use vault_helper::state::VaultState;

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

#[test]
fn sg01_resumed_publication_commits_while_locked() {
    let _g = vault_fx::serial();
    let cloud = mfx::Cloud::new("sg01");
    let net = Net(&cloud);
    let mut fx = vault_fx::fx();
    fx.push_panel(submitted(vault_fx::MP));
    let setup = fx.op(json!({ "op": "setup_vault", "handle": "synthetic-sg01@example.test" }));
    assert_eq!(setup["ok"], true, "{setup}");
    // The helper restarts before anything reached the provider.
    fx.reboot();
    assert_eq!(fx.state(), VaultState::Locked);
    assert_eq!(fx.core.lock().unwrap().reported_state(), VaultState::BackingUp);
    let done = Flows { helper: &FxHelper(&fx), transport: &net }.backup_now().unwrap();
    assert_eq!(done["committed"], true, "{done}");
    assert_eq!(fx.state(), VaultState::Locked, "no VK was needed");
    assert_eq!(fx.op(json!({ "op": "remote_update_status" }))["pending"], false);
    assert!(!vault_helper::sync::staged_disk::dir(&fx.dir).exists(), "staging forgotten after the commit");
    // And the committed vault is the one the provider now serves.
    assert_eq!(vault_fx::unlock(&fx, vault_fx::MP)["ok"], true);
    let synced = Flows { helper: &FxHelper(&fx), transport: &net }.run_sync().unwrap();
    assert_eq!(synced["up_to_date"], true, "{synced}");
    fx.remove_dir();
}
