//! CR-05…CR-08 (spec §16.1): record cryptography and VK rotation.
//! Synthetic data only.

mod common;

use common::*;
use std::collections::HashSet;
use vault_helper::crypto::record::{self, RecordCiphertext, RevBinding};
use vault_helper::crypto::rotate::{self, SealedRecord};
use vault_helper::crypto::secret::random_secret;
use vault_helper::crypto::wrap;
use vault_helper::crypto::CryptoError;

const PLAINTEXT: &[u8] = br#"{"kind":"login","title":"synthetic","password":"not-real"}"#;

/// Synthetic revision binding (§2.6 v0.4): revision_id + graph_digest.
const BIND: RevBinding = RevBinding { revision_id: [0x11; 32], graph_digest: [0x22; 32] };

fn seal(vk: &vault_helper::crypto::secret::SecretBytes<32>, gen: u32) -> RecordCiphertext {
    record::seal_record(vk, &VAULT_ID, &RECORD_ID, &BIND, 1, gen, PLAINTEXT).unwrap()
}

/// CR-05: record ciphertext 1-byte tamper → integrity failure, no
/// plaintext (maps to RECORD_CORRUPT at the op layer).
#[test]
fn cr05_record_tamper_fails() {
    let vk = random_secret();
    let mut sealed = seal(&vk, 0);
    sealed.ct[0] ^= 1;
    let result = record::open_record(&vk, &VAULT_ID, &RECORD_ID, &BIND, 1, 0, &sealed);
    assert_eq!(result.err(), Some(CryptoError::IntegrityFailure));
    // tag tamper too (last 16 bytes are the Poly1305 tag)
    let mut sealed = seal(&vk, 0);
    let n = sealed.ct.len();
    sealed.ct[n - 1] ^= 1;
    assert!(record::open_record(&vk, &VAULT_ID, &RECORD_ID, &BIND, 1, 0, &sealed).is_err());
}

/// CR-06: AAD tamper — record_id, generation, schema swapped — decryption
/// fails in every case.
#[test]
fn cr06_aad_tamper_fails() {
    let vk = random_secret();
    let sealed = seal(&vk, 0);
    assert!(record::open_record(&vk, &VAULT_ID, &RECORD_ID, &BIND, 1, 0, &sealed).is_ok());
    assert!(record::open_record(&vk, &VAULT_ID, &[0xB1; 16], &BIND, 1, 0, &sealed).is_err());
    assert!(record::open_record(&vk, &VAULT_ID, &RECORD_ID, &BIND, 2, 0, &sealed).is_err());
    assert!(record::open_record(&vk, &VAULT_ID, &RECORD_ID, &BIND, 1, 1, &sealed).is_err());
    assert!(record::open_record(&vk, &[0xA1; 16], &RECORD_ID, &BIND, 1, 0, &sealed).is_err());
    // v0.4: moving a ciphertext to another revision id or graph position
    // (parents, author, counter, flags, kind) fails too.
    let other_id = RevBinding { revision_id: [0x12; 32], ..BIND };
    let other_graph = RevBinding { graph_digest: [0x23; 32], ..BIND };
    assert!(record::open_record(&vk, &VAULT_ID, &RECORD_ID, &other_id, 1, 0, &sealed).is_err());
    assert!(record::open_record(&vk, &VAULT_ID, &RECORD_ID, &other_graph, 1, 0, &sealed).is_err());
}

/// CR-07: nonce strategy audit — 2^20 seals, no nonce/key pair repeats.
/// Construction: 192-bit random nonces per seal (§2.5); this is the
/// statistical guard.
#[test]
fn cr07_nonce_uniqueness_over_2_to_20_seals() {
    let vk = random_secret();
    let mut seen: HashSet<[u8; 24]> = HashSet::with_capacity(1 << 20);
    for _ in 0..(1u32 << 20) {
        let sealed = seal(&vk, 0);
        assert!(
            seen.insert(sealed.nonce),
            "nonce repeated under the same key"
        );
    }
    assert_eq!(seen.len(), 1 << 20);
}

/// CR-08: VK rotation — full vault re-seal. Old VK fails on every new
/// record and wrap; new VK opens all (§2.10).
#[test]
fn cr08_vk_rotation_reseals_everything() {
    let old_vk = random_secret();
    let new_vk = random_secret();
    let pk = pk(FAST);
    let rk = random_secret();

    // Synthetic vault: 3 records sealed under old_vk at generation 0, plus
    // both recovery wraps wrapping old_vk.
    let records: Vec<SealedRecord> = (0u8..3)
        .map(|i| SealedRecord {
            record_id: [0xE0 + i; 16],
            bind: BIND,
            schema_version: 1,
            vk_generation: 0,
            ciphertext: record::seal_record(&old_vk, &VAULT_ID, &[0xE0 + i; 16], &BIND, 1, 0, PLAINTEXT)
                .unwrap(),
        })
        .collect();
    let wrap_payload =
        |vk: &vault_helper::crypto::secret::SecretBytes<32>| wrap::RecoveryWrapPayload {
            vk: vault_helper::crypto::secret::SecretBytes::new(*vk.expose()),
            wrapped_at: 1,
            vk_generation: 0,
        };
    let mp_file =
        wrap::seal_wrap_mp(&wrap_payload(&old_vk), &pk, &VAULT_ID, FAST, &KDF_SALT).unwrap();
    let rk_file = wrap::seal_wrap_rk(&wrap_payload(&old_vk), &rk, &VAULT_ID).unwrap();

    // Rotate: records + both wraps to generation 1 under new_vk.
    let rotated: Vec<SealedRecord> = records
        .iter()
        .map(|r| rotate::rotate_record(r, &old_vk, &new_vk, &VAULT_ID, 1).unwrap())
        .collect();
    let mp_rot = rotate::rotate_wrap_mp(&mp_file, &pk, &VAULT_ID, &new_vk, 2, 1).unwrap();
    let rk_rot = rotate::rotate_wrap_rk(&rk_file, &rk, &VAULT_ID, &new_vk, 2, 1).unwrap();

    for r in &rotated {
        assert_eq!(r.vk_generation, 1);
        assert!(
            record::open_record(&old_vk, &VAULT_ID, &r.record_id, &BIND, 1, 1, &r.ciphertext).is_err(),
            "old VK must fail on rotated records"
        );
        let opened =
            record::open_record(&new_vk, &VAULT_ID, &r.record_id, &BIND, 1, 1, &r.ciphertext).unwrap();
        assert_eq!(&*opened, PLAINTEXT, "new VK opens every rotated record");
    }
    let from_mp = wrap::open_wrap_mp(&mp_rot, &pk, &VAULT_ID).unwrap();
    let from_rk = wrap::open_wrap_rk(&rk_rot, &rk, &VAULT_ID).unwrap();
    assert_eq!(from_mp.vk.expose(), new_vk.expose());
    assert_eq!(from_rk.vk.expose(), new_vk.expose());
    assert_eq!(from_mp.vk_generation, 1);
    assert_eq!(from_rk.vk_generation, 1);

    // Rotated wraps yield only the new VK; the old VK no longer appears in
    // any newly written object (§2.10: wraps rewritten, records re-sealed).
    assert_ne!(from_mp.vk.expose(), old_vk.expose());
    // Sanity: pre-rotation wraps did carry the old VK.
    assert_eq!(
        wrap::open_wrap_mp(&mp_file, &pk, &VAULT_ID)
            .unwrap()
            .vk
            .expose(),
        old_vk.expose()
    );
}
