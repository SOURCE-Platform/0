//! Secure Enclave keys and CryptoKit HPKE through the §2.12 bridge.
//!
//! Two SE-resident P-256 keys per device, by role (§2.7): a **signing**
//! key (registry entries, enrollment ACK) and an **agreement** key (HPKE
//! device envelopes). Neither private key can leave the Enclave; the
//! helper only ever holds the 65-byte public keys and opaque references.
//! Path A (proven in the Phase E0 PoC) does the HPKE: the decapsulation
//! DH happens inside the Enclave.

use crate::errors::ErrorCode;

// Linked by `build.rs` (static Swift library + OS Swift runtime).
extern "C" {
    fn ov0_se_key_create(tag: *const i8, out: *mut u8, out_len: *mut usize) -> i32;
    fn ov0_se_key_public(tag: *const i8, out: *mut u8, out_len: *mut usize) -> i32;
    fn ov0_se_key_delete(tag: *const i8) -> i32;
    fn ov0_se_sign_create(tag: *const i8, out: *mut u8, out_len: *mut usize) -> i32;
    fn ov0_se_sign_public(tag: *const i8, out: *mut u8, out_len: *mut usize) -> i32;
    fn ov0_se_sign_digest(tag: *const i8, digest: *const u8, digest_len: usize, out: *mut u8, out_len: *mut usize) -> i32;
    #[allow(clippy::too_many_arguments)]
    fn ov0_hpke_seal(
        pub65: *const u8, pub_len: usize,
        info: *const u8, info_len: usize,
        pt: *const u8, pt_len: usize,
        aad: *const u8, aad_len: usize,
        out_enc: *mut u8, out_enc_len: *mut usize,
        out_ct: *mut u8, ct_cap: usize, out_ct_len: *mut usize,
    ) -> i32;
    #[allow(clippy::too_many_arguments)]
    fn ov0_hpke_open_se(
        tag: *const i8,
        info: *const u8, info_len: usize,
        enc: *const u8, enc_len: usize,
        ct: *const u8, ct_len: usize,
        aad: *const u8, aad_len: usize,
        out_pt: *mut u8, pt_cap: usize, out_pt_len: *mut usize,
    ) -> i32;
}

/// Bridge error codes are opaque to callers: they carry no key material.
fn map(rc: i32) -> ErrorCode {
    match rc {
        -1 => ErrorCode::InvalidInput,
        -2 | -3 => ErrorCode::DeviceNotAuthorized,
        _ => ErrorCode::Internal,
    }
}

fn c_tag(tag: &str) -> Result<std::ffi::CString, ErrorCode> {
    std::ffi::CString::new(tag).map_err(|_| ErrorCode::InvalidInput)
}

fn pubkey_call(
    tag: &str,
    f: unsafe extern "C" fn(*const i8, *mut u8, *mut usize) -> i32,
) -> Result<[u8; 65], ErrorCode> {
    let tag = c_tag(tag)?;
    let mut out = [0u8; 65];
    let mut len = 0usize;
    // SAFETY: fixed 65-byte buffer; the bridge writes at most that.
    let rc = unsafe { f(tag.as_ptr(), out.as_mut_ptr(), &mut len) };
    if rc == 0 && len == 65 && out[0] == 0x04 {
        Ok(out)
    } else {
        Err(map(rc))
    }
}

/// Create (replacing any existing) the SE **agreement** key for `tag`.
pub fn create_agreement_key(tag: &str) -> Result<[u8; 65], ErrorCode> {
    pubkey_call(tag, ov0_se_key_create)
}

pub fn agreement_public(tag: &str) -> Result<[u8; 65], ErrorCode> {
    pubkey_call(tag, ov0_se_key_public)
}

/// Create (replacing any existing) the SE **signing** key for `tag`.
pub fn create_signing_key(tag: &str) -> Result<[u8; 65], ErrorCode> {
    pubkey_call(tag, ov0_se_sign_create)
}

pub fn signing_public(tag: &str) -> Result<[u8; 65], ErrorCode> {
    pubkey_call(tag, ov0_se_sign_public)
}

/// Both keys for `tag` are destroyed (device reset / failed enrollment).
pub fn delete_keys(tag: &str) {
    if let Ok(tag) = c_tag(tag) {
        // SAFETY: valid C string; missing items are ignored by the bridge.
        unsafe { ov0_se_key_delete(tag.as_ptr()) };
    }
}

/// Raw ECDSA signature (r‖s) over a 32-byte digest, from the SE key.
pub fn sign_digest(tag: &str, digest: &[u8; 32]) -> Result<[u8; 64], ErrorCode> {
    let tag = c_tag(tag)?;
    let mut out = [0u8; 64];
    let mut len = 0usize;
    // SAFETY: 32-byte digest in, 64-byte buffer out, as the bridge expects.
    let rc = unsafe { ov0_se_sign_digest(tag.as_ptr(), digest.as_ptr(), 32, out.as_mut_ptr(), &mut len) };
    if rc == 0 && len == 64 { Ok(out) } else { Err(map(rc)) }
}

/// HPKE seal to a 65-byte recipient key (CryptoKit `HPKE.Sender`).
pub fn hpke_seal(recipient: &[u8; 65], info: &[u8], plaintext: &[u8]) -> Result<(Vec<u8>, Vec<u8>), ErrorCode> {
    let mut enc = [0u8; 65];
    let mut enc_len = 0usize;
    let mut ct = vec![0u8; plaintext.len() + 64];
    let mut ct_len = 0usize;
    // SAFETY: every pointer/length pair describes a live buffer.
    let rc = unsafe {
        ov0_hpke_seal(
            recipient.as_ptr(), 65,
            info.as_ptr(), info.len(),
            plaintext.as_ptr(), plaintext.len(),
            [].as_ptr(), 0,
            enc.as_mut_ptr(), &mut enc_len,
            ct.as_mut_ptr(), ct.len(), &mut ct_len,
        )
    };
    if rc != 0 || enc_len != 65 {
        return Err(map(rc));
    }
    ct.truncate(ct_len);
    Ok((enc.to_vec(), ct))
}

/// HPKE open with this device's SE agreement key (DH inside the Enclave).
pub fn hpke_open(tag: &str, info: &[u8], enc: &[u8], ct: &[u8]) -> Result<crate::crypto::secret::SecretVec, ErrorCode> {
    let tag = c_tag(tag)?;
    let mut pt = vec![0u8; ct.len() + 64];
    let mut pt_len = 0usize;
    // SAFETY: as above.
    let rc = unsafe {
        ov0_hpke_open_se(
            tag.as_ptr(),
            info.as_ptr(), info.len(),
            enc.as_ptr(), enc.len(),
            ct.as_ptr(), ct.len(),
            [].as_ptr(), 0,
            pt.as_mut_ptr(), pt.len(), &mut pt_len,
        )
    };
    if rc != 0 {
        return Err(ErrorCode::IntegrityFailure);
    }
    pt.truncate(pt_len);
    Ok(crate::crypto::secret::SecretVec::new(pt))
}
