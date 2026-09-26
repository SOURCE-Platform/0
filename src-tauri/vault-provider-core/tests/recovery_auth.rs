//! MP-only recovery throttle (RL-01…RL-04), locate with fake responses
//! (BK-22), recovery-auth rotation (BK-16) and retention GC (BK-08,
//! BK-21) at the provider. Synthetic data only.

mod pfx;

use pfx::*;
use serde_json::{json, Value};
use vault_proto::crypto::hex;
use vault_proto::crypto::recovery_auth::{self, RecoveryClass};
use vault_proto::crypto::secret::random_secret;
use vault_proto::header::{Hex16, KdfBlock};
use vault_proto::request::Operation;
use vault_proto::state::TransitionKind;
use vault_provider_core::gc::MIN_AGE;
use vault_provider_core::stores::BlobStore;
use vault_provider_core::Request;

fn wrong_mp_key(s: &Sim) -> recovery_auth::RecoveryAuthKey {
    let h = &s.cur.as_ref().unwrap().header;
    recovery_auth::derive(RecoveryClass::Mp, &random_secret(), &h.auth_salt_mp.0, &s.vault_id).unwrap()
}

/// RL-01 / RL-02 / RL-04: failed MP-class verifications fill at most L
/// slots per window across instances; the next MP request is 429 without
/// verification; successes consume nothing; RK is never throttled.
#[test]
fn rl_mp_throttle() {
    let s = Sim::created("rl");
    let second = provider_on(s.fs.clone());
    let h = s.cur.as_ref().unwrap().header.clone();
    let good_mp = s.auth_key(RecoveryClass::Mp, &h);
    for _ in 0..5 {
        assert_eq!(s.call(Operation::StateGet, None, b"", Who::Rec(&good_mp, RecoveryClass::Mp), None).status, 200, "RL-02");
    }
    let bad = wrong_mp_key(&s);
    let mut failures = 0;
    for i in 0..20 {
        let mut sim_view = Sim::on(s.fs.clone(), s.dir.join("unused"), "synthetic-unused@example.test");
        sim_view.vault_id = s.vault_id;
        sim_view.p = if i % 2 == 0 { s.p.clone() } else { second.clone() };
        let r = sim_view.call(Operation::StateGet, None, b"", Who::Rec(&bad, RecoveryClass::Mp), None);
        std::mem::forget(sim_view); // shares the store directory
        match r.status {
            401 => failures += 1,
            429 => assert_eq!(Sim::error(&r), "RECOVERY_THROTTLED"),
            other => panic!("unexpected {other}"),
        }
    }
    assert_eq!(failures, 10, "≤ L failures per window across both instances");
    let r = s.call(Operation::StateGet, None, b"", Who::Rec(&good_mp, RecoveryClass::Mp), None);
    assert_eq!(r.status, 429, "a full window refuses before verification");
    let rk = s.auth_key(RecoveryClass::Rk, &h);
    assert_eq!(s.call(Operation::StateGet, None, b"", Who::Rec(&rk, RecoveryClass::Rk), None).status, 200, "RL-04");
    let mut later = Sim::on(s.fs.clone(), s.dir.join("unused2"), "synthetic-unused2@example.test");
    later.vault_id = s.vault_id;
    later.now = T0 + 3600;
    assert_eq!(later.call(Operation::StateGet, None, b"", Who::Rec(&good_mp, RecoveryClass::Mp), None).status, 200, "next window");
    std::mem::forget(later);
}

fn locate(s: &Sim, hk: &[u8; 32]) -> Value {
    let body = json!({ "handle_key": hex::encode(hk) }).to_string();
    let r = s.p.handle(&Request { method: "POST", path: "/v2/recover/locate", auth: None, body: body.as_bytes(), now: s.now, client_ip: "198.51.100.7" });
    assert_eq!(r.status, 200);
    serde_json::from_slice(&r.body).unwrap()
}

/// BK-22: a known handle returns the committed header's public fields;
/// an unknown one returns the same shape, deterministic, allowlisted KDF;
/// its next recovery step fails with the same generic 401.
#[test]
fn bk22_locate_real_and_fake() {
    let s = Sim::created("bk22");
    let h = s.cur.as_ref().unwrap().header.clone();
    let real = locate(&s, &s.handle_key);
    assert_eq!(real["vault_id"], hex::encode(s.vault_id));
    assert_eq!(real["kdf"], serde_json::to_value(&h.kdf).unwrap());
    assert_eq!(real["auth_salt_mp"], json!(Hex16(h.auth_salt_mp.0)));
    let unknown = [0x42u8; 32];
    let fake = locate(&s, &unknown);
    let keys = |v: &Value| v.as_object().unwrap().keys().cloned().collect::<Vec<_>>();
    assert_eq!(keys(&fake), keys(&real));
    assert_eq!(fake, locate(&s, &unknown), "deterministic");
    let kdf: KdfBlock = serde_json::from_value(fake["kdf"].clone()).unwrap();
    assert!(kdf.is_allowlisted());
    let mut probe = Sim::on(s.fs.clone(), s.dir.join("probe"), "synthetic-probe@example.test");
    probe.vault_id = hex::decode_array(fake["vault_id"].as_str().unwrap()).unwrap();
    let salt: [u8; 16] = hex::decode_array(fake["auth_salt_rk"].as_str().unwrap()).unwrap();
    let k = recovery_auth::derive(RecoveryClass::Rk, &random_secret(), &salt, &probe.vault_id).unwrap();
    let r = probe.call(Operation::StateGet, None, b"", Who::Rec(&k, RecoveryClass::Rk), None);
    assert_eq!((r.status, Sim::error(&r)), (401, "AUTH_INVALID".into()));
    std::mem::forget(probe);
}

/// BK-16: an RK replacement carries the rk update; before commit the old
/// key works, after it only the new one.
#[test]
fn bk16_rk_replacement() {
    let mut s = Sim::created("bk16");
    let old = s.auth_key(RecoveryClass::Rk, &s.cur.as_ref().unwrap().header);
    let mut d = s.next_draft();
    d.header.auth_salt_rk = Hex16::random();
    s.rk = random_secret();
    d.updates = vec![s.update(RecoveryClass::Rk, &d.header)];
    d.wrap_rk = Some(rk_wrap("new-rk"));
    let new = s.auth_key(RecoveryClass::Rk, &d.header);
    assert_eq!(s.call(Operation::StateGet, None, b"", Who::Rec(&old, RecoveryClass::Rk), None).status, 200);
    assert_eq!(s.call(Operation::StateGet, None, b"", Who::Rec(&new, RecoveryClass::Rk), None).status, 401);
    assert_eq!(s.publish(d).status, 200);
    assert_eq!(s.call(Operation::StateGet, None, b"", Who::Rec(&old, RecoveryClass::Rk), None).status, 401);
    assert_eq!(s.call(Operation::StateGet, None, b"", Who::Rec(&new, RecoveryClass::Rk), None).status, 200);
    // A salt change without its update is stale.
    let mut d = s.next_draft();
    d.header.auth_salt_mp = Hex16::random();
    assert_eq!(Sim::error(&s.publish(d)), "RECOVERY_AUTH_STALE");
}

/// BK-08 / BK-21: GC keeps everything reachable from the current and two
/// retained states, keeps young orphans, deletes old orphans; a commit
/// referencing a collected blob gets BLOB_MISSING and succeeds after a
/// re-upload.
#[test]
fn bk08_bk21_gc() {
    let mut s = Sim::created("gc");
    let orphan = b"synthetic orphan from an interrupted upload".to_vec();
    let sha: [u8; 32] = sha2::Digest::finalize(<sha2::Sha256 as sha2::Digest>::new_with_prefix(&orphan)).into();
    assert_eq!(s.call(Operation::BlobPut, Some(&sha), &orphan, Who::Dev(&s.mac), None).status, 200);
    for i in 0..4 {
        let mut d = s.next_draft();
        s.add_record(&mut d, i + 1);
        assert_eq!(s.publish(d).status, 200);
    }
    // FsStores stamps blobs with real file times, so GC runs on the wall
    // clock (the request clock `s.now` is a fixed synthetic instant).
    let wall = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
    assert_eq!(s.p.gc_vault(&s.vault_id, wall).unwrap(), 0, "young blobs are kept");
    let far = wall + MIN_AGE + 60;
    let before: Vec<_> = s.fs.list(&s.vault_id).unwrap();
    let deleted = s.p.gc_vault(&s.vault_id, far).unwrap();
    assert!(deleted >= 1);
    assert!(s.fs.get(&s.vault_id, &sha).unwrap().is_none(), "orphan collected");
    assert!(s.fs.list(&s.vault_id).unwrap().len() < before.len());
    let g = s.call(Operation::StateGet, None, b"", Who::Dev(&s.mac), None);
    assert_eq!(g.status, 200, "current state intact after GC");
    // A publish whose record blob was never uploaded (or was collected).
    let mut d = s.next_draft();
    s.add_record(&mut d, 9);
    let (t, blobs) = s.build(&d, TransitionKind::Publish, &s.mac);
    let r = s.commit(&t, Who::Dev(&s.mac));
    assert_eq!((r.status, Sim::error(&r)), (412, "BLOB_MISSING".into()));
    let v: Value = serde_json::from_slice(&r.body).unwrap();
    assert!(v["count"].as_u64().unwrap() >= 1);
    s.put_blobs(&blobs, &s.mac);
    let r = s.commit(&t, Who::Dev(&s.mac));
    assert_eq!(r.status, 200);
}
