use sha2::{Digest, Sha256};
use std::path::PathBuf;

/// Ensure a self-signed cert exists for the mobile HTTPS server.
/// Returns (cert_pem, key_pem, fingerprint_hex).
/// Fingerprint is SHA-256 over the DER certificate (TOFU pinning).
pub fn ensure_mobile_cert(data_dir: &std::path::Path) -> Result<(Vec<u8>, Vec<u8>, String), String> {
    let dir = data_dir.join("mobile");
    std::fs::create_dir_all(&dir).map_err(|error| format!("Failed to create mobile dir: {error}"))?;
    let cert_path = dir.join("cert.pem");
    let key_path = dir.join("key.pem");
    if cert_path.exists() && key_path.exists() {
        let cert_pem = std::fs::read(&cert_path)
            .map_err(|error| format!("Failed to read mobile cert: {error}"))?;
        let key_pem = std::fs::read(&key_path)
            .map_err(|error| format!("Failed to read mobile key: {error}"))?;
        let fingerprint = fingerprint_pem(&cert_pem)?;
        return Ok((cert_pem, key_pem, fingerprint));
    }
    let (cert_pem, key_pem) = generate_self_signed()?;
    std::fs::write(&cert_path, &cert_pem)
        .map_err(|error| format!("Failed to write mobile cert: {error}"))?;
    std::fs::write(&key_path, &key_pem)
        .map_err(|error| format!("Failed to write mobile key: {error}"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&key_path, std::fs::Permissions::from_mode(0o600));
    }
    let fingerprint = fingerprint_pem(&cert_pem)?;
    Ok((cert_pem, key_pem, fingerprint))
}

fn generate_self_signed() -> Result<(Vec<u8>, Vec<u8>), String> {
    let certified = rcgen::generate_simple_self_signed(vec!["source.local".to_string()])
        .map_err(|error| format!("Failed to generate cert: {error}"))?;
    let cert_pem = certified.cert.pem().into_bytes();
    let key_pem = certified.signing_key.serialize_pem().into_bytes();
    Ok((cert_pem, key_pem))
}

fn fingerprint_pem(cert_pem: &[u8]) -> Result<String, String> {
    let pem_str =
        std::str::from_utf8(cert_pem).map_err(|_| "Mobile cert is not UTF-8.".to_string())?;
    let der = pem_to_der(pem_str).ok_or("Failed to parse mobile cert PEM.".to_string())?;
    let mut hasher = Sha256::new();
    hasher.update(&der);
    Ok(hex_digest(hasher.finalize()))
}

fn pem_to_der(pem: &str) -> Option<Vec<u8>> {
    let start = pem.find("-----BEGIN CERTIFICATE-----")?;
    let end = pem.find("-----END CERTIFICATE-----")?;
    let body = &pem[start + "-----BEGIN CERTIFICATE-----".len()..end];
    let cleaned: String = body.chars().filter(|c| !c.is_whitespace()).collect();
    base64::Engine::decode(&base64::engine::general_purpose::STANDARD, cleaned).ok()
}

fn hex_digest(bytes: impl AsRef<[u8]>) -> String {
    bytes
        .as_ref()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

pub fn mobile_data_dir() -> Option<PathBuf> {
    crate::platform::get_platform().get_data_directory().ok().map(|dir| dir.join("mobile"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fingerprint_is_hex_64() {
        let (cert, _key) = generate_self_signed().expect("generate");
        let fp = fingerprint_pem(&cert).expect("fingerprint");
        assert_eq!(fp.len(), 64);
    }
}
