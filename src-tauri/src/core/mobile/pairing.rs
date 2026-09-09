use super::pair_requests::new_token;
use super::types::AuthDevice;
use crate::core::database::Database;
use sha2::{Digest, Sha256};
use std::sync::Arc;

/// Bearer token issuance and paired-device records.
/// The Mac stores only SHA-256(token); the raw token lives in the iOS Keychain.
pub struct PairingManager {
    db: Arc<Database>,
}

impl PairingManager {
    pub fn new(db: Arc<Database>) -> Self {
        Self { db }
    }

    /// Mint a token for an approved device and record it. Returns (token, device_id).
    pub async fn issue_token(
        &self,
        device_id: &str,
        device_name: &str,
    ) -> Result<(String, String), String> {
        let token = new_token();
        let token_hash = hash_token(&token);
        let device_id = if device_id.is_empty() {
            uuid::Uuid::new_v4().to_string()
        } else {
            device_id.to_string()
        };
        let now_ms = chrono::Utc::now().timestamp_millis();
        sqlx::query(
            "INSERT INTO mobile_devices (device_id, device_name, token_hash, last_seen_at, created_at)
             VALUES (?, ?, ?, ?, ?)
             ON CONFLICT(device_id) DO UPDATE SET
                device_name = excluded.device_name,
                token_hash = excluded.token_hash,
                last_seen_at = excluded.last_seen_at",
        )
        .bind(&device_id)
        .bind(device_name)
        .bind(&token_hash)
        .bind(now_ms)
        .bind(now_ms)
        .execute(self.db.pool())
        .await
        .map_err(|error| format!("Failed to store paired device: {error}"))?;
        Ok((token, device_id))
    }

    /// Verify a bearer token, returning the device on success.
    pub async fn verify(&self, token: &str) -> Option<AuthDevice> {
        let token_hash = hash_token(token);
        let row: Option<(String, String)> = sqlx::query_as(
            "SELECT device_id, device_name FROM mobile_devices WHERE token_hash = ?",
        )
        .bind(&token_hash)
        .fetch_optional(self.db.pool())
        .await
        .ok()?;
        row.map(|(device_id, device_name)| AuthDevice {
            device_id,
            device_name,
        })
    }

    pub async fn touch(&self, device_id: &str) {
        let now_ms = chrono::Utc::now().timestamp_millis();
        let _ = sqlx::query("UPDATE mobile_devices SET last_seen_at = ? WHERE device_id = ?")
            .bind(now_ms)
            .bind(device_id)
            .execute(self.db.pool())
            .await;
    }

    pub async fn unpair(&self, device_id: &str) -> Result<(), String> {
        sqlx::query("DELETE FROM mobile_devices WHERE device_id = ?")
            .bind(device_id)
            .execute(self.db.pool())
            .await
            .map_err(|error| format!("Failed to unpair device: {error}"))?;
        Ok(())
    }

    pub async fn list_devices(
        &self,
    ) -> Result<Vec<(String, String, i64)>, String> {
        let rows: Vec<(String, String, i64)> = sqlx::query_as(
            "SELECT device_id, device_name, last_seen_at FROM mobile_devices ORDER BY last_seen_at DESC",
        )
        .fetch_all(self.db.pool())
        .await
        .map_err(|error| format!("Failed to list devices: {error}"))?;
        Ok(rows)
    }
}

pub fn hash_token(token: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(token.as_bytes());
    hex_digest(hasher.finalize())
}

fn hex_digest(bytes: impl AsRef<[u8]>) -> String {
    bytes
        .as_ref()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_hash_is_stable_hex() {
        let first = hash_token("abc123");
        assert_eq!(first, hash_token("abc123"));
        assert_eq!(first.len(), 64);
        assert_ne!(first, hash_token("different"));
    }

    #[test]
    fn tokens_do_not_contain_separator() {
        assert!(!hash_token("x").contains(' '));
    }
}
