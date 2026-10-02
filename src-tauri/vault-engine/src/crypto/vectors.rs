//! Deterministic cross-language vector generation (spec §16.8).
//! `gen_vectors` writes these to `tests/vectors/`; `xv_vectors.rs` tests
//! recompute and compare. Any wire/crypto change must bump versions and
//! regenerate in the same commit (§16.8) — the `--check` mode of
//! gen_vectors enforces freshness in CI.
//!
//! All inputs are fixed synthetic constants: vectors are reproducible and
//! carry no secrets.
//!
//! Phase B families: XV-HKDF, XV-TLV (registry entries, all kinds),
//! XV-BIP39, XV-ECDSA (Rust-produced; Apple-produced vectors land with
//! Phase E), XV-RECOVERY-EPOCH. XV-ECDH/HPKE, XV-HPKE-SE, XV-SAS are
//! Phase E (enrollment); approval payloads and manifests join XV-TLV in
//! Phase E/F respectively.

use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use super::bip39;
use super::ecdsa;
use super::hex;
use super::hkdf;
use super::registry::{self, EntryKind, RegistryEntry};
use super::secret::SecretBytes;

// Fixed synthetic inputs (documented: not secrets).
const VEC_IKM: [u8; 32] = [
    0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0A, 0x0B, 0x0C, 0x0D, 0x0E, 0x0F,
    0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18, 0x19, 0x1A, 0x1B, 0x1C, 0x1D, 0x1E, 0x1F,
];
const VEC_SALT: [u8; 16] = [
    0x20, 0x21, 0x22, 0x23, 0x24, 0x25, 0x26, 0x27, 0x28, 0x29, 0x2A, 0x2B, 0x2C, 0x2D, 0x2E, 0x2F,
];
const VEC_VAULT_ID: [u8; 16] = [0xA0; 16];
const VEC_VK: [u8; 32] = [0x44; 32];
const VEC_MANIFEST_HASH: [u8; 32] = [0x77; 32];
const VEC_RECOVERY_NONCE: [u8; 16] = [0x88; 16];
const DEVICE_A_SCALAR: [u8; 32] = [0x11; 32];
const DEVICE_B_SCALAR: [u8; 32] = [0x33; 32];

fn hx(bytes: &[u8]) -> Value {
    Value::String(hex::encode(bytes))
}

/// XV-HKDF: every §2.9 info string with fixed ikm/salt → 32-byte output.
pub fn xv_hkdf() -> Value {
    let ikm = SecretBytes::new(VEC_IKM);
    let entries: Vec<Value> = hkdf::ALL_INFO_STRINGS
        .iter()
        .map(|info| {
            let out = hkdf::hkdf32(ikm.expose(), &VEC_SALT, info).unwrap();
            json!({
                "ikm": hex::encode(VEC_IKM),
                "salt": hex::encode(VEC_SALT),
                "info": String::from_utf8_lossy(info),
                "okm32": hex::encode(out.expose()),
            })
        })
        .collect();
    json!({ "family": "XV-HKDF", "kdf": "HKDF-SHA-256 (RFC 5869)", "vectors": entries })
}

/// Produce the sign_input digest and low-S signature for an entry.
fn sign(entry: &RegistryEntry, scalar: [u8; 32]) -> [u8; 64] {
    let input = registry::sign_input(entry).unwrap();
    let (signing, _) = ecdsa::dev_keypair_from_scalar(scalar);
    ecdsa::dev_sign_prehash(&signing, &input)
}

fn base_entry(kind: EntryKind, seq: u64) -> RegistryEntry {
    RegistryEntry {
        seq,
        prev_hash: [0; 32],
        epoch: 0,
        kind,
        device_id: [0x01; 16],
        device_name: None,
        platform: None,
        sign_pub: None,
        agree_pub: None,
        enrolled_at: None,
        authorizer: None,
        revoked_at: None,
        recovery_proof: None,
        manifest_hash: None,
        signature: None,
        prior_epoch: None,
        vault_id: None,
        recovery_nonce: None,
    }
}

fn device_fields(e: &mut RegistryEntry, name: &str, platform: u8, pubkey: [u8; 65]) {
    e.device_name = Some(name.to_string());
    e.platform = Some(platform);
    e.sign_pub = Some(pubkey);
    e.agree_pub = Some(pubkey);
    e.enrolled_at = Some(1_751_200_000);
}

fn entry_json(note: &str, entry: &RegistryEntry, extra: &[(&str, Value)]) -> Value {
    let tlv = entry.encode_tlv(true).unwrap();
    let sign_input = if entry.kind == EntryKind::RecoveryEpoch {
        Value::Null // never signed (§4.4 rule 6)
    } else {
        Value::String(hex::encode(registry::sign_input(entry).unwrap()))
    };
    let mut map = json!({
        "note": note,
        "kind": entry.kind as u8,
        "tlv": hex::encode(&tlv),
        "entry_hash": hex::encode(registry::entry_hash(entry).unwrap()),
        "sign_input": sign_input,
        "signature": entry.signature.map(hex::encode),
        "recovery_proof": entry.recovery_proof.map(hex::encode),
    });
    for (k, v) in extra {
        map[k.to_string()] = v.clone();
    }
    map
}

fn vector_entries() -> Vec<Value> {
    let (_, pub_a) = ecdsa::dev_keypair_from_scalar(DEVICE_A_SCALAR);
    let (_, pub_b) = ecdsa::dev_keypair_from_scalar(DEVICE_B_SCALAR);

    // genesis (self-signed by device A)
    let mut genesis = base_entry(EntryKind::Genesis, 0);
    device_fields(&mut genesis, "Vector Mac", 1, pub_a);
    genesis.authorizer = Some(genesis.device_id);
    genesis.signature = Some(sign(&genesis, DEVICE_A_SCALAR));
    let genesis_json = entry_json("genesis: first device, self-signed (§4.1)", &genesis, &[]);

    // enroll (device B authorized by device A)
    let mut enroll = base_entry(EntryKind::Enroll, 1);
    enroll.prev_hash = registry::entry_hash(&genesis).unwrap();
    enroll.device_id = [0x02; 16];
    device_fields(&mut enroll, "Vector iPhone", 2, pub_b);
    enroll.authorizer = Some(genesis.device_id);
    enroll.signature = Some(sign(&enroll, DEVICE_A_SCALAR));
    let enroll_json = entry_json(
        "enroll: iPhone added by Mac (authorizer = genesis device)",
        &enroll,
        &[],
    );

    // revoke (device B revoked by device A)
    let mut revoke = base_entry(EntryKind::Revoke, 2);
    revoke.prev_hash = registry::entry_hash(&enroll).unwrap();
    revoke.device_id = [0x02; 16];
    revoke.authorizer = Some(genesis.device_id);
    revoke.revoked_at = Some(1_751_210_000);
    revoke.signature = Some(sign(&revoke, DEVICE_A_SCALAR));
    let revoke_json = entry_json("revoke: iPhone removed by Mac", &revoke, &[]);

    // recovery_epoch (new device installs itself by proof, §4.4 rule 6)
    let vk = SecretBytes::new(VEC_VK);
    let mut recovery = base_entry(EntryKind::RecoveryEpoch, 3);
    recovery.prev_hash = registry::entry_hash(&revoke).unwrap();
    recovery.epoch = 1;
    recovery.device_id = [0x03; 16];
    device_fields(&mut recovery, "Recovered Mac", 1, pub_b);
    recovery.manifest_hash = Some(VEC_MANIFEST_HASH);
    recovery.prior_epoch = Some(0);
    recovery.vault_id = Some(VEC_VAULT_ID);
    recovery.recovery_nonce = Some(VEC_RECOVERY_NONCE);
    let proof = registry::compute_recovery_proof(&vk, &VEC_MANIFEST_HASH, &recovery).unwrap();
    let mut recovery_final = recovery.clone();
    recovery_final.recovery_proof = Some(proof);
    let recovery_json = entry_json(
        "recovery_epoch: replacement device self-installs via VK proof (no signature)",
        &recovery_final,
        &[
            ("vk", hx(&VEC_VK)),
            ("manifest_hash", hx(&VEC_MANIFEST_HASH)),
            ("recovery_nonce", hx(&VEC_RECOVERY_NONCE)),
            ("vault_id", hx(&VEC_VAULT_ID)),
        ],
    );

    vec![genesis_json, enroll_json, revoke_json, recovery_json]
}

/// XV-TLV: canonical bytes + hashes + signatures/proofs for all entry kinds.
pub fn xv_tlv() -> Value {
    json!({
        "family": "XV-TLV",
        "encoding": "canonical TLV per spec §4.2 (entry_version=2)",
        "entry_version": registry::ENTRY_VERSION_V2,
        "device_a_scalar": hex::encode(DEVICE_A_SCALAR),
        "device_b_scalar": hex::encode(DEVICE_B_SCALAR),
        "vectors": vector_entries(),
    })
}

/// XV-BIP39: official reference vectors + vault-produced rows.
pub fn xv_bip39() -> Value {
    let mut vectors: Vec<(String, String)> = vec![
        (
            "00000000000000000000000000000000".into(),
            "official 128-bit zeros".into(),
        ),
        (
            "ffffffffffffffffffffffffffffffff".into(),
            "official 128-bit ones".into(),
        ),
        (
            "0000000000000000000000000000000000000000000000000000000000000000".into(),
            "official 256-bit zeros".into(),
        ),
        (
            "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff".into(),
            "official 256-bit ones (RK shape)".into(),
        ),
    ];
    // Vault-produced rows: two fixed synthetic RKs.
    for byte in [0x42u8, 0x99u8] {
        vectors.push((
            hex::encode([byte; 32]),
            format!("vault synthetic 0x{byte:02x}*32"),
        ));
    }
    let rows: Vec<Value> = vectors
        .iter()
        .map(|(entropy_hex, note)| {
            let entropy = hex::decode(entropy_hex).unwrap();
            let words = bip39::encode(&entropy).unwrap();
            json!({
                "entropy": entropy_hex,
                "words": words.join(" "),
                "note": note,
            })
        })
        .collect();
    json!({ "family": "XV-BIP39", "wordlist_sha256": "2f5eed53a4727b4bf8880d8f3f199efc90e58503646d9ff8eff3a2ed3b24dbda", "vectors": rows })
}

/// XV-ECDSA: 65-byte key parsing, low-S production/verification, high-S
/// rejection twin (§2.7). Rust-produced; Apple-signed vectors arrive with
/// Phase E.
pub fn xv_ecdsa() -> Value {
    let (signing, pubkey) = ecdsa::dev_keypair_from_scalar(DEVICE_A_SCALAR);
    let digest: [u8; 32] = Sha256::digest(b"ov0/xv/ecdsa/v1").into();
    let sig = ecdsa::dev_sign_prehash(&signing, &digest);
    let parsed = p256::ecdsa::Signature::from_slice(&sig).unwrap();
    let high_s: p256::Scalar = -*parsed.s();
    let high_sig: [u8; 64] =
        p256::ecdsa::Signature::from_scalars(parsed.r().to_bytes(), high_s.to_bytes())
            .unwrap()
            .to_bytes()
            .into();
    json!({
        "family": "XV-ECDSA",
        "curve": "P-256 (secp256r1), prehashed SHA-256, wire r‖s 64B",
        "signing_scalar_dev_only": hex::encode(DEVICE_A_SCALAR),
        "pubkey_uncompressed": hex::encode(pubkey),
        "digest": hex::encode(digest),
        "signature_low_s": hex::encode(sig),
        "signature_high_s_twin_must_reject": hex::encode(high_sig),
        "is_low_s": ecdsa::is_low_s(&sig),
    })
}

/// XV-RECOVERY-EPOCH: VK + manifest + new-device keys + nonce → proof.
pub fn xv_recovery_epoch() -> Value {
    let (_, pub_b) = ecdsa::dev_keypair_from_scalar(DEVICE_B_SCALAR);
    let vk = SecretBytes::new(VEC_VK);
    let mut entry = base_entry(EntryKind::RecoveryEpoch, 7);
    entry.prev_hash = [0x66; 32];
    entry.epoch = 1;
    entry.device_id = [0x03; 16];
    device_fields(&mut entry, "Recovered Mac", 1, pub_b);
    entry.manifest_hash = Some(VEC_MANIFEST_HASH);
    entry.prior_epoch = Some(0);
    entry.vault_id = Some(VEC_VAULT_ID);
    entry.recovery_nonce = Some(VEC_RECOVERY_NONCE);
    let proof = registry::compute_recovery_proof(&vk, &VEC_MANIFEST_HASH, &entry).unwrap();
    let mut complete = entry.clone();
    complete.recovery_proof = Some(proof);
    let mut without_proof_wire = entry.clone();
    without_proof_wire.recovery_proof = Some([0u8; 32]); // placeholder; excluded from bytes below
    json!({
        "family": "XV-RECOVERY-EPOCH",
        "construction": "HMAC-SHA256(HKDF(VK, salt=manifest_hash, info=ov0/recovery-auth/v1), ov0/registry/recovery/v1 ‖ tlv(entry ∖ 0x0E))",
        "vk": hex::encode(VEC_VK),
        "manifest_hash": hex::encode(VEC_MANIFEST_HASH),
        "new_device": {
            "device_id": hex::encode([0x03; 16]),
            "sign_pub": hex::encode(pub_b),
            "agree_pub": hex::encode(pub_b),
            "platform": 1,
            "device_name": "Recovered Mac",
        },
        "prev_hash": hex::encode([0x66; 32]),
        "prior_epoch": 0,
        "epoch": 1,
        "recovery_nonce": hex::encode(VEC_RECOVERY_NONCE),
        "vault_id": hex::encode(VEC_VAULT_ID),
        "tlv_without_proof": hex::encode(without_proof_wire.encode_tlv(false).unwrap()),
        "tlv": hex::encode(complete.encode_tlv(true).unwrap()),
        "recovery_proof": hex::encode(proof),
    })
}

/// All Phase B families as (file stem, document).
pub fn all() -> Vec<(&'static str, Value)> {
    vec![
        ("xv_hkdf", xv_hkdf()),
        ("xv_tlv", xv_tlv()),
        ("xv_bip39", xv_bip39()),
        ("xv_ecdsa", xv_ecdsa()),
        ("xv_recovery_epoch", xv_recovery_epoch()),
    ]
}
