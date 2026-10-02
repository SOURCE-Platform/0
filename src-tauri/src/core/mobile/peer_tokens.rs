//! Peer-sync tokens (spec v0.5 §22.8, wire annex A.2.1/A.4): their own
//! store, separate from SOURCE Mobile's `mobile_devices`. A token is 32
//! bytes from the OS RNG (base64url), issued at enrollment for one vault
//! registry `device_id`; the Mac keeps only SHA-256(token) and compares
//! hashes. It survives that device's revocation (that is how a revoked
//! phone still learns its status) and is removed only by "Forget this
//! device" or a re-enrollment.

use std::sync::{Arc, OnceLock};

use base64::Engine;
use sha2::{Digest, Sha256};

use crate::core::database::Database;

pub struct PeerTokens {
    db: Arc<Database>,
}

static TOKENS: OnceLock<Arc<PeerTokens>> = OnceLock::new();
static SPKI: OnceLock<String> = OnceLock::new();

pub fn install(db: Arc<Database>, spki_sha256: String) {
    let _ = TOKENS.set(Arc::new(PeerTokens { db }));
    let _ = SPKI.set(spki_sha256);
}

pub fn shared() -> Option<Arc<PeerTokens>> {
    TOKENS.get().cloned()
}

/// SHA-256 over the SPKI of the mobile server's long-lived TLS key.
pub fn spki_sha256() -> Option<String> {
    SPKI.get().cloned()
}

fn hash(token: &str) -> String {
    vault_proto::crypto::hex::encode(Sha256::digest(token.as_bytes()))
}

impl PeerTokens {
    /// Mint (or replace) the token for a registry device id.
    pub async fn issue(&self, device_id: &str) -> Result<String, String> {
        let mut raw = [0u8; 32];
        rand::fill(&mut raw);
        let token = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(raw);
        sqlx::query(
            "INSERT INTO vault_peer_tokens (device_id, token_hash, created_at) VALUES (?, ?, ?)
             ON CONFLICT(device_id) DO UPDATE SET token_hash = excluded.token_hash, created_at = excluded.created_at",
        )
        .bind(device_id)
        .bind(hash(&token))
        .bind(chrono::Utc::now().timestamp_millis())
        .execute(self.db.pool())
        .await
        .map_err(|e| format!("Failed to store peer token: {e}"))?;
        Ok(token)
    }

    /// The registry device id a token was issued to.
    pub async fn verify(&self, token: &str) -> Option<String> {
        let row: Option<(String,)> = sqlx::query_as("SELECT device_id FROM vault_peer_tokens WHERE token_hash = ?")
            .bind(hash(token))
            .fetch_optional(self.db.pool())
            .await
            .ok()?;
        row.map(|r| r.0)
    }

    pub async fn forget(&self, device_id: &str) -> Result<(), String> {
        sqlx::query("DELETE FROM vault_peer_tokens WHERE device_id = ?")
            .bind(device_id)
            .execute(self.db.pool())
            .await
            .map(|_| ())
            .map_err(|e| format!("Failed to forget peer token: {e}"))
    }
}

/// SPKI hash of a PEM private key's public half (the pin a SOURCE Vault
/// phone stores; it survives certificate renewal with the same key).
pub fn spki_of_key_pem(key_pem: &[u8]) -> Result<String, String> {
    let text = std::str::from_utf8(key_pem).map_err(|_| "mobile key is not UTF-8".to_string())?;
    let kp = rcgen::KeyPair::from_pem(text).map_err(|e| format!("mobile key: {e}"))?;
    // "PUBLIC KEY" PEM is exactly the SubjectPublicKeyInfo DER.
    let pem = kp.public_key_pem();
    let b64: String = pem.lines().filter(|l| !l.starts_with("-----")).collect();
    let der = base64::engine::general_purpose::STANDARD.decode(b64).map_err(|_| "mobile key SPKI".to_string())?;
    Ok(vault_proto::crypto::hex::encode(Sha256::digest(der)))
}
