//! Phase E0 PoC (spec §2.12 Path A): Rust RFC 9180 HPKE ↔ Apple CryptoKit
//! HPKE with a Secure-Enclave-resident P-256 agreement key.
//!
//! Suite: DHKEM(P-256, HKDF-SHA256) / HKDF-SHA256 / ChaCha20-Poly1305
//! (KEM 0x0010, KDF 0x0001, AEAD 0x0003). Public keys are the 65-byte
//! uncompressed X9.63 encoding. Synthetic data only; no vault code
//! depends on this crate.

use std::ffi::CString;

pub type Kem = hpke::kem::DhP256HkdfSha256;
pub type Kdf = hpke::kdf::HkdfSha256;
pub type Aead = hpke::aead::ChaCha20Poly1305;

/// RFC 9180 registry ids for the suite under test.
pub const KEM_ID: u16 = 0x0010;
pub const KDF_ID: u16 = 0x0001;
pub const AEAD_ID: u16 = 0x0003;

#[link(name = "VaultAppleCrypto", kind = "static")]
extern "C" {
    pub fn ov0_se_key_create(tag: *const i8, out: *mut u8, out_len: *mut usize) -> i32;
    pub fn ov0_se_key_public(tag: *const i8, out: *mut u8, out_len: *mut usize) -> i32;
    pub fn ov0_se_key_delete(tag: *const i8) -> i32;
    pub fn ov0_se_key_blob(tag: *const i8, out: *mut u8, cap: usize, out_len: *mut usize) -> i32;
    #[allow(clippy::too_many_arguments)]
    pub fn ov0_hpke_seal(
        pub65: *const u8, pub_len: usize,
        info: *const u8, info_len: usize,
        pt: *const u8, pt_len: usize,
        aad: *const u8, aad_len: usize,
        out_enc: *mut u8, out_enc_len: *mut usize,
        out_ct: *mut u8, ct_cap: usize, out_ct_len: *mut usize,
    ) -> i32;
    #[allow(clippy::too_many_arguments)]
    pub fn ov0_hpke_open_se(
        tag: *const i8,
        info: *const u8, info_len: usize,
        enc: *const u8, enc_len: usize,
        ct: *const u8, ct_len: usize,
        aad: *const u8, aad_len: usize,
        out_pt: *mut u8, pt_cap: usize, out_pt_len: *mut usize,
    ) -> i32;
    #[allow(clippy::too_many_arguments)]
    pub fn ov0_hpke_open_sw(
        sk: *const u8, sk_len: usize,
        info: *const u8, info_len: usize,
        enc: *const u8, enc_len: usize,
        ct: *const u8, ct_len: usize,
        aad: *const u8, aad_len: usize,
        out_pt: *mut u8, pt_cap: usize, out_pt_len: *mut usize,
    ) -> i32;
}

fn tag(name: &str) -> CString {
    CString::new(name).expect("tag has no NUL")
}

/// Create (or replace) an SE key under `name`; returns its 65-byte key.
pub fn se_create(name: &str) -> Result<[u8; 65], i32> {
    let mut out = [0u8; 65];
    let mut len = 0usize;
    // SAFETY: fixed 65-byte buffer, as the bridge requires.
    let rc = unsafe { ov0_se_key_create(tag(name).as_ptr(), out.as_mut_ptr(), &mut len) };
    if rc == 0 && len == 65 { Ok(out) } else { Err(rc) }
}

pub fn se_public(name: &str) -> Result<[u8; 65], i32> {
    let mut out = [0u8; 65];
    let mut len = 0usize;
    // SAFETY: as above.
    let rc = unsafe { ov0_se_key_public(tag(name).as_ptr(), out.as_mut_ptr(), &mut len) };
    if rc == 0 && len == 65 { Ok(out) } else { Err(rc) }
}

pub fn se_delete(name: &str) {
    // SAFETY: valid C string; the bridge ignores a missing item.
    unsafe { ov0_se_key_delete(tag(name).as_ptr()) };
}

/// The stored form of the SE private key (an opaque device-bound blob).
pub fn se_blob(name: &str) -> Result<Vec<u8>, i32> {
    let mut out = vec![0u8; 1024];
    let mut len = 0usize;
    // SAFETY: cap matches the allocation.
    let rc = unsafe { ov0_se_key_blob(tag(name).as_ptr(), out.as_mut_ptr(), out.len(), &mut len) };
    if rc == 0 { out.truncate(len); Ok(out) } else { Err(rc) }
}

/// CryptoKit seal to a 65-byte recipient key → (enc, ciphertext).
pub fn ck_seal(pub65: &[u8; 65], info: &[u8], pt: &[u8], aad: &[u8]) -> Result<(Vec<u8>, Vec<u8>), i32> {
    let mut enc = [0u8; 65];
    let mut enc_len = 0usize;
    let mut ct = vec![0u8; pt.len() + 64];
    let mut ct_len = 0usize;
    // SAFETY: every pointer/length pair below describes a live buffer.
    let rc = unsafe {
        ov0_hpke_seal(
            pub65.as_ptr(), 65,
            info.as_ptr(), info.len(),
            pt.as_ptr(), pt.len(),
            aad.as_ptr(), aad.len(),
            enc.as_mut_ptr(), &mut enc_len,
            ct.as_mut_ptr(), ct.len(), &mut ct_len,
        )
    };
    if rc != 0 { return Err(rc); }
    ct.truncate(ct_len);
    Ok((enc[..enc_len].to_vec(), ct))
}

/// CryptoKit open with the SE key at `name` (DH inside the Enclave).
pub fn ck_open_se(name: &str, info: &[u8], enc: &[u8], ct: &[u8], aad: &[u8]) -> Result<Vec<u8>, i32> {
    let mut pt = vec![0u8; ct.len() + 64];
    let mut pt_len = 0usize;
    // SAFETY: as above.
    let rc = unsafe {
        ov0_hpke_open_se(
            tag(name).as_ptr(),
            info.as_ptr(), info.len(),
            enc.as_ptr(), enc.len(),
            ct.as_ptr(), ct.len(),
            aad.as_ptr(), aad.len(),
            pt.as_mut_ptr(), pt.len(), &mut pt_len,
        )
    };
    if rc != 0 { return Err(rc); }
    pt.truncate(pt_len);
    Ok(pt)
}

/// CryptoKit open with a software key (vector generation only).
pub fn ck_open_sw(sk: &[u8], info: &[u8], enc: &[u8], ct: &[u8], aad: &[u8]) -> Result<Vec<u8>, i32> {
    let mut pt = vec![0u8; ct.len() + 64];
    let mut pt_len = 0usize;
    // SAFETY: as above.
    let rc = unsafe {
        ov0_hpke_open_sw(
            sk.as_ptr(), sk.len(),
            info.as_ptr(), info.len(),
            enc.as_ptr(), enc.len(),
            ct.as_ptr(), ct.len(),
            aad.as_ptr(), aad.len(),
            pt.as_mut_ptr(), pt.len(), &mut pt_len,
        )
    };
    if rc != 0 { return Err(rc); }
    pt.truncate(pt_len);
    Ok(pt)
}
