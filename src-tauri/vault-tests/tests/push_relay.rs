//! Phase G push relay (spec §7.2; design `phase-g-design.md`) against the
//! provider core, with a recording stand-in for APNs: the phone registers
//! its token through the C ABI; a Mac of the vault wakes it; the alert is
//! the fixed generic payload; only devices of the vault, at a bounded rate.
//! Synthetic data only.

mod mfx;
mod phone_fx;

use std::sync::{Arc, Mutex};

use mfx::*;
use phone_fx::*;
use serde_json::{json, Value};
use vault_ffi::*;
use vault_helper::registry::device::DeviceIdentity;
use vault_provider_core::push::PushSender;
use vault_proto::request::Operation;

#[derive(Default)]
struct Recorder(Mutex<Vec<(String, String, u64)>>);

impl PushSender for Recorder {
    fn send(&self, token: &str, payload: &str, expiration: u64) -> Result<(), String> {
        self.0.lock().unwrap().push((token.into(), payload.into(), expiration));
        Ok(())
    }
}

fn wake(cloud: &Cloud, mac: &Mac, to: &[u8; 16], rid: &str) -> u16 {
    let body = json!({ "to": vault_helper::crypto::hex::encode(to), "rid": rid });
    let (pr, auth, bytes) = vault_helper::sync::sign::sign_push(ORIGIN, mac.vid(), &mac.dev, &body, Operation::PushSend, now()).unwrap();
    cloud.send(&pr.method, &pr.path, Some(&auth), &bytes).status
}

#[test]
fn a_mac_wakes_its_phone_with_the_generic_alert_only() {
    let _g = serial();
    let (mut ctx, a, b) = pair("push");
    let rec = Arc::new(Recorder::default());
    let p = vault_provider_core::Provider::with_fs(vault_provider_core::Config::new(ORIGIN, [0x5e; 32]), ctx.cloud.fs.clone()).with_push(rec.clone());
    ctx.cloud.p = Arc::new(p);
    let e = open(&ctx, &b.dir);
    // The phone registers its token (the engine builds and signs the body).
    let reg = call(e, r#"{"op":"push_register_request","token":"a1b2c3d4e5f60718"}"#);
    assert_eq!(reg["ok"], true, "{reg}");
    let r = ctx.cloud.send(reg["method"].as_str().unwrap(), reg["path"].as_str().unwrap(), reg["auth"].as_str(), reg["body"].as_str().unwrap().as_bytes());
    assert_eq!(r.status, 200, "{}", String::from_utf8_lossy(&r.body));
    // The Mac wakes it: the fixed payload, the request id, 120 s.
    let rid = "00112233445566778899aabbccddeeff";
    assert_eq!(wake(&ctx.cloud, &a, &b.dev.device_id(), rid), 200);
    let sent = rec.0.lock().unwrap().clone();
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].0, "a1b2c3d4e5f60718");
    let p: Value = serde_json::from_str(&sent[0].1).unwrap();
    assert_eq!((p["rid"].as_str(), p["aps"]["alert"]["body"].as_str()), (Some(rid), Some("Approval request — open Source to review")));
    assert!(sent[0].2 > now() && sent[0].2 <= now() + 120);
    // Not a device of this vault → 404; past the rate → refused.
    assert_eq!(wake(&ctx.cloud, &a, &[7; 16], rid), 404);
    let statuses: Vec<u16> = (0..12).map(|_| wake(&ctx.cloud, &a, &b.dev.device_id(), rid)).collect();
    assert!(statuses.iter().any(|s| *s != 200), "rate-limited: {statuses:?}");
    // The general signer never signs a push request.
    let none = std::collections::BTreeSet::new();
    let scope = vault_helper::sync::sign::SignScope { put_blobs: &none, staged: None };
    let req = vault_helper::sync::sign::SignRequest { operation: Operation::PushSend, blob: None, body_sha256: [0; 32], expected_state: None };
    assert!(vault_helper::sync::sign::sign(ORIGIN, a.vid(), vault_helper::sync::sign::Key::Device(&a.dev), &req, &scope, 0, now()).is_err());
    unsafe { ov0_engine_close(e) };
}
