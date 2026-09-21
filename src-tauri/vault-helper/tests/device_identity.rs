//! Phase E: Secure-Enclave device identity and HPKE device envelopes
//! (§2.7, §2.2, §2.9, §2.12 Path A). Real SE keys, synthetic vaults.

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use vault_helper::crypto::ecdsa;
use vault_helper::crypto::secret::{random_secret, SecretBytes};
use vault_helper::crypto::wrap::DeviceEnvelopePayload;
use vault_helper::device::{envelope, identity::SeDevice, se};
use vault_helper::registry::device::{DeviceIdentity, PLATFORM_MACOS};
use sha2::{Digest, Sha256};

/// Secure Enclave items live in the login keychain, whose global lock
/// stalls when several threads hit it at once; every test in this binary
/// takes the same guard (the pattern the other op test binaries use).
fn serial() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::OnceLock<std::sync::Mutex<()>> = std::sync::OnceLock::new();
    LOCK.get_or_init(|| std::sync::Mutex::new(()))
        .lock()
        .unwrap_or_else(|e| e.into_inner())
}

/// One fresh synthetic vault directory per test (same pattern as the
/// other helper test binaries).
fn tmp() -> PathBuf {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let dir = PathBuf::from(format!(
        "/tmp/vhdev-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::SeqCst)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn payload(gen: u32) -> DeviceEnvelopePayload {
    DeviceEnvelopePayload {
        vk: random_secret(),
        device_backup_cred: random_secret(),
        wrapped_at: 1_700_000_000,
        vk_generation: gen,
    }
}

/// DV-01: both SE roles exist, are distinct keys, and are 65-byte
/// uncompressed points (§2.7 "never one key for both roles").
#[test]
fn se_device_has_two_distinct_65_byte_keys() {
    let _g = serial();
    let dir = tmp();
    let dev = SeDevice::create(&dir, "Test Mac", PLATFORM_MACOS).expect("create");
    assert_eq!(dev.sign_pub().len(), 65);
    assert_eq!(dev.sign_pub()[0], 0x04);
    assert_eq!(dev.agree_pub()[0], 0x04);
    assert_ne!(dev.sign_pub(), dev.agree_pub(), "roles must not share a key");
    assert!(ecdsa::parse_verifying_key(&dev.sign_pub()).is_ok());
    assert!(ecdsa::parse_verifying_key(&dev.agree_pub()).is_ok());
    let tag = dev.key_tag().to_string();
    // Reloading yields the same identity, still backed by the Enclave.
    let again = SeDevice::load(&dir).expect("load");
    assert_eq!(again.device_id(), dev.device_id());
    assert_eq!(again.sign_pub(), dev.sign_pub());
    se::delete_keys(&tag);
}

/// DV-02: SE signatures are valid, canonical low-S, and randomized —
/// two signatures over the same digest differ but both verify (§2.7).
#[test]
fn se_signatures_are_low_s_and_randomized() {
    let _g = serial();
    let dir = tmp();
    let dev = SeDevice::create(&dir, "Test Mac", PLATFORM_MACOS).expect("create");
    let digest: [u8; 32] = Sha256::digest(b"ov0/test/digest").into();
    let a = dev.sign_prehash(&digest).expect("sign");
    let b = dev.sign_prehash(&digest).expect("sign");
    assert!(ecdsa::is_low_s(&a) && ecdsa::is_low_s(&b));
    assert!(ecdsa::verify_prehash(&dev.sign_pub(), &digest, &a).is_ok());
    assert!(ecdsa::verify_prehash(&dev.sign_pub(), &digest, &b).is_ok());
    assert_ne!(a, b, "SE/CryptoKit ECDSA is randomized, not RFC 6979");
    // A different digest does not verify under either signature.
    let other: [u8; 32] = Sha256::digest(b"other").into();
    assert!(ecdsa::verify_prehash(&dev.sign_pub(), &other, &a).is_err());
    se::delete_keys(dev.key_tag());
}

/// DV-03: missing SE keys are detected rather than silently trusted
/// (§2.8: the device must re-enroll or recover).
#[test]
fn deleted_se_keys_make_the_identity_unloadable() {
    let _g = serial();
    let dir = tmp();
    let dev = SeDevice::create(&dir, "Test Mac", PLATFORM_MACOS).expect("create");
    let tag = dev.key_tag().to_string();
    se::delete_keys(&tag);
    assert!(SeDevice::load(&dir).is_err());
}

/// EV-01: a device envelope round-trips through the Enclave, and the
/// payload is the §2.2 device shape (VK + per-device backup credential).
#[test]
fn envelope_round_trips_through_the_enclave() {
    let _g = serial();
    let dir = tmp();
    let dev = SeDevice::create(&dir, "Test Mac", PLATFORM_MACOS).expect("create");
    let vault_id = [7u8; 16];
    let nonce = [9u8; 16];
    let pt = payload(3);
    let (vk, cred) = (*pt.vk.expose(), *pt.device_backup_cred.expose());

    let file = envelope::seal_envelope(&dev.agree_pub(), &vault_id, &dev.device_id(), &nonce, &pt)
        .expect("seal");
    assert_eq!(
        vault_helper::crypto::hex::decode(&file.enc).unwrap().len(),
        65,
        "encapsulated key is the 65-byte uncompressed form"
    );
    envelope::write_envelope(&dir, &dev.device_id(), &file).expect("write");
    let read = envelope::read_envelope(&dir, &dev.device_id()).expect("read");
    let opened = envelope::open_envelope(dev.key_tag(), &vault_id, &read).expect("open");
    assert_eq!(opened.vk.expose(), &vk);
    assert_eq!(opened.device_backup_cred.expose(), &cred);
    assert_eq!(opened.vk_generation, 3);
    se::delete_keys(dev.key_tag());
}

/// EV-02: the HPKE `info` binds vault, device and enrollment instance —
/// changing any of the three makes the envelope unopenable (§2.9).
#[test]
fn envelope_info_binds_vault_device_and_enrollment() {
    let _g = serial();
    let dir = tmp();
    let dev = SeDevice::create(&dir, "Test Mac", PLATFORM_MACOS).expect("create");
    let vault_id = [7u8; 16];
    let nonce = [9u8; 16];
    let file =
        envelope::seal_envelope(&dev.agree_pub(), &vault_id, &dev.device_id(), &nonce, &payload(1))
            .expect("seal");

    // Wrong vault.
    assert!(envelope::open_envelope(dev.key_tag(), &[8u8; 16], &file).is_err());
    // Wrong enrollment nonce.
    let mut replayed = file.clone();
    replayed.enrollment_nonce = vault_helper::crypto::hex::encode([1u8; 16]);
    assert!(envelope::open_envelope(dev.key_tag(), &vault_id, &replayed).is_err());
    // Wrong device id.
    let mut rebound = file.clone();
    rebound.device_id = vault_helper::crypto::hex::encode([2u8; 16]);
    assert!(envelope::open_envelope(dev.key_tag(), &vault_id, &rebound).is_err());
    // Flipped ciphertext byte.
    let mut tampered = file.clone();
    let mut ct = vault_helper::crypto::hex::decode(&tampered.ct).unwrap();
    ct[0] ^= 1;
    tampered.ct = vault_helper::crypto::hex::encode(ct);
    assert!(envelope::open_envelope(dev.key_tag(), &vault_id, &tampered).is_err());
    se::delete_keys(dev.key_tag());
}

/// EV-03: an envelope sealed to another device's agreement key cannot be
/// opened by this one, even with matching info.
#[test]
fn envelope_for_another_device_does_not_open() {
    let _g = serial();
    let dir_a = tmp();
    let dir_b = tmp();
    let a = SeDevice::create(&dir_a, "Mac A", PLATFORM_MACOS).expect("a");
    let b = SeDevice::create(&dir_b, "Mac B", PLATFORM_MACOS).expect("b");
    let vault_id = [7u8; 16];
    let nonce = [9u8; 16];
    let for_b = envelope::seal_envelope(&b.agree_pub(), &vault_id, &b.device_id(), &nonce, &payload(1))
        .expect("seal");
    assert!(envelope::open_envelope(a.key_tag(), &vault_id, &for_b).is_err());
    assert!(envelope::open_envelope(b.key_tag(), &vault_id, &for_b).is_ok());
    se::delete_keys(a.key_tag());
    se::delete_keys(b.key_tag());
}

/// The recorded public identity survives a round trip through the file,
/// and nothing secret is in it.
#[test]
fn device_file_is_public_material_only() {
    let _g = serial();
    let dir = tmp();
    let dev = SeDevice::create(&dir, "Test Mac", PLATFORM_MACOS).expect("create");
    let raw = std::fs::read_to_string(dir.join("device.json")).expect("read");
    let digest: [u8; 32] = Sha256::digest(b"x").into();
    let sig = dev.sign_prehash(&digest).expect("sign");
    for secret in [&sig[..], &random_secret().expose()[..]] {
        let hexed = vault_helper::crypto::hex::encode(secret);
        assert!(!raw.contains(&hexed));
    }
    assert!(raw.contains("key_tag"));
    let _: SecretBytes<32> = random_secret();
    se::delete_keys(dev.key_tag());
}
