use crate::app::state::AppState;
use crate::core::mobile::PhonePromptSetting;
use tauri::{AppHandle, Manager, State};

/// Build the QR code the phone scans. Contains this Mac's address, its TLS
/// fingerprint, and a single-use secret, so the phone can pin the certificate
/// before it opens a connection.
#[tauri::command]
pub async fn mobile_pairing_qr() -> Result<String, String> {
    let (Some(enrollment), Some(fingerprint)) = (
        crate::core::mobile::enrollment(),
        crate::core::mobile::mobile_fingerprint(),
    ) else {
        return Err("Mobile server is not running.".to_string());
    };
    let port = crate::core::mobile::mobile_port();
    if port == 0 {
        return Err("Mobile server is not running.".to_string());
    }
    let payload = crate::core::mobile::EnrollmentPayload {
        v: 1,
        host: crate::core::mobile::local_hostname(),
        port,
        fp: fingerprint,
        secret: enrollment.issue().await,
        name: hostname::get()
            .ok()
            .and_then(|name| name.into_string().ok())
            .unwrap_or_else(|| "Source".to_string()),
    };
    crate::core::mobile::render_enrollment_qr(&payload)
}

/// Invalidate the on-screen QR (called when the Settings tab closes).
#[tauri::command]
pub async fn mobile_clear_pairing_qr() -> Result<(), String> {
    if let Some(enrollment) = crate::core::mobile::enrollment() {
        enrollment.clear().await;
    }
    Ok(())
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PairRequestDto {
    pub request_id: String,
    pub device_name: String,
    pub sas: String,
}

/// Pairing requests waiting for the user to click Allow.
#[tauri::command]
pub async fn mobile_pending_pair_requests() -> Result<Vec<PairRequestDto>, String> {
    let Some(requests) = crate::core::mobile::pair_requests() else {
        return Ok(Vec::new());
    };
    let sas = crate::core::mobile::mobile_fingerprint()
        .map(|fingerprint| crate::core::mobile::short_auth_string(&fingerprint))
        .unwrap_or_default();
    Ok(requests
        .list_pending()
        .await
        .into_iter()
        .map(|pending| PairRequestDto {
            request_id: pending.request_id,
            device_name: pending.device_name,
            sas: sas.clone(),
        })
        .collect())
}

/// Approve a request: mint the token and hand it to the waiting phone.
#[tauri::command]
pub async fn mobile_approve_pair(request_id: String) -> Result<(), String> {
    let (Some(requests), Some(manager)) = (
        crate::core::mobile::pair_requests(),
        crate::core::mobile::pairing_manager(),
    ) else {
        return Err("Mobile server is not running.".to_string());
    };
    let Some((device_id, device_name)) = requests.device_for(&request_id).await else {
        return Err("That pairing request expired.".to_string());
    };
    let (token, device_id) = manager.issue_token(&device_id, &device_name).await?;
    if !requests.approve(&request_id, token, device_id).await {
        return Err("That pairing request expired.".to_string());
    }
    Ok(())
}

#[tauri::command]
pub async fn mobile_deny_pair(request_id: String) -> Result<(), String> {
    let Some(requests) = crate::core::mobile::pair_requests() else {
        return Err("Mobile server is not running.".to_string());
    };
    requests.deny(&request_id).await;
    Ok(())
}

#[tauri::command]
pub async fn mobile_tls_fingerprint() -> Result<String, String> {
    crate::core::mobile::mobile_fingerprint()
        .ok_or_else(|| "Mobile server is not running.".to_string())
}

#[tauri::command]
pub async fn mobile_server_port() -> Result<u16, String> {
    let port = crate::core::mobile::mobile_port();
    if port == 0 {
        return Err("Mobile server is not running.".to_string());
    }
    Ok(port)
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MobileDeviceDto {
    pub device_id: String,
    pub device_name: String,
    pub last_seen_at: i64,
}

#[tauri::command]
pub async fn mobile_list_devices(
    state: State<'_, AppState>,
) -> Result<Vec<MobileDeviceDto>, String> {
    let Some(manager) = crate::core::mobile::pairing_manager() else {
        return Ok(Vec::new());
    };
    let rows = manager.list_devices().await?;
    Ok(rows
        .into_iter()
        .map(|(device_id, device_name, last_seen_at)| MobileDeviceDto {
            device_id,
            device_name,
            last_seen_at,
        })
        .collect())
}

/// Switch "Let the phone send prompts to coding agents". Unlike most of
/// Settings this saves at once, touching only this setting, and tells
/// connected phones straight away.
#[tauri::command]
pub fn set_mobile_agent_prompts_enabled(
    enabled: bool,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<(), String> {
    {
        let mut current = state.config.lock().map_err(|e| format!("Failed to lock config: {e}"))?;
        let mut next = current.clone();
        next.mobile_agent_prompts_enabled = enabled;
        next.save().map_err(|e| format!("Failed to save configuration: {e}"))?;
        *current = next;
    }
    announce_phone_prompts(&app, enabled);
    Ok(())
}

/// Tell connected phones whether they may send prompts, if that changed.
pub fn announce_phone_prompts(app: &AppHandle, enabled: bool) {
    if let Some(setting) = app.try_state::<PhonePromptSetting>() {
        setting.announce(enabled);
    }
}

#[tauri::command]
pub async fn mobile_unpair_device(device_id: String) -> Result<(), String> {
    let Some(manager) = crate::core::mobile::pairing_manager() else {
        return Err("Mobile server is not running.".to_string());
    };
    manager.unpair(&device_id).await
}

#[tauri::command]
pub async fn debug_mobile_transcribe(
    state: State<'_, AppState>,
    path: String,
    clip_id: Option<String>,
) -> Result<String, String> {
    use crate::core::multimodal::SupervisorCommand;
    let id = clip_id.unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let now_ms = chrono::Utc::now().timestamp_millis();
    let sender = state.dictation_commands.lock().await.clone();
    let Some(sender) = sender else {
        return Err("Dictation helper is not running.".to_string());
    };
    sender
        .send(SupervisorCommand::TranscribeFile {
            id: id.clone(),
            path,
            source: crate::core::multimodal::MOBILE_SOURCE_ID.to_string(),
            started_at_ms: now_ms,
            ended_at_ms: now_ms,
        })
        .await
        .map_err(|_| "Failed to dispatch transcription.".to_string())?;
    Ok(id)
}
