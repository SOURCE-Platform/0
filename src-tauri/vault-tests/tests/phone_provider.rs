//! F.2d steps 2–4 through SOURCE Vault's C ABI: a second device's vault,
//! opened with `ov0_engine_open` like the iPhone app does, syncs with the
//! provider core and publishes its own edit through `provider_sync` /
//! `provider_publish`; the provider HTTPS is the `http_send` callback.
//! A provider-confirmed state lifts a removal marker (§22.9). Synthetic
//! data only.

mod mfx;
mod phone_fx;

use mfx::*;
use phone_fx::*;
use serde_json::Value;
use vault_ffi::*;

#[test]
fn a_phone_engine_syncs_and_publishes_through_the_provider_path() {
    let _g = serial();
    let (cloud, mut a, b) = pair("ffi-provider");
    a.add("from-a");
    a.publish(&cloud.cloud).unwrap();
    let e = open(&cloud, &b.dir);
    let synced = call(e, r#"{"op":"provider_sync","interactive":true}"#);
    assert_eq!(synced["ok"], true, "{synced}");
    assert!(call(e, r#"{"op":"list_items"}"#)["items"].to_string().contains("from-a"));
    let add = call(e, r#"{"op":"add_item","kind":"login","title":"from-b","username":"u@example.test","hosts":["example.test"],"password":"synthetic-b"}"#);
    assert_eq!(add["ok"], true, "{add}");
    let published = call(e, r#"{"op":"provider_publish","interactive":true}"#);
    assert_eq!(published["ok"], true, "{published}");
    assert_eq!(published["result"]["committed"], true, "{published}");
    a.sync(&cloud.cloud).unwrap();
    assert!(a.titles().contains(&"from-b".to_string()), "the Mac received the phone's edit");
    // The step-wise ops are the flows' alone now (review SEC-O2).
    assert_eq!(call(e, r#"{"op":"backup_prepare"}"#)["error"], "UNKNOWN_OP");
    unsafe { ov0_engine_close(e) };
}

#[test]
fn a_provider_confirmed_state_lifts_the_removal_lock() {
    let _g = serial();
    let (cloud, _a, b) = pair("ffi-lift");
    std::fs::write(b.dir.join("removal.json"), br#"{"published":false,"at":1}"#).unwrap();
    let e = open(&cloud, &b.dir);
    assert!(call(e, r#"{"op":"get_state"}"#).get("removed").is_some());
    // The op the app sends at unlock and on "Sync now" (review SEC-B2).
    let synced = call(e, r#"{"op":"provider_publish","interactive":true}"#);
    assert_eq!(synced["ok"], true, "{synced}");
    assert!(call(e, r#"{"op":"get_state"}"#).get("removed").is_none(), "lifted: the provider says this device is active");
    unsafe { ov0_engine_close(e) };
}

/// "Sync now" with nothing of the phone's own to publish still checks the
/// provider and brings in what others published (reviews VER-B1 / SEC-I2);
/// every request went to the vault's own provider origin.
#[test]
fn sync_now_with_nothing_to_publish_still_syncs() {
    let _g = serial();
    let (cloud, mut a, b) = pair("ffi-syncnow");
    let e = open(&cloud, &b.dir);
    assert_eq!(call(e, r#"{"op":"provider_sync"}"#)["ok"], true);
    a.add("later-from-a");
    a.publish(&cloud.cloud).unwrap();
    let now = call(e, r#"{"op":"provider_publish","interactive":true}"#);
    assert_eq!(now["ok"], true, "{now}");
    assert!(call(e, r#"{"op":"list_items"}"#)["items"].to_string().contains("later-from-a"), "{now}");
    let origins = cloud.origins.borrow();
    assert!(!origins.is_empty() && origins.iter().all(|o| *o == origins[0]), "one origin: {origins:?}");
    unsafe { ov0_engine_close(e) };
}

/// An unreachable provider lifts nothing; a callback calling back into
/// the engine gets `BAD_STATE` (no deadlock); a locked engine runs no flow.
#[test]
fn an_unreachable_provider_lifts_nothing_and_callbacks_cannot_reenter() {
    let _g = serial();
    let (cloud, _a, b) = pair("ffi-guard");
    std::fs::write(b.dir.join("removal.json"), br#"{"published":false,"at":1}"#).unwrap();
    let e = open(&cloud, &b.dir);
    cloud.offline.set(true);
    cloud.reenter.set(e);
    let r = call(e, r#"{"op":"provider_sync"}"#);
    assert_eq!(r["error"], "BACKUP_UNAVAILABLE", "{r}");
    assert_eq!(cloud.reentered.borrow().as_ref().unwrap()["error"], "BAD_STATE");
    assert!(call(e, r#"{"op":"get_state"}"#).get("removed").is_some(), "still removed");
    unsafe { ov0_engine_lock(e) };
    assert_eq!(call(e, r#"{"op":"provider_sync"}"#)["error"], "BAD_STATE", "locked: no flow");
    unsafe { ov0_engine_close(e) };
}


/// RC-01 (§22.10, F.2d step 5): the phone revokes the Mac — staged first,
/// the new Recovery Key acknowledged, then committed — and publishes it;
/// the provider then refuses the Mac.
#[test]
fn a_phone_revokes_the_mac_and_publishes_the_cutoff() {
    let _g = serial();
    use vault_helper::registry::device::DeviceIdentity;
    let (cloud, mut a, b) = pair("ffi-revoke");
    let e = open(&cloud, &b.dir);
    assert_eq!(call(e, r#"{"op":"provider_sync"}"#)["ok"], true);
    let mac = vault_helper::crypto::hex::encode(a.dev.device_id());
    let r = call(e, &format!(r#"{{"op":"revoke_device","device_id":"{mac}"}}"#));
    assert_eq!(r["ok"], true, "{r}");
    let p = call(e, r#"{"op":"provider_publish"}"#);
    assert_eq!(p["result"]["committed"], true, "{p}");
    assert_eq!(a.sync(&cloud.cloud).err(), Some(vault_helper::errors::ErrorCode::AuthInvalid), "the Mac is cut off");
    // Nothing else waits: a second rotation is allowed again.
    assert!(call(e, r#"{"op":"get_state"}"#).get("rk_sheet_void").is_none());
    unsafe { ov0_engine_close(e) };
}

/// F.2d step 6 (§22.9): "Remove this vault" says what only this phone
/// holds, then removes everything — files, keys, Keychain items — and the
/// engine is ready to pair again.
#[test]
fn remove_this_vault_warns_then_removes_everything() {
    let _g = serial();
    let (cloud, _a, b) = pair("ffi-remove");
    let e = open(&cloud, &b.dir);
    let add = call(e, r#"{"op":"add_item","kind":"login","title":"only-here","username":"u@example.test","hosts":["example.test"],"password":"synthetic-x"}"#);
    assert_eq!(add["ok"], true, "{add}");
    let preview = call(e, r#"{"op":"remove_vault_preview"}"#);
    assert!(preview["local_only"].as_u64().unwrap() >= 1, "{preview}");
    assert_eq!(call(e, r#"{"op":"remove_vault"}"#)["error"], "INVALID_INPUT", "needs confirm");
    let r = call(e, r#"{"op":"remove_vault","confirm":true}"#);
    assert_eq!(r["ok"], true, "{r}");
    assert_eq!(call(e, r#"{"op":"get_state"}"#)["state"], "uninitialized");
    assert_eq!(std::fs::read_dir(&b.dir).unwrap().count(), 0, "nothing left in the vault directory");
    unsafe { ov0_engine_close(e) };
}

/// F.2d chunk 3 (§11.5, §11.8; scenario 3 on a phone): every other device
/// is lost; a fresh phone recovers the vault with the master password,
/// sees the preview, finishes on a fresh key and Recovery Key, and the old
/// Mac is cut off by the recovery epoch.
#[test]
fn total_loss_recovery_onto_a_phone() {
    let _g = serial();
    let cloud = ctx("ffi-recover");
    let mut a = Mac::new("ffi-recover-a");
    a.setup(&cloud.cloud, "synthetic-ffi-recover@example.test").unwrap();
    a.add("before-the-loss");
    a.publish(&cloud.cloud).unwrap();
    let dir = tmp("ffi-recover-phone");
    let e = open_raw(&cloud, &dir);
    assert_eq!(call(e, r#"{"op":"provider_recover_finish"}"#)["error"], "BAD_STATE", "nothing to finish yet");
    let start = call(e, r#"{"op":"provider_recover_start","handle":"synthetic-ffi-recover@example.test","kind":"mp"}"#);
    assert_eq!(start["ok"], true, "{start}");
    let preview = call(e, r#"{"op":"recovery_preview"}"#);
    assert_eq!(preview["ok"], true, "{preview}");
    let done = call(e, r#"{"op":"provider_recover_finish"}"#);
    assert_eq!(done["ok"], true, "{done}");
    assert_eq!(call(e, r#"{"op":"get_state"}"#)["state"], "unlocked");
    assert!(call(e, r#"{"op":"list_items"}"#)["items"].to_string().contains("before-the-loss"));
    let devices = call(e, r#"{"op":"list_devices"}"#);
    assert!(devices["devices"].as_array().unwrap().iter().any(|d| d["self"] == true && d["platform"] != json_mac()), "{devices}");
    assert_eq!(a.sync(&cloud.cloud).err(), Some(vault_helper::errors::ErrorCode::AuthInvalid), "the lost Mac is cut off");
    unsafe { ov0_engine_close(e) };
}

fn json_mac() -> Value {
    serde_json::json!(vault_helper::registry::device::PLATFORM_MACOS)
}


/// §22.10 / §22.11 (F.2d chunk 3): a revocation whose upload failed is
/// kept fully staged; after a lock it finishes at the next foreground with
/// the vault still LOCKED, and the Mac is cut off.
#[test]
fn a_cut_short_revocation_finishes_while_locked() {
    let _g = serial();
    use vault_helper::registry::device::DeviceIdentity;
    let (cloud, mut a, b) = pair("ffi-resume");
    let e = open(&cloud, &b.dir);
    assert_eq!(call(e, r#"{"op":"provider_sync"}"#)["ok"], true);
    let mac = vault_helper::crypto::hex::encode(a.dev.device_id());
    assert_eq!(call(e, &format!(r#"{{"op":"revoke_device","device_id":"{mac}"}}"#))["ok"], true);
    cloud.offline.set(true);
    assert_eq!(call(e, r#"{"op":"provider_publish"}"#)["error"], "BACKUP_UNAVAILABLE");
    unsafe { ov0_engine_lock(e) };
    let state = call(e, r#"{"op":"get_state"}"#);
    // LOCKED with a staged publication reports BACKING_UP (§13), key absent.
    assert_eq!((state["state"].as_str(), state["vault_open"].as_bool(), state["upload_waiting"].as_bool()), (Some("backing_up"), Some(false), Some(true)), "{state}");
    cloud.offline.set(false);
    let p = call(e, r#"{"op":"provider_publish"}"#);
    assert_eq!(p["result"]["committed"], true, "{p}");
    assert_eq!(call(e, r#"{"op":"get_state"}"#)["state"], "locked", "no key was needed");
    assert_eq!(a.sync(&cloud.cloud).err(), Some(vault_helper::errors::ErrorCode::AuthInvalid), "the Mac is cut off");
    assert_eq!(call(e, r#"{"op":"provider_sync"}"#)["error"], "BAD_STATE", "nothing else runs while locked");
    unsafe { ov0_engine_close(e) };
}
