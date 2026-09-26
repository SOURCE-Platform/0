//! §11.3 state transitions at the provider (BK-09/12/13/17/19/20/23/24,
//! PR-04 provider side). Synthetic data only.

mod pfx;

use pfx::*;
use vault_proto::crypto::recovery_auth::RecoveryClass;
use vault_proto::registry::build;
use vault_proto::registry::chain::{verify_chain_with, EpochPolicy};
use vault_proto::registry::device::{DeviceIdentity, SoftwareDevice, PLATFORM_MACOS};
use vault_proto::request::Operation;
use vault_proto::state::TransitionKind;

#[test]
fn create_then_publish_then_read() {
    let mut s = Sim::created("basic");
    assert_eq!(s.generation, 1);
    let mut d = s.next_draft();
    s.add_record(&mut d, 1);
    let r = s.publish(d);
    assert_eq!(r.status, 200);
    assert_eq!(s.generation, 2);
    let g = s.call(Operation::StateGet, None, b"", Who::Dev(&s.mac), None);
    assert_eq!(g.status, 200);
    let v: serde_json::Value = serde_json::from_slice(&g.body).unwrap();
    assert_eq!(v["generation"], 2);
    assert_eq!(v["recovery_auth"].as_array().unwrap().len(), 2);
}

/// BK-09/17: the loser of a CAS race gets STATE_MOVED with the new commit.
#[test]
fn bk09_stale_expected_state_moves() {
    let mut s = Sim::created("bk09");
    let stale = s.state_commit;
    let d = s.next_draft();
    assert_eq!(s.publish(d).status, 200);
    let mut d2 = s.next_draft();
    s.add_record(&mut d2, 1);
    let (mut t, blobs) = s.build(&d2, TransitionKind::Publish, &s.mac);
    s.put_blobs(&blobs, &s.mac);
    t.expected_state = stale;
    let r = s.commit(&t, Who::Dev(&s.mac));
    assert_eq!((r.status, Sim::error(&r)), (409, "STATE_MOVED".into()));
}

/// BK-12: a verbatim replay of a mutating request (same nonce) is refused;
/// BK-20: the same body under a fresh nonce returns the stored result.
#[test]
fn bk12_bk20_replay_and_idempotency() {
    let mut s = Sim::created("bk12");
    let d = s.next_draft();
    let (t, blobs) = s.build(&d, TransitionKind::Publish, &s.mac);
    s.put_blobs(&blobs, &s.mac);
    let body = t.encode().unwrap();
    let n = [9u8; 16];
    let r1 = s.call_n(Operation::StateCommit, None, &body, Who::Dev(&s.mac), Some(t.expected_state), n);
    assert_eq!(r1.status, 200);
    let r2 = s.call_n(Operation::StateCommit, None, &body, Who::Dev(&s.mac), Some(t.expected_state), n);
    assert_eq!(Sim::error(&r2), "BACKUP_REPLAY");
    let r3 = s.commit(&t, Who::Dev(&s.mac));
    assert_eq!((r3.status, &r3.body), (200, &r1.body), "lost-response retry is idempotent");
    s.accept(&r1, d, &t);
    assert_eq!(s.generation, 2);
}

/// BK-13 / EV-04: after a revoking publish commits, the revoked key gets
/// the generic 401 for reads and writes.
#[test]
fn bk13_revoked_device_is_401() {
    let mut s = Sim::created("bk13");
    let other = SoftwareDevice::generate("Synthetic Mac B", PLATFORM_MACOS);
    let mut d = s.next_draft();
    let st = verify_chain_with(&d.registry, &s.vault_id, &EpochPolicy::CheckpointAnchored).unwrap();
    d.registry.push(build::enroll(&st, &s.mac, &other).unwrap());
    d.envs.push((other.device_id(), env_blob(&other.device_id(), 1)));
    assert_eq!(s.publish(d).status, 200);
    assert_eq!(s.call(Operation::StateGet, None, b"", Who::Dev(&other), None).status, 200);
    // Revoke: registry revoke + vk+1 + both classes re-keyed + no env.
    let mut d = s.next_draft();
    let st = verify_chain_with(&d.registry, &s.vault_id, &EpochPolicy::CheckpointAnchored).unwrap();
    d.registry.push(build::revoke(&st, &s.mac, other.device_id()).unwrap());
    rotate_and_rekey(&s, &mut d);
    d.envs.retain(|(id, _)| *id != other.device_id());
    assert_eq!(s.publish(d).status, 200);
    let r = s.call(Operation::StateGet, None, b"", Who::Dev(&other), None);
    assert_eq!((r.status, Sim::error(&r)), (401, "AUTH_INVALID".into()));
}

/// Rotation plus both recovery classes re-keyed, as a revocation requires.
pub fn rotate_and_rekey(s: &Sim, d: &mut Draft) {
    d.header.vk_generation += 1;
    d.header.kdf = vault_proto::header::KdfBlock::fresh();
    d.header.auth_salt_mp = vault_proto::header::Hex16::random();
    d.header.auth_salt_rk = vault_proto::header::Hex16::random();
    d.wrap_mp = mp_wrap(&d.header, "rot");
    d.wrap_rk = Some(rk_wrap("rot"));
    let g = d.header.vk_generation;
    for (id, env) in d.envs.iter_mut() {
        *env = env_blob(id, g);
    }
    for r in d.revs.iter_mut() {
        r.vk_generation = g;
    }
    d.vk = vault_proto::crypto::secret::random_secret();
    d.updates = vec![s.update(RecoveryClass::Mp, &d.header), s.update(RecoveryClass::Rk, &d.header)];
}

/// BK-19 / BK-24: a revoking publish without the vk bump, with the
/// target's env, or missing a recovery-class update is refused.
#[test]
fn bk19_bk24_revocation_shape() {
    let mut s = Sim::created("bk19");
    let other = SoftwareDevice::generate("Synthetic Mac B", PLATFORM_MACOS);
    let mut d = s.next_draft();
    let st = verify_chain_with(&d.registry, &s.vault_id, &EpochPolicy::CheckpointAnchored).unwrap();
    d.registry.push(build::enroll(&st, &s.mac, &other).unwrap());
    d.envs.push((other.device_id(), env_blob(&other.device_id(), 1)));
    assert_eq!(s.publish(d).status, 200);
    let base = |s: &Sim| {
        let mut d = s.next_draft();
        let st = verify_chain_with(&d.registry, &s.vault_id, &EpochPolicy::CheckpointAnchored).unwrap();
        d.registry.push(build::revoke(&st, &s.mac, other.device_id()).unwrap());
        rotate_and_rekey(s, &mut d);
        d
    };
    let mut d = base(&s);
    d.envs.retain(|(id, _)| *id != other.device_id());
    d.header.vk_generation -= 1;
    for r in d.revs.iter_mut() {
        r.vk_generation -= 1;
    }
    assert_eq!(Sim::error(&s.publish(d)), "MANIFEST_INVALID", "no vk bump");
    let d = base(&s);
    assert_eq!(Sim::error(&s.publish(d)), "INDEX_INVALID", "target env kept");
    let mut d = base(&s);
    d.envs.retain(|(id, _)| *id != other.device_id());
    d.updates.retain(|u| u.class == RecoveryClass::Mp);
    d.header.auth_salt_rk = s.cur.as_ref().unwrap().header.auth_salt_rk;
    assert_eq!(Sim::error(&s.publish(d)), "RECOVERY_AUTH_STALE", "rk class not re-keyed");
    let mut d = base(&s);
    d.envs.retain(|(id, _)| *id != other.device_id());
    d.wrap_mp = mp_wrap(&s.cur.as_ref().unwrap().header, "old-kdf");
    assert_eq!(Sim::error(&s.publish(d)), "RECOVERY_AUTH_STALE", "wrap kdf ≠ header");
    // A plain rotation re-sealing password.wrap under the same PK (same
    // kdf block) with no update is accepted.
    let mut d = s.next_draft();
    d.header.vk_generation += 1;
    d.wrap_mp = mp_wrap(&d.header, "same-pk");
    let g = d.header.vk_generation;
    for (id, env) in d.envs.iter_mut() {
        *env = env_blob(id, g);
    }
    d.vk = vault_proto::crypto::secret::random_secret();
    assert_eq!(s.publish(d).status, 200);
}

/// BK-23: no blob_put into a vault without state; a create whose inline
/// blob set ≠ the index's references writes nothing.
#[test]
fn bk23_bootstrap_rules() {
    let s = Sim::new("bk23");
    let r = s.call(Operation::BlobPut, Some(&[0u8; 32]), b"x", Who::Dev(&s.mac), None);
    assert_eq!(r.status, 401);
    let d = s.genesis_draft();
    let (mut t, _) = s.build(&d, TransitionKind::Create, &s.mac);
    t.bootstrap_blobs.push(b"an extra unreferenced blob".to_vec());
    let r = s.commit(&t, Who::Dev(&s.mac));
    assert_eq!(Sim::error(&r), "INDEX_INVALID");
    assert!(!s.dir.join("v2/vaults").exists() || std::fs::read_dir(s.dir.join("v2/vaults")).unwrap().next().is_none());
    assert!(!s.dir.join("v2/handles").exists());
}

/// PR-04 (provider side): anything bound by the signature and changed
/// afterwards is refused with the generic 401.
#[test]
fn pr04_bound_fields() {
    let s = Sim::created("pr04");
    let ok = s.call(Operation::StateGet, None, b"", Who::Dev(&s.mac), None);
    assert_eq!(ok.status, 200);
    let mut n = [0u8; 16];
    getrandom::fill(&mut n).unwrap();
    use vault_proto::crypto::recovery_auth::key_id;
    use vault_proto::request::{auth_header, ProviderRequest, SignerId};
    let signer = SignerId::Device { device_id: s.mac.device_id(), key_id: key_id(&s.mac.sign_pub()) };
    let req = ProviderRequest::build(ORIGIN, s.vault_id, Operation::StateGet, None, signer, b"", None, s.now, n).unwrap();
    let h = auth_header(&req.encode(), &s.mac.sign_prehash(&req.prehash()).unwrap());
    let send = |method: &str, path: &str, body: &[u8], now: u64| {
        s.p.handle(&vault_provider_core::Request { method, path, auth: Some(&h), body, now, client_ip: "192.0.2.1" }).status
    };
    let other_vid = format!("/v2/vaults/{}/state", "00".repeat(16));
    assert_eq!(send("GET", &req.path, b"extra", s.now), 401, "body");
    assert_eq!(send("POST", &req.path, b"", s.now), 401, "method");
    assert_eq!(send("GET", &other_vid, b"", s.now), 401, "vault/path");
    assert_eq!(send("GET", &req.path, b"", s.now + 301), 401, "clock");
    assert_eq!(send("GET", &req.path, b"", s.now), 200);
}

/// BK-17 / BK-12 (second instance): two publishers race for one
/// generation on two provider instances sharing the store — exactly one
/// commits, the other gets STATE_MOVED; a nonce spent on one instance is
/// refused by the other.
#[test]
fn bk17_concurrent_publishers() {
    let mut s = Sim::created("bk17");
    let other = SoftwareDevice::generate("Synthetic Mac B", PLATFORM_MACOS);
    let mut d = s.next_draft();
    let st = verify_chain_with(&d.registry, &s.vault_id, &EpochPolicy::CheckpointAnchored).unwrap();
    d.registry.push(build::enroll(&st, &s.mac, &other).unwrap());
    d.envs.push((other.device_id(), env_blob(&other.device_id(), 1)));
    assert_eq!(s.publish(d).status, 200);
    let second = provider_on(s.fs.clone());
    let mut da = s.next_draft();
    s.add_record(&mut da, 1);
    let mut db = s.next_draft();
    db.revs.push(row(&vault_proto::rev::new_record_id(), &other, 1, vec![], 1));
    let (ta, ba) = s.build(&da, TransitionKind::Publish, &s.mac);
    let (tb, bb) = s.build(&db, TransitionKind::Publish, &other);
    s.put_blobs(&ba, &s.mac);
    s.put_blobs(&bb, &other);
    let first = s.p.clone();
    let (ra, rb) = std::thread::scope(|sc| {
        let a = sc.spawn(|| s.commit(&ta, Who::Dev(&s.mac)));
        let b = sc.spawn(|| {
            let body = tb.encode().unwrap();
            let mut n = [0u8; 16];
            getrandom::fill(&mut n).unwrap();
            let key = vault_proto::crypto::recovery_auth::key_id(&other.sign_pub());
            let req = vault_proto::request::ProviderRequest::build(
                ORIGIN,
                s.vault_id,
                Operation::StateCommit,
                None,
                vault_proto::request::SignerId::Device { device_id: other.device_id(), key_id: key },
                &body,
                Some(tb.expected_state),
                s.now,
                n,
            )
            .unwrap();
            let h = vault_proto::request::auth_header(&req.encode(), &other.sign_prehash(&req.prehash()).unwrap());
            let r = second.handle(&vault_provider_core::Request { method: "POST", path: &req.path, auth: Some(&h), body: &body, now: s.now, client_ip: "192.0.2.2" });
            let replay = first.handle(&vault_provider_core::Request { method: "POST", path: &req.path, auth: Some(&h), body: &body, now: s.now, client_ip: "192.0.2.2" });
            (r, replay)
        });
        (a.join().unwrap(), b.join().unwrap())
    });
    let (rb, replay) = rb;
    let mut codes = vec![(ra.status, Sim::error(&ra)), (rb.status, Sim::error(&rb))];
    codes.sort();
    assert_eq!(codes, vec![(200, String::new()), (409, "STATE_MOVED".into())]);
    assert_eq!(Sim::error(&replay), "BACKUP_REPLAY", "nonce spent on the other instance");
}
