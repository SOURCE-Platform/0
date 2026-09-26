//! CR-01…CR-04, CR-09, CR-10, CR-12 (spec §16.1): wrap cryptography.
//! Synthetic data only; no real credentials.

mod common;

use common::*;
use vault_helper::crypto::kdf::{self, Argon2Params};
use vault_helper::crypto::secret::{random_secret, SecretBytes};
use vault_helper::crypto::wrap::{self, DeviceEnvelopePayload, RecoveryWrapPayload};
use vault_helper::crypto::CryptoError;

fn payload_fields(p: &RecoveryWrapPayload) -> (u64, u32, [u8; 32]) {
    (p.wrapped_at, p.vk_generation, *p.vk.expose())
}

/// CR-01: MP wrap → unwrap round trip, byte-identical payload. Runs once
/// at the production v1 tuple (m=64 MiB, t=3, p=1) so the gate covers the
/// real parameter path, not only the fast test params.
#[test]
fn cr01_mp_wrap_roundtrip() {
    let payload = sample_payload(0x11);
    let pk = pk(Argon2Params::V1);
    let file = wrap::seal_wrap_mp(&payload, &pk, &VAULT_ID, Argon2Params::V1, &KDF_SALT).unwrap();
    assert_eq!(file.kind, "mp");
    assert_eq!(file.kdf_version, kdf::KDF_VERSION_V1);
    let opened = wrap::open_wrap_mp(&file, &pk, &VAULT_ID).unwrap();
    assert_eq!(payload_fields(&opened), payload_fields(&payload));
    // payload re-encode is byte-identical
    assert_eq!(opened.encode(), payload.encode());
}

/// CR-02: RK wrap → unwrap round trip.
#[test]
fn cr02_rk_wrap_roundtrip() {
    let payload = sample_payload(0x22);
    let rk = random_secret();
    let file = wrap::seal_wrap_rk(&payload, &rk, &VAULT_ID).unwrap();
    assert_eq!(file.kind, "rk");
    let opened = wrap::open_wrap_rk(&file, &rk, &VAULT_ID).unwrap();
    assert_eq!(payload_fields(&opened), payload_fields(&payload));
    assert_eq!(opened.encode(), payload.encode());
}

/// CR-03: wrong MP (1-bit flip) → AEAD failure, no partial state.
#[test]
fn cr03_wrong_mp_fails_closed() {
    let payload = sample_payload(0x33);
    let pk = pk(FAST);
    let file = wrap::seal_wrap_mp(&payload, &pk, &VAULT_ID, FAST, &KDF_SALT).unwrap();
    let mut wrong = MP.to_vec();
    wrong[0] ^= 1;
    let wrong_pk = kdf::derive_pk(&wrong, &KDF_SALT, FAST).unwrap();
    let result = wrap::open_wrap_mp(&file, &wrong_pk, &VAULT_ID);
    assert_eq!(result.err(), Some(CryptoError::IntegrityFailure));
}

/// CR-04: wrong RK (valid checksum, wrong entropy) → AEAD failure.
#[test]
fn cr04_wrong_rk_fails_closed() {
    let payload = sample_payload(0x44);
    let rk = random_secret();
    let file = wrap::seal_wrap_rk(&payload, &rk, &VAULT_ID).unwrap();
    // A *valid-checksum* mnemonic for different entropy (BIP-39 codec path):
    let other = SecretBytes::new([0x99; 32]);
    let words = vault_helper::crypto::bip39::encode_rk(&other);
    let wrong_rk = vault_helper::crypto::bip39::decode_rk(&words).unwrap();
    assert_ne!(wrong_rk.expose(), rk.expose());
    let result = wrap::open_wrap_rk(&file, &wrong_rk, &VAULT_ID);
    assert_eq!(result.err(), Some(CryptoError::IntegrityFailure));
}

/// CR-09: kdf_version downgrade refused (§2.3).
#[test]
fn cr09_kdf_downgrade_refused() {
    assert!(matches!(
        kdf::check_no_kdf_downgrade(1, 2),
        Err(CryptoError::KdfDowngrade)
    ));
    assert!(kdf::check_no_kdf_downgrade(2, 2).is_ok());
    assert!(kdf::check_no_kdf_downgrade(3, 2).is_ok());
}

/// CR-10: wrap header tamper — salt or params swapped between files —
/// breaks the derivation chain and fails AEAD (§16.1). The test drives
/// the full MP → derive(file params) → open path, as the op layer will.
#[test]
fn cr10_wrap_header_tamper_fails() {
    let salt_b = [0xC1; 16];
    let payload = sample_payload(0x55);
    let pk_a = pk(FAST);
    let file_a = wrap::seal_wrap_mp(&payload, &pk_a, &VAULT_ID, FAST, &KDF_SALT).unwrap();

    // Full-path open helper: derive PK from the file's own header block.
    let open_full = |file: &wrap::PasswordWrapFile| {
        let salt: [u8; 16] = vault_helper::crypto::hex::decode_array(&file.argon2id.salt).unwrap();
        let params = Argon2Params {
            m: file.argon2id.m,
            t: file.argon2id.t,
            p: file.argon2id.p,
        };
        let pk = kdf::derive_pk(MP, &salt, params).unwrap();
        wrap::open_wrap_mp(file, &pk, &VAULT_ID)
    };
    assert!(open_full(&file_a).is_ok());

    // Salt swapped in from another wrap file.
    let mut tampered = wrap::PasswordWrapFile {
        argon2id: wrap::KdfBlock {
            salt: vault_helper::crypto::hex::encode(salt_b),
            ..file_a.argon2id.clone()
        },
        ..clone_wrap(&file_a)
    };
    assert!(open_full(&tampered).is_err(), "salt swap must fail");

    // Params swapped (salt intact): derived PK differs → AEAD failure.
    tampered = wrap::PasswordWrapFile {
        argon2id: wrap::KdfBlock {
            m: FAST.m * 2,
            ..file_a.argon2id.clone()
        },
        ..clone_wrap(&file_a)
    };
    assert!(open_full(&tampered).is_err(), "param swap must fail");
}

fn clone_wrap(f: &wrap::PasswordWrapFile) -> wrap::PasswordWrapFile {
    serde_json::from_value(serde_json::to_value(f).unwrap()).unwrap()
}

/// CR-12: the wrap payload split holds — device_backup_cred is present in
/// every DeviceEnvelopePayload and absent from both recovery wraps,
/// including after rotation (§2.2, v0.2 finding 1).
#[test]
fn cr12_payload_split_holds() {
    let vk_a = random_secret();
    let recovery = RecoveryWrapPayload {
        vk: vk_a,
        wrapped_at: 1,
        vk_generation: 0,
    };
    // v0.4 CR-12: both payload shapes are exactly {vk, wrapped_at,
    // vk_generation}; the retired credential tag 0x04 is refused by both.
    let bytes = recovery.encode();
    let tags = |b: &[u8]| -> Vec<u8> { vault_helper::crypto::tlv::EntryReader::parse(b).unwrap().tags().collect() };
    assert_eq!(tags(&bytes), vec![0x01, 0x02, 0x03]);
    let device = DeviceEnvelopePayload { vk: random_secret(), wrapped_at: 1, vk_generation: 0 };
    assert_eq!(tags(&device.encode()), vec![0x01, 0x02, 0x03]);
    assert!(DeviceEnvelopePayload::parse(&device.encode()).is_ok());
    let with_cred = vault_helper::crypto::tlv::EntryBuilder::new()
        .field_bytes(0x01, &[1; 32])
        .and_then(|b| b.field_uint(0x02, 1))
        .and_then(|b| b.field_uint(0x03, 0))
        .and_then(|b| b.field_bytes(0x04, &[2; 32]))
        .unwrap()
        .build();
    assert!(matches!(DeviceEnvelopePayload::parse(&with_cred), Err(CryptoError::FieldPresence)));
    assert!(matches!(RecoveryWrapPayload::parse(&with_cred), Err(CryptoError::FieldPresence)));

    // After rotation both recovery wraps still carry no backup credential.
    let rk = random_secret();
    let pk = pk(FAST);
    let new_vk = random_secret();
    let mp_file = wrap::seal_wrap_mp(&recovery, &pk, &VAULT_ID, FAST, &KDF_SALT).unwrap();
    let rk_file = wrap::seal_wrap_rk(&recovery, &rk, &VAULT_ID).unwrap();
    let mp_rot =
        vault_helper::crypto::rotate::rotate_wrap_mp(&mp_file, &pk, &VAULT_ID, &new_vk, 2, 1)
            .unwrap();
    let rk_rot =
        vault_helper::crypto::rotate::rotate_wrap_rk(&rk_file, &rk, &VAULT_ID, &new_vk, 2, 1)
            .unwrap();
    for payload in [
        wrap::open_wrap_mp(&mp_rot, &pk, &VAULT_ID).unwrap(),
        wrap::open_wrap_rk(&rk_rot, &rk, &VAULT_ID).unwrap(),
    ] {
        assert_eq!(tags(&payload.encode()), vec![0x01, 0x02, 0x03]);
    }
}
