//! F.2d chunk-3 review fixes, through SOURCE Vault's C ABI: a publication
//! staged before a revocation never goes out in its place (§22.11; reviews
//! SEC-I2 / VER-I1); recovery is refused over a pairing in progress or an
//! existing vault (SEC-B1); the foreground trigger only ever finishes a
//! staged upload (SEC-O1). Synthetic data only.

mod mfx;
mod phone_fx;

use mfx::*;
use phone_fx::*;
use vault_ffi::*;
use vault_helper::registry::device::DeviceIdentity;

#[test]
fn a_publication_staged_before_a_revocation_never_goes_out_in_its_place() {
    let _g = serial();
    let (cloud, mut a, b) = pair("ffi-stale");
    let e = open(&cloud, &b.dir);
    assert_eq!(call(e, r#"{"op":"provider_sync"}"#)["ok"], true);
    // An edit whose upload fails offline: staged, kept.
    let add = call(e, r#"{"op":"add_item","kind":"login","title":"offline-edit","username":"u@example.test","hosts":["example.test"],"password":"synthetic-y"}"#);
    assert_eq!(add["ok"], true, "{add}");
    cloud.offline.set(true);
    assert_eq!(call(e, r#"{"op":"provider_publish"}"#)["error"], "BACKUP_UNAVAILABLE");
    // Then the revocation, also offline; then a lock.
    let mac = vault_helper::crypto::hex::encode(a.dev.device_id());
    assert_eq!(call(e, &format!(r#"{{"op":"revoke_device","device_id":"{mac}"}}"#))["ok"], true);
    assert_eq!(call(e, r#"{"op":"provider_publish"}"#)["error"], "BACKUP_UNAVAILABLE");
    unsafe { ov0_engine_lock(e) };
    // Back online, at the next foreground: what goes out is the revocation.
    cloud.offline.set(false);
    let p = call(e, r#"{"op":"provider_publish","resume":true}"#);
    assert_eq!(p["result"]["committed"], true, "{p}");
    assert_eq!(a.sync(&cloud.cloud).err(), Some(vault_helper::errors::ErrorCode::AuthInvalid), "the revocation went out");
    unsafe { ov0_engine_close(e) };
}

#[test]
fn recovery_is_refused_over_a_pairing_or_a_vault_and_resume_never_stages() {
    let _g = serial();
    let (cloud, _a, b) = pair("ffi-guards");
    // Over an existing (unlocked) vault.
    let e = open(&cloud, &b.dir);
    assert_eq!(call(e, r#"{"op":"provider_recover_start","handle":"synthetic-ffi-guards@example.test","kind":"mp"}"#)["error"], "BAD_STATE");
    // The foreground trigger never stages anything new.
    assert_eq!(call(e, r#"{"op":"provider_publish","resume":true}"#)["error"], "BAD_STATE");
    unsafe { ov0_engine_close(e) };
    // Over a pairing in progress (its marker in an empty directory).
    let dir = tmp("ffi-guards-pairing");
    let e = open_raw(&cloud, &dir);
    std::fs::write(dir.join("join.attempt"), b"").unwrap();
    assert_eq!(call(e, r#"{"op":"provider_recover_start","handle":"synthetic-ffi-guards@example.test","kind":"mp"}"#)["error"], "BAD_STATE");
    unsafe { ov0_engine_close(e) };
}
