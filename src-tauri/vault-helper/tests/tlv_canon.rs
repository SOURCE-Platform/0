//! RG-10 (spec §16.2): TLV canonicalization. Reordered tags, padded
//! integers, non-NFC strings, trailing bytes, reserved tags, and unknown
//! tags must all be rejected (or, at encode, deterministically
//! normalized) such that decode→re-encode is byte-identical.

mod common;

use vault_helper::crypto::registry::{self, EntryKind, RegistryEntry};
use vault_helper::crypto::tlv::{self, EntryBuilder, EntryReader, TlvError};

fn genesis_entry() -> RegistryEntry {
    let (_, sign_pub) = vault_helper::crypto::ecdsa::dev_keypair_from_scalar([0x11; 32]);
    let (_, agree_pub) = vault_helper::crypto::ecdsa::dev_keypair_from_scalar([0x22; 32]);
    RegistryEntry {
        seq: 0,
        prev_hash: [0; 32],
        epoch: 0,
        kind: EntryKind::Genesis,
        device_id: [0x01; 16],
        device_name: Some("Test Mac".to_string()),
        platform: Some(1),
        sign_pub: Some(sign_pub),
        agree_pub: Some(agree_pub),
        enrolled_at: Some(1_751_200_000),
        authorizer: Some([0x01; 16]),
        revoked_at: None,
        recovery_proof: None,
        manifest_hash: None,
        signature: Some([0x5A; 64]),
        prior_epoch: None,
        vault_id: None,
        recovery_nonce: None,
    }
}

/// Byte-identical re-encode through decode → encode.
#[test]
fn rg10_decode_reencode_is_byte_identical() {
    let entry = genesis_entry();
    let bytes = entry.encode_tlv(true).unwrap();
    let decoded = RegistryEntry::decode_tlv(&bytes).unwrap();
    assert_eq!(decoded, entry);
    assert_eq!(decoded.encode_tlv(true).unwrap(), bytes);
}

/// Reordered tags: rejected at decode.
#[test]
fn rg10_reordered_tags_rejected() {
    let bytes = EntryBuilder::new()
        .field_uint(0x02, 0)
        .unwrap()
        .field_uint(0x01, 2);
    assert_eq!(bytes.err(), Some(TlvError::TagOrder));

    // hand-build a reordered wire form to prove decode rejects it too
    let mut raw = vec![0x02u8, 0, 0, 0, 1, 0x00];
    raw.extend_from_slice(&[0x01, 0, 0, 0, 1, 0x02, 0xFF]);
    assert_eq!(EntryReader::parse(&raw).err(), Some(TlvError::TagOrder));
}

/// Padded integers: rejected (encode never emits them).
#[test]
fn rg10_padded_integers_rejected() {
    assert_eq!(
        tlv::decode_uint(&[0x00, 0x02]),
        Err(TlvError::PaddedInteger)
    );
    assert_eq!(tlv::decode_uint(&[0x00]), Ok(0));
    let mut raw = vec![0x01u8, 0, 0, 0, 2, 0x00, 0x02, 0xFF];
    assert_eq!(
        EntryReader::parse(&raw).and_then(|r| r.get_uint(0x01).map(|_| ())),
        Err(TlvError::PaddedInteger)
    );
    raw = vec![0x01u8, 0, 0, 0, 9, 1, 2, 3, 4, 5, 6, 7, 8, 9, 0xFF];
    assert_eq!(
        EntryReader::parse(&raw).and_then(|r| r.get_uint(0x01).map(|_| ())),
        Err(TlvError::IntegerTooLong)
    );
}

/// Non-NFC strings: encode normalizes; decode rejects non-NFC input, so
/// any accepted byte string re-encodes byte-identically.
#[test]
fn rg10_non_nfc_handling_is_canonical() {
    // "é" as e + U+0301 combining acute (NFD) vs U+00E9 (NFC).
    let nfd = "Ad\u{0065}\u{0301}m's Mac";
    let nfc = "Ad\u{00e9}m's Mac";
    let encoded = EntryBuilder::new().field_string(0x07, nfd).unwrap().build();
    let reader = EntryReader::parse(&encoded).unwrap();
    assert_eq!(reader.get_string(0x07, 64).unwrap().as_deref(), Some(nfc));

    // Hand-craft the NFD wire form: decode must reject it.
    let nfd_bytes = nfd.as_bytes();
    let mut raw = vec![0x07u8];
    raw.extend_from_slice(&(nfd_bytes.len() as u32).to_be_bytes());
    raw.extend_from_slice(nfd_bytes);
    raw.push(0xFF);
    let reader = EntryReader::parse(&raw).unwrap();
    assert_eq!(
        reader.get_string(0x07, 64).err(),
        Some(TlvError::NonNfcString)
    );
}

/// Trailing bytes, missing terminator, truncation, reserved tags, unknown
/// tags, over-length names: all rejected.
#[test]
fn rg10_malformed_inputs_rejected() {
    let good = genesis_entry().encode_tlv(true).unwrap();

    let mut trailing = good.clone();
    trailing.push(0x00);
    assert!(RegistryEntry::decode_tlv(&trailing).is_err());

    let missing_term = &good[..good.len() - 1];
    assert_eq!(
        EntryReader::parse(missing_term).err(),
        Some(TlvError::MissingTerminator)
    );

    let truncated = &good[..10];
    assert!(EntryReader::parse(truncated).is_err());

    // reserved tag 0x00 as a field
    let raw = vec![0x00u8, 0, 0, 0, 1, 0x02, 0xFF];
    assert_eq!(EntryReader::parse(&raw).err(), Some(TlvError::ReservedTag));

    // unknown tag in a registry entry
    let raw = EntryBuilder::new()
        .field_bytes(0x77, b"nope")
        .unwrap()
        .build();
    assert!(RegistryEntry::decode_tlv(&raw).is_err());

    // over-length device name (§4.3: ≤ 64 chars)
    let long = "x".repeat(65);
    let raw = EntryBuilder::new()
        .field_string(0x07, &long)
        .unwrap()
        .build();
    assert_eq!(
        EntryReader::parse(&raw).and_then(|r| r
            .get_string(0x07, registry::DEVICE_NAME_MAX_CHARS)
            .map(|_| ())),
        Err(TlvError::StringTooLong)
    );
}

/// Presence rules: a recovery_epoch carrying a signature, or an enroll
/// missing one, is rejected regardless of TLV well-formedness (§4.3).
#[test]
fn rg10_presence_rules_per_kind() {
    let mut entry = genesis_entry();
    entry.signature = None;
    assert!(entry.encode_tlv(true).is_err(), "genesis without signature");

    let entry = genesis_entry();
    assert!(entry.encode_tlv(true).is_ok());

    // recovery_epoch presence set is exercised by the XV-TLV vectors and
    // the recovery-proof tests; here assert the split both ways:
    let mut bad = genesis_entry();
    bad.kind = EntryKind::RecoveryEpoch;
    assert!(
        bad.encode_tlv(true).is_err(),
        "recovery_epoch needs its fields"
    );
}

/// 33-byte or off-curve keys invalidate the entry (§4.4 rule 8, RG-12's
/// codec half; full chain semantics land with Phase E).
#[test]
fn rg10_key_length_and_curve_rules() {
    let mut entry = genesis_entry();
    entry.sign_pub = Some([0x04; 65]); // right length, off-curve (0,0)-ish
    assert!(entry.encode_tlv(true).is_err());
    let mut entry = genesis_entry();
    entry.sign_pub = None;
    assert!(entry.encode_tlv(true).is_err(), "genesis requires sign_pub");
}
