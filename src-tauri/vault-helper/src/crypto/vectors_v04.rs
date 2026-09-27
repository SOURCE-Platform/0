//! Spec v0.4 cross-language vector families (§16.8): XV-OBJ,
//! XV-RECORD-AAD, XV-INDEX, XV-STATE, XV-REQSIG, XV-RECOVERY-AUTH and
//! XV-HANDLE. All inputs are fixed synthetic constants; the Swift side
//! and the PoC `hpke` cross-check recompute them independently.

use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use vault_proto::backup::index::{IndexEntry, ObjectIndex, Role};
use vault_proto::handle;
use vault_proto::request::{Operation, ProviderRequest, SignerId};
use vault_proto::state::{recovery_auth_digest, state_commit, RecoveryAuthEntry, StateTransition, TransitionKind};

use super::ecdsa;
use super::hex;
use super::record::{meta_aad, record_aad};
use super::recovery_auth::{self, key_id, RecoveryClass};
use super::secret::SecretBytes;
use crate::backup::object;
use crate::storage::revisions::{graph_digest, RevBinding, RevisionRow};

const VID: [u8; 16] = [0xA0; 16];
const RID: &str = "5e7e0000-0000-4000-8000-000000000001";
const AUTHOR: &str = "a0000000-0000-4000-8000-00000000000a";
const REV: [u8; 32] = [0x44; 32];
const PARENTS: [[u8; 32]; 2] = [[0x10; 32], [0x20; 32]];
const DEVICE_SCALAR: [u8; 32] = [0x11; 32];

fn hx(b: impl AsRef<[u8]>) -> Value {
    Value::String(hex::encode(b))
}

fn uuid(s: &str) -> [u8; 16] {
    crate::storage::revisions::uuid_bytes(s).expect("fixed uuid")
}

fn row() -> RevisionRow {
    RevisionRow {
        revision_id: REV,
        record_id: RID.into(),
        parent_ids: PARENTS.to_vec(),
        author_device: AUTHOR.into(),
        counter: 7,
        deleted: false,
        kind_tag: 1,
        vk_generation: 3,
        schema_version: 1,
        nonce: [0x01; 24],
        ct: b"synthetic-ciphertext".to_vec(),
        meta_nonce: [0x02; 24],
        meta_ct: b"synthetic-meta".to_vec(),
        created_at: 1_900_000_000,
        updated_at: 1_900_000_001,
    }
}

/// XV-OBJ: canonical bytes, blob hash, and parse rejections by offset.
pub fn xv_obj() -> Value {
    let bytes = object::encode(&row()).expect("fixed row encodes");
    let reject = |name: &str, f: &dyn Fn(&mut Vec<u8>), code: &str| {
        let mut b = bytes.clone();
        f(&mut b);
        json!({ "name": name, "object": hx(&b), "error": code })
    };
    json!({
        "family": "XV-OBJ",
        "object": hx(&bytes),
        "blob_hash": hx(object::blob_hash(&bytes)),
        "rejections": [
            reject("retired magic", &|b| b[..8].copy_from_slice(b"OV0OBJ01"), "FORMAT_INVALID"),
            reject("unknown magic", &|b| b[..8].copy_from_slice(b"OV0OBJ03"), "FORMAT_TOO_NEW"),
            reject("reserved flag bit", &|b| b[9] = 2, "FORMAT_INVALID"),
            reject("zero author", &|b| b[22..38].fill(0), "FORMAT_INVALID"),
            reject("nine parents", &|b| b[86] = 9, "FORMAT_INVALID"),
            reject("unsorted parents", &|b| b[87..119].fill(0x30), "FORMAT_INVALID"),
            reject("trailing byte", &|b| b.push(0), "FORMAT_INVALID"),
        ],
    })
}

/// XV-RECORD-AAD: graph_digest and the v2 record and meta AAD.
pub fn xv_record_aad() -> Value {
    let r = row();
    let digest = graph_digest(&uuid(RID), &REV, &uuid(AUTHOR), r.counter, r.deleted, r.kind_tag, &PARENTS);
    let bind = RevBinding { revision_id: REV, graph_digest: digest };
    json!({
        "family": "XV-RECORD-AAD",
        "vault_id": hx(VID), "record_id": hx(uuid(RID)), "revision_id": hx(REV), "author": hx(uuid(AUTHOR)),
        "counter": r.counter, "deleted": r.deleted, "kind": r.kind_tag, "parents": [hx(PARENTS[0]), hx(PARENTS[1])],
        "graph_digest": hx(digest),
        "schema_version": r.schema_version, "vk_generation": r.vk_generation,
        "record_aad": hx(record_aad(&VID, &uuid(RID), &bind, r.schema_version, r.vk_generation)),
        "meta_field_tag": "list",
        "meta_aad": hx(meta_aad(&VID, &uuid(RID), &bind, b"list")),
    })
}

/// XV-INDEX: canonical index v2 text and its hash.
pub fn xv_index() -> Value {
    let blob = |b: &[u8]| IndexEntry::of(Role::Header, b).blob;
    let e = |role: Role, bytes: &[u8]| IndexEntry { role, blob: blob(bytes), size: bytes.len() as u64 };
    let idx = ObjectIndex {
        generation: 5,
        item_count: 1,
        entries: vec![
            e(Role::Header, b"synthetic-header"),
            e(Role::Registry, b"synthetic-registry"),
            e(Role::WrapMp, b"synthetic-wrap-mp"),
            e(Role::WrapRk, b"synthetic-wrap-rk"),
            e(Role::Env { device_id: uuid(AUTHOR) }, b"synthetic-envelope"),
            e(Role::Rev { record_id: uuid(RID), revision_id: PARENTS[0], parents: vec![] }, b"synthetic-rev-root"),
            e(Role::Rev { record_id: uuid(RID), revision_id: REV, parents: vec![PARENTS[0]] }, b"synthetic-rev-child"),
        ],
    };
    let text = idx.encode();
    json!({ "family": "XV-INDEX", "text": String::from_utf8(text.clone()).expect("ascii"), "object_index_hash": hx(Sha256::digest(&text)) })
}

fn auth_entries() -> Vec<RecoveryAuthEntry> {
    let (_, mp_pub) = ecdsa::dev_keypair_from_scalar([0x21; 32]);
    let (_, rk_pub) = ecdsa::dev_keypair_from_scalar([0x22; 32]);
    vec![
        RecoveryAuthEntry { class: RecoveryClass::Mp, public: mp_pub, salt: [0x31; 16] },
        RecoveryAuthEntry { class: RecoveryClass::Rk, public: rk_pub, salt: [0x32; 16] },
    ]
}

/// XV-STATE: recovery_auth_digest, state_commit and a `publish` body.
pub fn xv_state() -> Value {
    let auth = auth_entries();
    let digest = recovery_auth_digest(&auth).expect("canonical");
    let commit = state_commit(&VID, 9, &[0x55; 32], &[0x66; 32], &digest);
    let body = StateTransition {
        vault_id: VID,
        kind: TransitionKind::Publish,
        expected_state: commit,
        manifest: b"synthetic-manifest".to_vec(),
        checkpoint: b"synthetic-checkpoint".to_vec(),
        recovery_auth_updates: vec![auth[1]],
        handle_key: None,
        bootstrap_blobs: vec![],
    }
    .encode()
    .expect("publish body");
    json!({
        "family": "XV-STATE",
        "recovery_auth": auth.iter().map(|a| json!({"class": a.class.code(), "pub": hx(a.public), "salt": hx(a.salt)})).collect::<Vec<_>>(),
        "recovery_auth_digest": hx(digest),
        "state_commit_inputs": {"vault_id": hx(VID), "generation": 9, "manifest_hash": hx([0x55; 32]), "checkpoint_hash": hx([0x66; 32])},
        "state_commit": hx(commit),
        "publish_body": hx(&body),
        "publish_body_sha256": hx(Sha256::digest(&body)),
    })
}

/// XV-REQSIG: canonical requests, prehashes, and signatures. The
/// recovery-class signature is byte-exact (RFC 6979); a device-class
/// signature is checked by verification only (the Enclave randomizes).
pub fn xv_reqsig() -> Value {
    let (dev_sk, dev_pub) = ecdsa::dev_keypair_from_scalar(DEVICE_SCALAR);
    let rk = recovery_auth::derive(RecoveryClass::Rk, &SecretBytes::new([0x52; 32]), &[0x53; 16], &VID).expect("derive");
    let n = [0x0D; 16];
    let get = ProviderRequest::build(
        "https://provider.test",
        VID,
        Operation::StateGet,
        None,
        SignerId::Device { device_id: uuid(AUTHOR), key_id: key_id(&dev_pub) },
        vault_proto::request::body_hash(b""),
        None,
        1_900_000_000,
        n,
    )
    .expect("state_get");
    let commit = ProviderRequest::build(
        "https://provider.test",
        VID,
        Operation::StateCommit,
        None,
        SignerId::Recovery { class: RecoveryClass::Rk.code(), key_id: rk.key_id() },
        [0x77; 32],
        Some([0x78; 32]),
        1_900_000_000,
        n,
    )
    .expect("state_commit");
    json!({
        "family": "XV-REQSIG",
        "device": { "sign_pub": hx(dev_pub), "tlv": hx(get.encode()), "prehash": hx(get.prehash()),
                    "signature_verify_only": hx(ecdsa::dev_sign_prehash(&dev_sk, &get.prehash())) },
        "recovery": { "class": "rk", "pub": hx(rk.public), "tlv": hx(commit.encode()), "prehash": hx(commit.prehash()),
                      "signature": hx(rk.sign_prehash(&commit.prehash())) },
    })
}

/// XV-RECOVERY-AUTH: the §11.4 composition for both classes, RFC 9180
/// A.3 DeriveKeyPair conformance, and RFC 6979 A.2.5 signing conformance.
pub fn xv_recovery_auth() -> Value {
    let rows: Vec<Value> = [(RecoveryClass::Mp, [0x51u8; 32], [0x61u8; 16]), (RecoveryClass::Rk, [0x52; 32], [0x62; 16])]
        .iter()
        .map(|(class, secret, salt)| {
            let mut info = match class {
                RecoveryClass::Mp => recovery_auth::DOMAIN_MP.to_vec(),
                RecoveryClass::Rk => recovery_auth::DOMAIN_RK.to_vec(),
            };
            info.extend_from_slice(&VID);
            let ikm = super::hkdf::hkdf32(secret, salt, &info).expect("hkdf");
            let k = recovery_auth::derive(*class, &SecretBytes::new(*secret), salt, &VID).expect("derive");
            let digest: [u8; 32] = Sha256::digest(b"ov0 synthetic request").into();
            json!({ "class": class.code(), "secret": hx(secret), "auth_salt": hx(salt), "vault_id": hx(VID),
                    "ikm": hx(ikm.expose()), "pub": hx(k.public), "key_id": hx(k.key_id()),
                    "prehash": hx(digest), "signature": hx(k.sign_prehash(&digest)) })
        })
        .collect();
    let rfc9180: Vec<Value> = [
        ("4270e54ffd08d79d5928020af4686d8f6b7d35dbe470265f1f5aa22816ce860e", "04a92719c6195d5085104f469a8b9814d5838ff72b60501e2c4466e5e67b325ac98536d7b61a1af4b78e5b7f951c0900be863c403ce65c9bfcb9382657222d18c4"),
        ("668b37171f1072f3cf12ea8a236a45df23fc13b82af3609ad1e354f6ef817550", "04fe8c19ce0905191ebc298a9245792531f26f0cece2460639e8bc39cb7f706a826a779b4cf969b8a0e539c7f62fb3d30ad6aa8f80e30f1d128aafd68a2ce72ea0"),
    ]
    .iter()
    .map(|(ikm, pk)| json!({ "ikm": ikm, "pk": pk }))
    .collect();
    // RFC 6979 A.2.5 (P-256, SHA-256): private key x, messages "sample"
    // and "test" (values as published; also in the p256 crate's tests).
    let rfc6979 = json!({
        "x": "c9afa9d845ba75166b5c215767b1d6934e50c3db36e89b127b8a622b120f6721",
        "sample": "efd48b2aacb6a8fd1140dd9cd45e81d69d2c877b56aaf991c34d0ea84eaf3716f7cb1c942d657c41d436c7a1b6e29f65f3e900dbb9aff4064dc4ab2f843acda8",
        "test": "f1abb023518351cd71d881567b1ea663ed3efcf6c5132b354f28d3b0b7d38367019f4113742a2b14bd25926b49c649155f267e60d3814b4c0cc84250e46f0083",
    });
    json!({ "family": "XV-RECOVERY-AUTH", "composition": rows, "rfc9180_a3": rfc9180, "rfc6979_a2_5": rfc6979 })
}

/// XV-HANDLE: normalization and rejection cases, handle_key.
pub fn xv_handle() -> Value {
    let inputs = ["  Alice@Example.TEST ", "ＡＢＣ", "Ünïcode.Handle", "ab", "a b c", "abc\u{200B}", "ab\u{7}c"];
    let cases: Vec<Value> = inputs
        .iter()
        .map(|s| match handle::normalize(s) {
            Ok(n) => json!({ "input": s, "normalized": n, "handle_key": hx(handle::handle_key(&n)) }),
            Err(_) => json!({ "input": s, "rejected": true }),
        })
        .collect();
    json!({ "family": "XV-HANDLE", "cases": cases })
}

pub fn all() -> Vec<(&'static str, Value)> {
    vec![
        ("xv_obj", xv_obj()),
        ("xv_record_aad", xv_record_aad()),
        ("xv_index", xv_index()),
        ("xv_state", xv_state()),
        ("xv_reqsig", xv_reqsig()),
        ("xv_recovery_auth", xv_recovery_auth()),
        ("xv_handle", xv_handle()),
    ]
}
