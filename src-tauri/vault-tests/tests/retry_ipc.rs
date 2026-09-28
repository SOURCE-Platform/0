//! §11.3.2 retries through the coordinator (spec v0.4 BK-07, RU-02,
//! RU-04; review finding VER-B1/SPEC-B1/SEC-B5): a provider failing the
//! upload leaves the fully staged publication resumable — also while
//! LOCKED — every failure is counted, the security-driven status stays
//! pending, and the next healthy attempt commits it. Synthetic data only.

#[path = "../../vault-helper/tests/vault_fx/mod.rs"]
mod vault_fx;
mod mfx;

use std::sync::atomic::{AtomicU32, Ordering};

use serde_json::{json, Value};
use vault_coordinator::flows::Flows;
use vault_coordinator::{Failure, Helper, HttpResponse, Transport, TransportError};
use vault_fx::{submitted, Fx};
use vault_helper::state::VaultState;
use vault_helper::vault::LockReason;

struct FxHelper<'a>(&'a Fx);
impl Helper for FxHelper<'_> {
    fn op(&self, frame: Value) -> Result<Value, String> {
        Ok(self.0.op(frame))
    }
}

/// Answers `503` to the next `fail` blob uploads; loses the answer to the
/// next `lose` state commits (they land, the caller sees a `503`).
struct Flaky<'a> {
    cloud: &'a mfx::Cloud,
    fail: AtomicU32,
    lose: AtomicU32,
}
impl Transport for Flaky<'_> {
    fn send(&self, _origin: &str, method: &str, path: &str, auth: Option<&str>, body: &[u8]) -> Result<HttpResponse, TransportError> {
        let unavailable = || HttpResponse { status: 503, body: br#"{"error":"BACKUP_UNAVAILABLE"}"#.to_vec(), date: Some(mfx::now()) };
        if method == "PUT" && path.contains("/blobs/") && self.fail.load(Ordering::SeqCst) > 0 {
            self.fail.fetch_sub(1, Ordering::SeqCst);
            return Ok(unavailable());
        }
        let r = self.cloud.send(method, path, auth, body);
        if method == "POST" && path.ends_with("/state") && self.lose.load(Ordering::SeqCst) > 0 {
            self.lose.fetch_sub(1, Ordering::SeqCst);
            return Ok(unavailable());
        }
        Ok(HttpResponse { status: r.status, body: r.body, date: Some(mfx::now()) })
    }
}

fn status(fx: &Fx) -> Value {
    fx.op(json!({ "op": "remote_update_status" }))
}

#[test]
fn a_failed_upload_is_counted_resumed_and_committed_while_locked() {
    let _g = vault_fx::serial();
    let cloud = mfx::Cloud::new("retry");
    let net = Flaky { cloud: &cloud, fail: AtomicU32::new(0), lose: AtomicU32::new(0) };
    let fx = vault_fx::fx();
    let mac = Flows { helper: &FxHelper(&fx), transport: &net };
    fx.push_panel(submitted(vault_fx::MP));
    let setup = fx.op(json!({ "op": "setup_vault", "handle": "synthetic-retry@example.test" }));
    assert_eq!(mac.run_publication(&setup["publication"]).unwrap()["committed"], true);
    assert_eq!(vault_fx::unlock(&fx, vault_fx::MP)["ok"], true);
    vault_fx::add_login(&fx);
    assert_eq!(mac.backup_now().unwrap()["committed"], true);

    // A suspected-stolen RK: security-driven, pending until committed.
    fx.push_panel(submitted(vault_fx::MP));
    assert_eq!(fx.op(json!({ "op": "rotate_recovery_key", "suspected_theft": true }))["ok"], true);
    net.fail.store(3, Ordering::SeqCst);
    for attempt in 1..=3u64 {
        if attempt == 3 {
            fx.core.lock().unwrap().lock(LockReason::Explicit); // resumes while LOCKED too
        }
        match mac.backup_now() {
            Err(Failure::Provider(503, _)) => {}
            other => panic!("attempt {attempt}: expected a 503, got {other:?}"),
        }
        let s = status(&fx);
        assert_eq!((s["pending"].clone(), s["security_driven"].clone(), s["attempts"].clone()), (json!(true), json!(true), json!(attempt)), "{s}");
        assert_eq!(fx.core.lock().unwrap().reported_state(), VaultState::BackingUp, "the staged publication is kept");
    }
    // The provider recovers: the same staged publication commits, LOCKED.
    let done = mac.backup_now().unwrap();
    assert_eq!(done["committed"], true, "{done}");
    assert_eq!(done["cleared"], json!(["rk_replacement"]));
    assert_eq!(status(&fx)["pending"], false);
    assert_eq!(fx.core.lock().unwrap().reported_state(), VaultState::Locked);
    fx.remove_dir();
}

/// A recovery finalize that lands but whose answer is lost is kept and
/// re-sent byte-identically (the provider replays its result): the
/// recovery completes instead of being thrown away (review NEW-I2).
#[test]
fn a_lost_finalize_answer_is_resent_not_discarded() {
    let _g = vault_fx::serial();
    let cloud = mfx::Cloud::new("lostfin");
    let net = Flaky { cloud: &cloud, fail: AtomicU32::new(0), lose: AtomicU32::new(0) };
    let fx = vault_fx::fx();
    let mac = Flows { helper: &FxHelper(&fx), transport: &net };
    fx.push_panel(submitted(vault_fx::MP));
    let setup = fx.op(json!({ "op": "setup_vault", "handle": "synthetic-lostfin@example.test" }));
    assert_eq!(mac.run_publication(&setup["publication"]).unwrap()["committed"], true);
    assert_eq!(vault_fx::unlock(&fx, vault_fx::MP)["ok"], true);
    vault_fx::add_login(&fx);
    assert_eq!(mac.backup_now().unwrap()["committed"], true);

    let fresh = vault_fx::fx();
    let new_mac = Flows { helper: &FxHelper(&fresh), transport: &net };
    fresh.push_panel(submitted(vault_fx::MP));
    new_mac.recovery_start(mfx::ORIGIN, "synthetic-lostfin@example.test", "mp").unwrap();
    net.lose.store(1, Ordering::SeqCst);
    assert!(matches!(new_mac.recovery_finish(), Err(Failure::Provider(503, _))));
    assert_eq!(fresh.state(), VaultState::Recovering, "the completed attempt is kept");
    let done = new_mac.recovery_finish().unwrap();
    assert_eq!(done["committed"], true, "{done}");
    assert_eq!(fresh.state(), VaultState::Unlocked);
    assert_eq!(fresh.op(json!({ "op": "list_items" }))["items"].as_array().unwrap().len(), 1);
    fx.remove_dir();
    fresh.remove_dir();
}

