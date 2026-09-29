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
    /// Unreachable while set.
    down: std::sync::atomic::AtomicBool,
    /// Answer the next state commits with `409 STATE_MOVED` (not posted).
    moved: AtomicU32,
}

impl<'a> Flaky<'a> {
    fn on(cloud: &'a mfx::Cloud) -> Self {
        Flaky { cloud, fail: AtomicU32::new(0), lose: AtomicU32::new(0), down: Default::default(), moved: AtomicU32::new(0) }
    }
}
impl Transport for Flaky<'_> {
    fn send(&self, _origin: &str, method: &str, path: &str, auth: Option<&str>, body: &[u8]) -> Result<HttpResponse, TransportError> {
        let unavailable = || HttpResponse { status: 503, body: br#"{"error":"BACKUP_UNAVAILABLE"}"#.to_vec(), date: Some(mfx::now()) };
        if self.down.load(Ordering::SeqCst) {
            return Err(TransportError::Unreachable("down".into()));
        }
        if method == "POST" && path.ends_with("/state") && self.moved.load(Ordering::SeqCst) > 0 {
            self.moved.fetch_sub(1, Ordering::SeqCst);
            return Ok(HttpResponse { status: 409, body: br#"{"error":"STATE_MOVED"}"#.to_vec(), date: Some(mfx::now()) });
        }
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
    let net = Flaky::on(&cloud);
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
    let net = Flaky::on(&cloud);
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

/// Can this recovery credential authenticate a read at the provider now?
fn can_read(cloud: &mfx::Cloud, handle: &str, cred: vault_helper::recovery::total_loss::Credential<'_>) -> u16 {
    use vault_helper::sync::sign::{SignRequest, SignScope};
    use vault_proto::request::{body_hash, Operation};
    let r = vault_helper::recovery::total_loss::Recovery::begin(mfx::ORIGIN, &cloud.locate(handle), cred, 0).unwrap();
    let none = std::collections::BTreeSet::new();
    let req = SignRequest { operation: Operation::StateGet, blob: None, body_sha256: body_hash(b""), expected_state: None };
    let h = r.sign(&req, &SignScope { put_blobs: &none, staged: None }, mfx::now()).unwrap();
    let (m, p) = Operation::StateGet.route(&r.locate.vault_id, None).unwrap();
    cloud.send(m, &p, Some(&h), b"").status
}

fn change(old: &[u8], new: &[u8]) -> vault_helper::vault::PanelOutcome {
    use vault_helper::crypto::secret::SecretVec;
    vault_helper::vault::PanelOutcome::SubmittedChange(SecretVec::new(old.to_vec()), SecretVec::new(new.to_vec()))
}

/// RU-01: an MP change while the provider is unreachable stays pending
/// across lock and a helper restart (re-staged at the next unlock); the old
/// MP keeps authenticating remotely until the commit, then never again.
#[test]
fn ru01_mp_change_while_the_provider_is_down() {
    use vault_helper::recovery::total_loss::Credential;
    const NEW: &[u8] = b"synthetic-ru01-master-password-new";
    let _g = vault_fx::serial();
    let cloud = mfx::Cloud::new("ru01");
    let handle = "synthetic-ru01@example.test";
    let net = Flaky::on(&cloud);
    let mut fx = vault_fx::fx();
    fx.push_panel(submitted(vault_fx::MP));
    let setup = fx.op(json!({ "op": "setup_vault", "handle": handle }));
    Flows { helper: &FxHelper(&fx), transport: &net }.run_publication(&setup["publication"]).unwrap();
    assert_eq!(vault_fx::unlock(&fx, vault_fx::MP)["ok"], true);
    fx.push_panel(change(vault_fx::MP, NEW));
    assert_eq!(fx.op(json!({ "op": "change_master_password" }))["ok"], true);
    net.down.store(true, Ordering::SeqCst);
    assert!(matches!(Flows { helper: &FxHelper(&fx), transport: &net }.backup_now(), Err(Failure::Unreachable)));
    assert_eq!(status(&fx)["ops"], json!(["mp_change"]));
    fx.core.lock().unwrap().lock(LockReason::Explicit);
    fx.reboot(); // a helper restart: the staging is gone, the record is not
    assert_eq!(status(&fx)["pending"], true, "REMOTE_UPDATE_PENDING survives lock and restart");
    assert_eq!(can_read(&cloud, handle, Credential::Mp(vault_fx::MP)), 200, "old MP still works remotely");
    net.down.store(false, Ordering::SeqCst);
    assert_eq!(vault_fx::unlock(&fx, NEW)["ok"], true);
    let done = Flows { helper: &FxHelper(&fx), transport: &net }.backup_now().unwrap();
    assert_eq!(done["cleared"], json!(["mp_change"]), "{done}");
    assert_ne!(can_read(&cloud, handle, Credential::Mp(vault_fx::MP)), 200, "old MP refused after the commit");
    assert_eq!(can_read(&cloud, handle, Credential::Mp(NEW)), 200);
    fx.remove_dir();
}

/// RU-02 + RU-05: a suspected-stolen RK replacement whose upload fails is
/// kept, security-driven; the old RK still recovers remotely until the
/// commit. When the provider moved meanwhile (a merge is needed) while
/// LOCKED, it waits for unlock with its warning, then commits and the old
/// RK stops working.
#[test]
fn ru02_ru05_stolen_rk_waits_for_unlock_then_cuts_off() {
    use vault_helper::recovery::total_loss::Credential;
    let _g = vault_fx::serial();
    let cloud = mfx::Cloud::new("ru02");
    let handle = "synthetic-ru02@example.test";
    let net = Flaky::on(&cloud);
    let fx = vault_fx::fx();
    let mac = Flows { helper: &FxHelper(&fx), transport: &net };
    fx.push_panel(submitted(vault_fx::MP));
    let setup = fx.op(json!({ "op": "setup_vault", "handle": handle }));
    mac.run_publication(&setup["publication"]).unwrap();
    let old_rk = vault_helper::crypto::bip39::decode_rk(&fx.panel.shown_rk.lock().unwrap().clone().unwrap()).unwrap();
    assert_eq!(vault_fx::unlock(&fx, vault_fx::MP)["ok"], true);
    fx.push_panel(submitted(vault_fx::MP));
    assert_eq!(fx.op(json!({ "op": "rotate_recovery_key", "suspected_theft": true }))["ok"], true);
    net.fail.store(1, Ordering::SeqCst);
    assert!(mac.backup_now().is_err());
    assert_eq!(can_read(&cloud, handle, Credential::Rk(&old_rk)), 200, "old RK recovers remotely until the commit");
    fx.core.lock().unwrap().lock(LockReason::Explicit);
    net.moved.store(1, Ordering::SeqCst);
    assert!(mac.backup_now().is_err(), "a merge is needed and the vault is locked");
    let s = status(&fx);
    assert_eq!((s["pending"].clone(), s["security_driven"].clone()), (json!(true), json!(true)), "the warning stays: {s}");
    assert_eq!(vault_fx::unlock(&fx, vault_fx::MP)["ok"], true);
    assert_eq!(mac.backup_now().unwrap()["committed"], true);
    assert_eq!(status(&fx)["pending"], false);
    assert_ne!(can_read(&cloud, handle, Credential::Rk(&old_rk)), 200, "cut off at the commit");
    fx.remove_dir();
}

