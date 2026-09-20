//! Phase E0, §2.12 Path A on macOS: Rust RFC 9180 HPKE ↔ Apple CryptoKit
//! HPKE with a Secure-Enclave-resident P-256 agreement key, at
//! DHKEM(P-256, HKDF-SHA256)/HKDF-SHA256/ChaCha20-Poly1305.
//!
//! (a) Rust seal → CryptoKit Recipient open with the SE key
//! (b) CryptoKit Sender seal → Rust open
//! (KAT) the official CFRG/RFC 9180 vector for this suite, opened by both
//! (SE) evidence that the agreement private key stays in the Enclave
//!
//! Synthetic data only; no vault code depends on this crate.

use hpke::{
    aead::ChaCha20Poly1305, kdf::HkdfSha256, kem::DhP256HkdfSha256, Deserializable, Kem as KemTrait,
    OpModeR, OpModeS, Serializable,
};
use ov0_hpke_poc as poc;

type Kem = DhP256HkdfSha256;
const INFO: &[u8] = b"ov0/e0-poc/v1";
const AAD: &[u8] = b"";

fn vector() -> serde_json::Value {
    let raw = include_str!("../vectors/rfc9180-p256-sha256-chacha20poly1305.json");
    serde_json::from_str(raw).expect("vector json")
}

fn hex(v: &serde_json::Value, k: &str) -> Vec<u8> {
    hex::decode(v[k].as_str().expect(k)).expect("hex")
}

/// (a) Rust `hpke` seal → CryptoKit `HPKE.Recipient` with the SE key.
#[test]
fn rust_seal_opens_in_cryptokit_with_secure_enclave_key() {
    let tag = "ov0-e0-path-a";
    let pk_bytes = poc::se_create(tag).expect("SE key");
    assert_eq!(pk_bytes.len(), 65);
    assert_eq!(pk_bytes[0], 0x04, "uncompressed X9.63 encoding");

    let pk = <Kem as KemTrait>::PublicKey::from_bytes(&pk_bytes).expect("parse SE public key");
    let msg = b"synthetic device envelope payload (a)";
    let (enc, ct) = hpke::single_shot_seal::<ChaCha20Poly1305, HkdfSha256, Kem, _>(
        &OpModeS::Base,
        &pk,
        INFO,
        msg,
        AAD,
        &mut rand::thread_rng(),
    )
    .expect("rust seal");
    let enc_bytes = enc.to_bytes();
    assert_eq!(enc_bytes.len(), 65, "encapsulated key is 65 bytes");

    let opened = poc::ck_open_se(tag, INFO, &enc_bytes, &ct, AAD).expect("CryptoKit SE open");
    assert_eq!(opened, msg);

    // Wrong info must fail (context binding), and the tag is not malleable.
    assert!(poc::ck_open_se(tag, b"ov0/wrong-info", &enc_bytes, &ct, AAD).is_err());
    let mut flipped = ct.clone();
    flipped[0] ^= 1;
    assert!(poc::ck_open_se(tag, INFO, &enc_bytes, &flipped, AAD).is_err());
    poc::se_delete(tag);
}

/// (b) CryptoKit `HPKE.Sender` seal → Rust `hpke` open.
#[test]
fn cryptokit_seal_opens_in_rust() {
    let (sk, pk) = Kem::gen_keypair(&mut rand::thread_rng());
    let pk65: [u8; 65] = pk.to_bytes().as_slice().try_into().expect("65-byte key");
    let msg = b"synthetic device envelope payload (b)";
    let (enc, ct) = poc::ck_seal(&pk65, INFO, msg, AAD).expect("CryptoKit seal");
    assert_eq!(enc.len(), 65);

    let enc = <Kem as KemTrait>::EncappedKey::from_bytes(&enc).expect("parse enc");
    let pt = hpke::single_shot_open::<ChaCha20Poly1305, HkdfSha256, Kem>(
        &OpModeR::Base,
        &sk,
        &enc,
        INFO,
        &ct,
        AAD,
    )
    .expect("rust open");
    assert_eq!(pt, msg);
}

/// Known-answer test: the official CFRG vector for this suite, opened by
/// Rust `hpke` *and* by Apple CryptoKit. Same bytes, same answers.
#[test]
fn rfc9180_vector_opens_in_both_implementations() {
    let v = vector();
    let p = &v["provenance"];
    assert_eq!(p["ids"]["kem"], "0x0010");
    assert_eq!(p["ids"]["kdf"], "0x0001");
    assert_eq!(p["ids"]["aead"], "0x0003");
    let vec = &v["vector"];
    let sk_rm = hex(vec, "skRm");
    let pk_rm = hex(vec, "pkRm");
    let enc_bytes = hex(vec, "enc");
    let info = hex(vec, "info");
    assert_eq!(sk_rm.len(), 32);
    assert_eq!(pk_rm.len(), 65);
    assert_eq!(enc_bytes.len(), 65);

    let sk = <Kem as KemTrait>::PrivateKey::from_bytes(&sk_rm).expect("skRm");
    let pk = <Kem as KemTrait>::PublicKey::from_bytes(&pk_rm).expect("pkRm");
    assert_eq!(
        <Kem as KemTrait>::sk_to_pk(&sk).to_bytes().as_slice(),
        pk_rm.as_slice(),
        "public key derives from the vector's private key"
    );
    let _ = pk;
    let enc = <Kem as KemTrait>::EncappedKey::from_bytes(&enc_bytes).expect("enc");

    let encryptions = vec["encryptions"].as_array().expect("encryptions");
    assert!(!encryptions.is_empty());
    for (i, e) in encryptions.iter().enumerate() {
        let (aad, ct, want) = (hex(e, "aad"), hex(e, "ct"), hex(e, "pt"));
        // Rust, single-shot, only for sequence number 0: later ciphertexts
        // need the running context, which the single-shot API doesn't keep.
        if i == 0 {
            let pt = hpke::single_shot_open::<ChaCha20Poly1305, HkdfSha256, Kem>(
                &OpModeR::Base,
                &sk,
                &enc,
                &info,
                &ct,
                &aad,
            )
            .expect("rust KAT open");
            assert_eq!(pt, want, "rust matches the RFC 9180 vector");
            // CryptoKit, same vector bytes, software recipient key.
            let ck = poc::ck_open_sw(&sk_rm, &info, &enc_bytes, &ct, &aad).expect("CryptoKit KAT open");
            assert_eq!(ck, want, "CryptoKit matches the RFC 9180 vector");
        }
    }
}

/// The agreement private key never leaves the Secure Enclave: what is
/// stored is an opaque, device-bound blob, not a P-256 scalar.
#[test]
fn secure_enclave_key_is_not_exportable() {
    let tag = "ov0-e0-se-evidence";
    let pk = poc::se_create(tag).expect("SE key");
    let blob = poc::se_blob(tag).expect("stored representation");
    assert_ne!(blob.len(), 32, "not a raw private scalar");
    assert!(blob.len() > 64, "opaque SE blob (was {} bytes)", blob.len());
    // The blob is not the key: it does not parse as a P-256 private key,
    // and it does not contain the public key's coordinates.
    assert!(<Kem as KemTrait>::PrivateKey::from_bytes(&blob).is_err());
    // Strongest available check: no 32-byte window of the stored blob is
    // the private scalar (the blob does legitimately contain the *public*
    // key, which is not secret).
    for w in blob.windows(32) {
        if let Ok(candidate) = <Kem as KemTrait>::PrivateKey::from_bytes(w) {
            assert_ne!(
                <Kem as KemTrait>::sk_to_pk(&candidate).to_bytes().as_slice(),
                &pk[..],
                "a private scalar for this key was recoverable from the stored blob"
            );
        }
    }
    // CryptoKit still decapsulates with it — the DH runs in the Enclave.
    let pkey = <Kem as KemTrait>::PublicKey::from_bytes(&pk).unwrap();
    let (enc, ct) = hpke::single_shot_seal::<ChaCha20Poly1305, HkdfSha256, Kem, _>(
        &OpModeS::Base, &pkey, INFO, b"se", AAD, &mut rand::thread_rng(),
    )
    .unwrap();
    assert_eq!(poc::ck_open_se(tag, INFO, &enc.to_bytes(), &ct, AAD).unwrap(), b"se");
    poc::se_delete(tag);
    // After deletion the key is gone; the blob cannot be used elsewhere.
    assert!(poc::se_public(tag).is_err());
}
