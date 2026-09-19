//! CR-11 and CR-13 (spec §16.1): zeroization and derivation hygiene.
//! Synthetic data only.

mod common;

use common::*;
use vault_helper::crypto::hkdf;
use vault_helper::crypto::secret::{random_secret, SecretBytes, SecretVec};
use vault_helper::crypto::{bip39, wrap};

/// CR-11: zeroization spot test — canary secrets must not survive in freed
/// buffers after the lock/drop path. (The mechanism test for SecretBytes
/// lives in crypto::secret::tests::zeroize_on_drop_wipes_backing_buffer;
/// this adds the variable-length plaintext type used for record bodies.)
#[test]
fn cr11_secret_vec_zeroizes_on_drop() {
    let canary = b"OV0-CANARY-RECORD-PLAINTEXT-0123456789".to_vec();
    let ptr;
    {
        // SecretVec discipline: built from a complete buffer and never
        // grown once it holds secrets — a growing realloc would abandon
        // the old heap block unzeroized, because Zeroizing<Vec<u8>> wipes
        // only its live buffer at drop. (Audited: no src/ code grows a
        // SecretVec after filling it.)
        let plaintext = SecretVec::new(canary.clone());
        ptr = plaintext.as_ptr();
        drop(plaintext);
    }
    // SAFETY: read of the just-dropped allocation (still mapped: the
    // allocator keeps small blocks); another test thread may already have
    // reused it, which the fragment check below tolerates. Spot-check only.
    let after = unsafe { std::slice::from_raw_parts(ptr, canary.len()) };
    assert!(
        !after.windows(canary.len()).any(|w| w == canary.as_slice()),
        "canary plaintext survived drop"
    );
    // Stronger than "canary absent": no 8-byte fragment of it survives
    // anywhere in the block. The freed block may legitimately hold other
    // bytes — the allocator's freelist linkage, or data from another test
    // thread that reused it — so "all zero" is not the invariant (that
    // stricter form failed ~1 in 15 runs under the parallel harness).
    for frag in canary.windows(8) {
        assert!(
            !after.windows(8).any(|w| w == frag),
            "fragment of the secret survived drop"
        );
    }
}

/// CR-13: cred_mp / cred_rk derivation follows the §2.9 context strings,
/// and derived secrets never touch the filesystem. The second half scans
/// every file the crypto layer produced for canary bytes — Phase B's
/// crypto core writes nothing by construction, and this test keeps it so.
#[test]
fn cr13_backup_credential_derivation_and_no_persistence() {
    let pk = SecretBytes::new([0x51; 32]);
    let rk = SecretBytes::new([0x52; 32]);
    let vault_salt = [0x53; 16];

    let cred_mp = hkdf::backup_cred_mp(&pk, &vault_salt).unwrap();
    let cred_rk = hkdf::backup_cred_rk(&rk, &vault_salt).unwrap();
    // distinct inputs + distinct info strings → distinct credentials
    assert_ne!(cred_mp.expose(), cred_rk.expose());

    // Determinism (vectors pin the exact bytes; XV-HKDF covers all §2.9
    // strings including ov0/backup-auth/{mp,rk}/v1).
    let cred_mp2 = hkdf::backup_cred_mp(&pk, &vault_salt).unwrap();
    assert_eq!(cred_mp.expose(), cred_mp2.expose());

    // No-persistence scan: produce every Phase B artifact a vault dir
    // would hold (wrap files), then scan for canary secrets.
    let dir = std::env::temp_dir().join(format!("vh-cr13-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let vk = random_secret();
    let payload = wrap::RecoveryWrapPayload {
        vk: clone32(&vk),
        wrapped_at: 1,
        vk_generation: 0,
    };
    let mp_file = wrap::seal_wrap_mp(&payload, &pk, &VAULT_ID, FAST, &KDF_SALT).unwrap();
    let rk_file = wrap::seal_wrap_rk(&payload, &rk, &VAULT_ID).unwrap();
    std::fs::write(
        dir.join("password.wrap"),
        serde_json::to_string(&mp_file).unwrap(),
    )
    .unwrap();
    std::fs::write(
        dir.join("recovery.wrap"),
        serde_json::to_string(&rk_file).unwrap(),
    )
    .unwrap();

    let canaries: Vec<&[u8]> = vec![
        cred_mp.expose(),
        cred_rk.expose(),
        vk.expose(),
        rk.expose(),
        pk.expose(),
    ];
    for entry in std::fs::read_dir(&dir).unwrap() {
        let bytes = std::fs::read(entry.unwrap().path()).unwrap();
        for canary in &canaries {
            assert!(
                !bytes.windows(canary.len()).any(|w| w == *canary),
                "secret canary found on disk"
            );
            let hexed = vault_helper::crypto::hex::encode(canary);
            assert!(!bytes.windows(hexed.len()).any(|w| w == hexed.as_bytes()));
        }
    }
    std::fs::remove_dir_all(&dir).ok();
}

fn clone32(s: &SecretBytes<32>) -> SecretBytes<32> {
    SecretBytes::new(*s.expose())
}

/// CR-13 companion: RK enters via words and the RK bytes are what the
/// derivation consumes (§2.4: entropy directly, no PBKDF2 expansion).
#[test]
fn cr13_rk_derivation_uses_raw_entropy() {
    let rk = SecretBytes::new([0x42; 32]);
    let words = bip39::encode_rk(&rk);
    let decoded = bip39::decode_rk(&words).unwrap();
    assert_eq!(decoded.expose(), rk.expose());
    let cred = hkdf::backup_cred_rk(&decoded, &[0x53; 16]).unwrap();
    let direct = hkdf::backup_cred_rk(&rk, &[0x53; 16]).unwrap();
    assert_eq!(cred.expose(), direct.expose());
}
