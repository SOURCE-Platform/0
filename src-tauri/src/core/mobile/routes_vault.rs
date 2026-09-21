//! The vault's only route on the paired mobile channel: a read-only
//! registry status refresh (§4.7, Phase E).
//!
//! §5 deliberately kept vault traffic off this always-on server, and
//! this is the single, narrow exception: an enrolled phone otherwise has
//! no way to discover that it was revoked, because revocation is a local
//! registry write on the Mac and the enrollment channel is long gone.
//!
//! What crosses it is the signed registry and the vault id — public
//! verification state the phone checks for itself. What never crosses
//! it: vault records, the VK, wraps, recovery material, backup
//! credentials. There is no write path here, and this is not sync.

use std::collections::HashMap;

use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::Json;
use serde::Serialize;
use serde_json::Value;

use super::server::{bearer_device, MobileState};

type Rejection = (StatusCode, String);

#[derive(Serialize)]
pub struct RegistryStatusResponse {
    pub vault_id: String,
    /// JSONL of canonical TLV entries (§4.1), verified by the phone.
    pub registry: String,
    pub entries: usize,
}

/// `GET /v1/vault/registry` — authenticated by the same paired bearer
/// token as every other route on this server.
pub async fn registry_status(
    State(state): State<MobileState>,
    headers: HeaderMap,
    Query(query): Query<HashMap<String, String>>,
) -> Result<Json<RegistryStatusResponse>, Rejection> {
    let Some(device) = bearer_device(&state, &headers, &query).await else {
        return Err((StatusCode::UNAUTHORIZED, "Unauthorized.".to_string()));
    };
    state.pairing.touch(&device.device_id).await;

    let response = call_helper().await?;
    Ok(Json(RegistryStatusResponse {
        vault_id: field(&response, "vault_id")?,
        registry: field(&response, "registry")?,
        entries: response
            .get("entries")
            .and_then(Value::as_u64)
            .unwrap_or_default() as usize,
    }))
}

#[cfg(target_os = "macos")]
async fn call_helper() -> Result<Value, Rejection> {
    let frame = serde_json::json!({"op": "registry_status"});
    let response = tauri::async_runtime::spawn_blocking(move || {
        crate::core::vault_client::request(frame)
    })
    .await
    .map_err(|_| (StatusCode::SERVICE_UNAVAILABLE, "UNAVAILABLE".to_string()))?
    // A helper that is down, or a vault that does not exist yet, is
    // "cannot answer right now" — never "you are revoked".
    .map_err(|_| (StatusCode::SERVICE_UNAVAILABLE, "UNAVAILABLE".to_string()))?;
    if response.get("ok").and_then(Value::as_bool) != Some(true) {
        // Distinguish "this helper is too old to answer" from "there is
        // no vault right now". Collapsing them produces a phone that
        // reports something the user can see is false.
        return Err(match response.get("error").and_then(Value::as_str) {
            Some("UNKNOWN_OP") => (StatusCode::NOT_IMPLEMENTED, "UNSUPPORTED".to_string()),
            Some("BAD_STATE") => (StatusCode::SERVICE_UNAVAILABLE, "NO_VAULT".to_string()),
            Some(code) => (StatusCode::SERVICE_UNAVAILABLE, code.to_string()),
            None => (StatusCode::SERVICE_UNAVAILABLE, "UNAVAILABLE".to_string()),
        });
    }
    Ok(response)
}

#[cfg(not(target_os = "macos"))]
async fn call_helper() -> Result<Value, Rejection> {
    Err((StatusCode::SERVICE_UNAVAILABLE, "Vault unavailable.".to_string()))
}

fn field(response: &Value, name: &str) -> Result<String, Rejection> {
    response
        .get(name)
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| (StatusCode::SERVICE_UNAVAILABLE, "Vault unavailable.".to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The response type is the contract. If a field is ever added that
    /// carries vault contents or key material, this fails.
    #[test]
    fn response_carries_only_public_verification_state() {
        let json = serde_json::to_value(RegistryStatusResponse {
            vault_id: "00".repeat(16),
            registry: String::new(),
            entries: 0,
        })
        .expect("serializes");
        let mut fields: Vec<&str> = json.as_object().unwrap().keys().map(String::as_str).collect();
        fields.sort_unstable();
        assert_eq!(fields, ["entries", "registry", "vault_id"]);
        for forbidden in ["vk", "wrap", "cred", "secret", "record", "recovery", "password"] {
            assert!(
                !fields.iter().any(|f| f.contains(forbidden)),
                "vault status route must not expose {forbidden}"
            );
        }
    }
}
