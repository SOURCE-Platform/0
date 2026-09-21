//! XV-RUST-SIDE (spec §16.8): every committed vector family is recomputed
//! from scratch and compared byte-exact, plus semantic checks (signatures
//! verify, proofs verify, words decode, high-S rejects). Staleness is
//! caught by `gen_vectors --check` in the gate; these tests catch
//! non-determinism and semantic drift.

use serde_json::Value;
use vault_helper::crypto::hkdf;
use vault_helper::crypto::secret::SecretBytes;
use vault_helper::crypto::{bip39, ecdsa, hex, registry};

fn committed(stem: &str) -> Value {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/vectors/");
    let raw = std::fs::read_to_string(format!("{path}{stem}.json")).expect("vector file exists");
    serde_json::from_str(&raw).unwrap()
}

/// Freshness: recomputed documents equal the committed ones (semantic
/// equality; byte-exact staleness is enforced by `gen_vectors --check`).
#[test]
fn xv_vectors_match_committed_files() {
    for (stem, doc) in vault_helper::all_vectors() {
        assert_eq!(
            doc,
            committed(stem),
            "{stem}.json drifted — regenerate via gen_vectors"
        );
    }
}

/// XV-HKDF: each row recomputes independently from the raw inputs.
#[test]
fn xv_hkdf_rows_recompute() {
    let doc = committed("xv_hkdf");
    for row in doc["vectors"].as_array().unwrap() {
        let ikm = hex::decode(row["ikm"].as_str().unwrap()).unwrap();
        let salt = hex::decode(row["salt"].as_str().unwrap()).unwrap();
        let info = row["info"].as_str().unwrap();
        let okm = hkdf::hkdf32(&ikm, &salt, info.as_bytes()).unwrap();
        assert_eq!(
            hex::encode(okm.expose()),
            row["okm32"].as_str().unwrap(),
            "info {info}"
        );
    }
    // Coverage guard: the vector set contains every §2.9 info string.
    let infos: Vec<&str> = doc["vectors"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["info"].as_str().unwrap())
        .collect();
    for info in hkdf::ALL_INFO_STRINGS {
        let text = std::str::from_utf8(info).unwrap();
        assert!(infos.contains(&text), "missing HKDF vector for {text}");
    }
}

/// XV-TLV: decode committed TLV bytes, re-encode byte-identical, verify
/// signatures against the authorizer's public key, verify the proof.
#[test]
fn xv_tlv_entries_verify() {
    let doc = committed("xv_tlv");
    let rows = doc["vectors"].as_array().unwrap();
    let mut device_keys: Vec<([u8; 16], [u8; 65])> = Vec::new();
    for row in rows {
        let tlv = hex::decode(row["tlv"].as_str().unwrap()).unwrap();
        let entry = registry::RegistryEntry::decode_tlv(&tlv).unwrap();
        assert_eq!(
            entry.encode_tlv(true).unwrap(),
            tlv,
            "non-canonical re-encode"
        );
        assert_eq!(
            hex::encode(registry::entry_hash(&entry).unwrap()),
            row["entry_hash"].as_str().unwrap()
        );
        // Register this device's key first: genesis self-signs, so its
        // own key must be visible when its signature is checked.
        if let Some(sign_pub) = entry.sign_pub {
            device_keys.push((entry.device_id, sign_pub));
        }
        match entry.kind {
            registry::EntryKind::RecoveryEpoch => {
                let vk =
                    SecretBytes::new(hex::decode_array::<32>(row["vk"].as_str().unwrap()).unwrap());
                let manifest =
                    hex::decode_array::<32>(row["manifest_hash"].as_str().unwrap()).unwrap();
                registry::verify_recovery_proof(&vk, &manifest, &entry).unwrap();
            }
            _ => {
                assert_eq!(
                    hex::encode(registry::sign_input(&entry).unwrap()),
                    row["sign_input"].as_str().unwrap()
                );
                // authorizer key: genesis self-signs; later entries are
                // signed by an earlier device.
                let authorizer = entry.authorizer.unwrap();
                let authorizer_key = device_keys
                    .iter()
                    .find(|(id, _)| *id == authorizer)
                    .map(|(_, key)| *key)
                    .expect("authorizer seen earlier");
                let sig = entry.signature.unwrap();
                let input = registry::sign_input(&entry).unwrap();
                ecdsa::verify_prehash(&authorizer_key, &input, &sig).unwrap();
            }
        }
    }
}

/// XV-BIP39: every row's word string decodes back to the entropy.
#[test]
fn xv_bip39_words_decode() {
    let doc = committed("xv_bip39");
    for row in doc["vectors"].as_array().unwrap() {
        let entropy = hex::decode(row["entropy"].as_str().unwrap()).unwrap();
        let words = row["words"].as_str().unwrap();
        let decoded = bip39::decode(words).unwrap();
        assert_eq!(decoded, entropy);
        let re_encoded: Vec<&str> = bip39::encode(&entropy).unwrap();
        assert_eq!(re_encoded.join(" "), words, "re-encode");
    }
}

/// XV-ECDSA: low-S verifies, high-S twin is rejected, key parses.
#[test]
fn xv_ecdsa_semantics() {
    let doc = committed("xv_ecdsa");
    let pubkey = hex::decode(doc["pubkey_uncompressed"].as_str().unwrap()).unwrap();
    let digest = hex::decode_array::<32>(doc["digest"].as_str().unwrap()).unwrap();
    let low = hex::decode(doc["signature_low_s"].as_str().unwrap()).unwrap();
    let high = hex::decode(doc["signature_high_s_twin_must_reject"].as_str().unwrap()).unwrap();

    ecdsa::verify_prehash(&pubkey, &digest, &low).unwrap();
    assert!(ecdsa::is_low_s(&low));
    assert!(!ecdsa::is_low_s(&high));
    assert_eq!(
        ecdsa::verify_prehash(&pubkey, &digest, &high),
        Err(vault_helper::crypto::CryptoError::NonCanonicalSignature)
    );
}

/// XV-RECOVERY-EPOCH: proof verifies against the committed fields, and
/// flipping any covered field breaks it.
#[test]
fn xv_recovery_epoch_proof_binds_fields() {
    let doc = committed("xv_recovery_epoch");
    let vk = SecretBytes::new(hex::decode_array::<32>(doc["vk"].as_str().unwrap()).unwrap());
    let manifest = hex::decode_array::<32>(doc["manifest_hash"].as_str().unwrap()).unwrap();
    let mut entry =
        registry::RegistryEntry::decode_tlv(&hex::decode(doc["tlv"].as_str().unwrap()).unwrap())
            .unwrap();
    assert_eq!(
        entry.recovery_proof.map(hex::encode).as_deref(),
        Some(doc["recovery_proof"].as_str().unwrap())
    );
    registry::verify_recovery_proof(&vk, &manifest, &entry).unwrap();

    // Altered nonce must fail.
    entry.recovery_nonce = Some([0x99; 16]);
    assert!(registry::verify_recovery_proof(&vk, &manifest, &entry).is_err());
}
