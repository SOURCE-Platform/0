use super::pair_requests::{short_auth_string, PairState};
use super::server::MobileState;
use super::types::{PairPollResponse, PairStartRequest, PairStartResponse};
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::Json;
use std::collections::HashMap;

/// Open a pairing request. The Mac raises an Allow/Deny dialog; nothing is
/// issued until the user approves it there.
pub async fn pair_start(
    State(state): State<MobileState>,
    Json(req): Json<PairStartRequest>,
) -> Result<Json<PairStartResponse>, (StatusCode, String)> {
    if req.device_name.len() > 128 || req.device_id.len() > 128 {
        return Err((StatusCode::BAD_REQUEST, "Device name too long.".to_string()));
    }
    // Cheap early reject: a plain relay that forwards our certificate untouched
    // is caught here. An active attacker can rewrite this field, which is why
    // the comparison tag shown to the user is the real defence.
    if !req.observed_fingerprint.is_empty() && req.observed_fingerprint != state.fingerprint {
        return Err((
            StatusCode::UNAUTHORIZED,
            "Certificate mismatch — refusing to pair.".to_string(),
        ));
    }
    let device_name = if req.device_name.is_empty() {
        "iPhone".to_string()
    } else {
        req.device_name.clone()
    };
    // QR path: the secret proves the phone read this Mac's screen, which is a
    // channel no one on the network can reach. Issue the token straight away.
    if !req.secret.is_empty() {
        if !state.enrollment.redeem(&req.secret).await {
            return Err((
                StatusCode::UNAUTHORIZED,
                "That pairing code is no longer valid — show a fresh one.".to_string(),
            ));
        }
        let (token, device_id) = state
            .pairing
            .issue_token(&req.device_id, &device_name)
            .await
            .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error))?;
        notify_paired(&state, &device_name);
        return Ok(Json(PairStartResponse {
            request_id: String::new(),
            token: Some(token),
            device_id: Some(device_id),
        }));
    }

    let request_id = state
        .pair_requests
        .open(&req.device_id, &device_name)
        .await
        .map_err(|error| (StatusCode::TOO_MANY_REQUESTS, error))?;

    if let Some(handle) = &state.app_handle {
        use tauri::Emitter;
        let _ = handle.emit(
            "mobile-pair-request",
            serde_json::json!({
                "requestId": request_id,
                "deviceName": device_name,
                "sas": short_auth_string(&state.fingerprint),
            }),
        );
    }
    Ok(Json(PairStartResponse {
        request_id,
        token: None,
        device_id: None,
    }))
}

/// Tell the Settings screen a phone just paired, so it can refresh and dismiss
/// the QR code.
fn notify_paired(state: &MobileState, device_name: &str) {
    if let Some(handle) = &state.app_handle {
        use tauri::Emitter;
        let _ = handle.emit(
            "mobile-paired",
            serde_json::json!({ "deviceName": device_name }),
        );
    }
}

/// Poll a pending request. Approved results are handed out exactly once.
pub async fn pair_poll(
    State(state): State<MobileState>,
    Query(query): Query<HashMap<String, String>>,
) -> Result<Json<PairPollResponse>, (StatusCode, String)> {
    let Some(request_id) = query.get("request_id") else {
        return Err((StatusCode::BAD_REQUEST, "Missing request_id.".to_string()));
    };
    let response = match state.pair_requests.poll(request_id).await {
        PairState::Pending => PairPollResponse {
            state: "pending".to_string(),
            token: None,
            device_id: None,
        },
        PairState::Approved { token, device_id } => PairPollResponse {
            state: "approved".to_string(),
            token: Some(token),
            device_id: Some(device_id),
        },
        PairState::Denied => PairPollResponse {
            state: "denied".to_string(),
            token: None,
            device_id: None,
        },
        PairState::Expired => PairPollResponse {
            state: "expired".to_string(),
            token: None,
            device_id: None,
        },
    };
    Ok(Json(response))
}
