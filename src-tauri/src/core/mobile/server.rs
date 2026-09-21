use super::ingest::{append_stream_pcm, finish_stream_clip};
use super::enrollment::Enrollment;
use super::pair_requests::PairRequests;
use super::pairing::PairingManager;
use super::tls::ensure_mobile_cert;
use super::types::{AuthDevice, HealthResponse};
use crate::core::database::Database;
use crate::core::multimodal::SupervisorCommand;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{DefaultBodyLimit, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::Json;
use axum::routing::{get, post};
use axum::Router;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};

#[derive(Clone)]
pub struct MobileState {
    pub db: Arc<Database>,
    pub pairing: Arc<PairingManager>,
    pub pair_requests: Arc<PairRequests>,
    pub enrollment: Arc<Enrollment>,
    pub commands: Arc<Mutex<Option<mpsc::Sender<SupervisorCommand>>>>,
    pub session: Option<Arc<crate::core::session_manager::SessionManager>>,
    pub fingerprint: String,
    pub device_name: String,
    pub app_handle: Option<tauri::AppHandle>,
    /// Present when agent features are running; `/v1/agent` needs them.
    pub agents: Option<super::agent_feed::AgentServices>,
}

pub async fn serve_mobile(
    db: Arc<Database>,
    commands: Arc<Mutex<Option<mpsc::Sender<SupervisorCommand>>>>,
    session: Option<Arc<crate::core::session_manager::SessionManager>>,
    app_handle: Option<tauri::AppHandle>,
    agents: Option<super::agent_feed::AgentServices>,
    enabled: bool,
    preferred_port: u16,
) -> Result<(u16, String), String> {
    if !enabled {
        return Err("Mobile server disabled.".to_string());
    }
    let data_dir = crate::platform::get_platform()
        .get_data_directory()
        .map_err(|error| format!("No data directory: {error}"))?;
    let (cert_pem, key_pem, fingerprint) = ensure_mobile_cert(&data_dir)?;
    let device_name = hostname::get()
        .ok()
        .and_then(|name| name.into_string().ok())
        .unwrap_or_else(|| "Source".to_string());
    let pairing = Arc::new(PairingManager::new(db.clone()));
    let pair_requests = Arc::new(PairRequests::new());
    let enrollment = Arc::new(Enrollment::new());
    crate::core::mobile::set_shared_pairing(
        pairing.clone(),
        pair_requests.clone(),
        enrollment.clone(),
    );
    let state = MobileState {
        db,
        pairing,
        pair_requests,
        enrollment,
        commands,
        session,
        fingerprint: fingerprint.clone(),
        device_name: device_name.clone(),
        app_handle,
        agents,
    };
    let app = Router::new()
        .route("/v1/health", get(health))
        .route("/v1/pair/start", post(super::routes_pair::pair_start))
        .route("/v1/pair/poll", get(super::routes_pair::pair_poll))
        .route(
            "/v1/clips",
            post(super::routes_clips::upload_clip)
                .layer(DefaultBodyLimit::max(super::routes_clips::MAX_CLIP_BYTES)),
        )
        .route("/v1/clips/status", get(super::routes_clips::clip_status))
        // The vault's only route here: read-only registry status (§4.7).
        .route("/v1/vault/registry", get(super::routes_vault::registry_status))
        .route("/v1/stream", get(stream_ws))
        .route("/v1/agent", get(super::agent_socket::agent_ws))
        .with_state(state.clone());

    let config = axum_server::tls_rustls::RustlsConfig::from_pem(cert_pem, key_pem)
        .await
        .map_err(|error| format!("Failed to load TLS cert: {error}"))?;
    for port in preferred_port..preferred_port + 20 {
        let addr = SocketAddr::from(([0, 0, 0, 0], port));
        let server = app.clone();
        let tls = config.clone();
        let bound = tokio::net::TcpListener::bind(addr).await;
        if bound.is_err() {
            continue;
        }
        drop(bound);
        let state_clone = state.clone();
        tokio::spawn(async move {
            let _ = axum_server::bind_rustls(addr, tls)
                .serve(server.into_make_service())
                .await;
        });
        // Advertise after bind attempt (best effort).
        advertise_mdns(port, &state_clone.fingerprint, &state_clone.device_name);
        emit_status(&state_clone, true);
        return Ok((port, fingerprint));
    }
    Err("No free port for mobile server.".to_string())
}

fn advertise_mdns(port: u16, fingerprint: &str, device_name: &str) {
    let fingerprint = fingerprint.to_string();
    let device_name = device_name.to_string();
    tokio::spawn(async move {
        let Ok(mut advertiser) = super::discovery::MobileAdvertiser::new() else {
            return;
        };
        let _ = advertiser.advertise(port, &fingerprint, &device_name);
        futures::future::pending::<()>().await;
    });
}

fn emit_status(state: &MobileState, connected: bool) {
    if let Some(handle) = &state.app_handle {
        use tauri::Emitter;
        let _ = handle.emit(
            "mobile-status",
            serde_json::json!({ "connected": connected }),
        );
    }
}

async fn health(State(state): State<MobileState>) -> Json<HealthResponse> {
    Json(HealthResponse {
        ok: true,
        version: env!("CARGO_PKG_VERSION").to_string(),
        device_name: state.device_name.clone(),
        tls_fingerprint_sha256: state.fingerprint.clone(),
    })
}

pub(super) async fn bearer_device(
    state: &MobileState,
    headers: &HeaderMap,
    query: &HashMap<String, String>,
) -> Option<AuthDevice> {
    if let Some(header) = headers.get("authorization").and_then(|value| value.to_str().ok()) {
        if let Some(token) = header.strip_prefix("Bearer ") {
            if let Some(device) = state.pairing.verify(token).await {
                return Some(device);
            }
        }
    }
    if let Some(token) = query.get("token") {
        if let Some(device) = state.pairing.verify(token).await {
            return Some(device);
        }
    }
    None
}

async fn stream_ws(
    State(state): State<MobileState>,
    headers: HeaderMap,
    Query(query): Query<HashMap<String, String>>,
    ws: WebSocketUpgrade,
) -> Result<axum::response::Response, StatusCode> {
    let Some(device) = bearer_device(&state, &headers, &query).await else {
        return Err(StatusCode::UNAUTHORIZED);
    };
    Ok(ws.on_upgrade(move |socket| handle_stream(socket, state, device)))
}

async fn handle_stream(mut socket: WebSocket, state: MobileState, device: AuthDevice) {
    emit_status(&state, true);
    let mut clip_id: Option<String> = None;
    let mut started_at_ms = chrono::Utc::now().timestamp_millis();
    let session_id = current_session_id(&state).await;
    // Create a live partial row on open so the lane shows a pulsing block.
    while let Some(message) = socket.recv().await {
        let Ok(message) = message else { break };
        match message {
            Message::Text(text) => {
                if let Ok(open) =
                    serde_json::from_str::<super::types::StreamOpen>(&text)
                {
                    match open {
                        super::types::StreamOpen::Open {
                            clip_id: id,
                            started_at_ms: started,
                            ..
                        } => {
                            clip_id = Some(id.clone());
                            started_at_ms = started;
                            let _ = crate::core::multimodal::persist_mobile_transcript(
                                &state.db,
                                &session_id,
                                &id,
                                "",
                                started_at_ms,
                                started_at_ms + 1,
                                None,
                                None,
                                "parakeet-tdt",
                                false,
                            )
                            .await;
                        }
                        super::types::StreamOpen::Close {
                            clip_id: id,
                            ended_at_ms,
                        } => {
                            let commands = state.commands.lock().await.clone();
                            let _ = finish_stream_clip(
                                &state.db,
                                &commands,
                                &session_id,
                                &device,
                                &id,
                                started_at_ms,
                                ended_at_ms,
                            )
                            .await;
                            break;
                        }
                    }
                }
            }
            Message::Binary(pcm) => {
                if let Some(id) = &clip_id {
                    let _ = append_stream_pcm(id, &pcm).await;
                    // Grow the partial block while speaking.
                    let now_ms = chrono::Utc::now().timestamp_millis();
                    let _ = crate::core::multimodal::persist_mobile_transcript(
                        &state.db,
                        &session_id,
                        id,
                        "",
                        started_at_ms,
                        now_ms,
                        None,
                        None,
                        "parakeet-tdt",
                        false,
                    )
                    .await;
                }
            }
            Message::Close(_) => break,
            _ => {}
        }
    }
    emit_status(&state, false);
}

pub(super) async fn current_session_id(state: &MobileState) -> String {
    if let Some(manager) = &state.session {
        if let Ok(Some(session)) = manager.get_current_session().await {
            return session.id;
        }
    }
    "mobile".to_string()
}
