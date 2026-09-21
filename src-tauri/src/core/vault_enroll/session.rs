//! The one in-flight enrollment session this process tracks.

use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tokio::sync::watch;

use crate::core::vault_client;

/// §5.2: the whole flow lives 300 s, on both sides.
pub const SESSION_TTL: Duration = Duration::from_secs(300);

/// What the QR on the Mac's screen encodes (§5.2). `v:2` distinguishes
/// vault enrollment from the legacy Source pairing payload; the phone
/// pins `fp` before it sends anything.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnrollmentPayloadV2 {
    pub v: u8,
    pub host: String,
    pub port: u16,
    pub fp: String,
    pub secret: String,
    pub mac_device_id: String,
    pub name: String,
}

pub struct Session {
    pub payload: EnrollmentPayloadV2,
    pub started: Instant,
    /// Set when the helper has authenticated the phone's hello.
    pub sas: Option<String>,
    /// The transfer bundle, published when the user confirms on the Mac.
    /// The phone's request waits on this.
    pub bundle: watch::Sender<Option<Value>>,
    pub handle: super::server::EnrollHandle,
    pub acked: bool,
}

impl Session {
    pub fn expired(&self) -> bool {
        self.started.elapsed() >= SESSION_TTL
    }

    pub fn expires_in(&self) -> u64 {
        SESSION_TTL.saturating_sub(self.started.elapsed()).as_secs()
    }
}

pub fn current() -> &'static Mutex<Option<Session>> {
    static CURRENT: OnceLock<Mutex<Option<Session>>> = OnceLock::new();
    CURRENT.get_or_init(|| Mutex::new(None))
}

fn with_session<T>(f: impl FnOnce(&mut Session) -> T) -> Option<T> {
    let mut guard = current().lock().unwrap_or_else(|e| e.into_inner());
    let expired = guard.as_ref().is_some_and(Session::expired);
    if expired {
        if let Some(s) = guard.take() {
            s.handle.shutdown();
        }
        return None;
    }
    guard.as_mut().map(f)
}

/// Start an enrollment: fresh certificate, ephemeral port, helper-minted
/// single-use secret, QR for the screen.
pub async fn begin(app_name: &str) -> Result<Value, String> {
    cancel().await;
    let (cert_pem, key_pem, der) = super::server::fresh_cert()?;
    let fp = hex(Sha256::digest(&der));
    // The fingerprint enters the helper's transcript, so the SAS the user
    // compares covers the exact channel the phone pinned off the screen.
    let begun = call(json!({"op": "begin_enrollment", "fp": fp})).await?;
    let secret = begun
        .get("secret")
        .and_then(Value::as_str)
        .ok_or("helper did not return an enrollment secret")?
        .to_string();
    let mac_device_id = begun
        .get("mac_device_id")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();

    let (port, handle) = super::server::start(cert_pem, key_pem).await?;
    let payload = EnrollmentPayloadV2 {
        v: 2,
        host: crate::core::mobile::qr::local_hostname(),
        port,
        fp,
        secret,
        mac_device_id,
        name: app_name.to_string(),
    };
    let qr = super::server::render_qr(&payload)?;
    let (bundle, _) = watch::channel(None);
    let expires_in = begun
        .get("expires_in")
        .and_then(Value::as_u64)
        .unwrap_or(SESSION_TTL.as_secs());
    let response = json!({
        "ok": true,
        "qr": qr,
        "host": payload.host,
        "port": payload.port,
        "fp": payload.fp,
        "expires_in": expires_in,
    });
    *current().lock().unwrap_or_else(|e| e.into_inner()) = Some(Session {
        payload,
        started: Instant::now(),
        sas: None,
        bundle,
        handle,
        acked: false,
    });
    Ok(response)
}

/// What the Vault tab polls while the QR is on screen.
pub fn status() -> Value {
    match with_session(|s| {
        json!({
            "ok": true,
            "active": true,
            "sas": s.sas.clone(),
            "acked": s.acked,
            "expires_in": s.expires_in(),
        })
    }) {
        Some(v) => v,
        None => json!({"ok": true, "active": false, "sas": Value::Null, "acked": false}),
    }
}

pub fn sas() -> Option<String> {
    with_session(|s| s.sas.clone()).flatten()
}

pub(super) fn record_sas(value: String) {
    with_session(|s| s.sas = Some(value));
}

pub(super) fn record_ack() {
    with_session(|s| s.acked = true);
}

pub(super) fn publish_bundle(bundle: Value) -> Result<(), String> {
    with_session(|s| {
        let _ = s.bundle.send(Some(bundle));
    })
    .ok_or_else(|| "no enrollment in progress".to_string())
}

pub(super) fn bundle_receiver() -> Option<watch::Receiver<Option<Value>>> {
    with_session(|s| s.bundle.subscribe())
}

/// The user compared the SAS and confirmed: ask the helper to build the
/// bundle (behind its own presence check) and hand it to the phone.
pub async fn confirm() -> Result<Value, String> {
    if with_session(|_| ()).is_none() {
        return Err("no enrollment in progress".to_string());
    }
    eprintln!("vault-enroll: confirm → helper (presence check follows)");
    let resp = call(json!({"op": "enroll_confirm"})).await?;
    let bundle = resp
        .get("bundle")
        .cloned()
        .ok_or("helper did not return an enrollment bundle")?;
    publish_bundle(bundle)?;
    eprintln!("vault-enroll: bundle published to the waiting device");
    Ok(json!({"ok": true}))
}

/// Tear the session down: the server stops, the helper burns the secret.
pub async fn cancel() {
    let taken = current().lock().unwrap_or_else(|e| e.into_inner()).take();
    if let Some(session) = taken {
        session.handle.shutdown();
    }
    let _ = call(json!({"op": "cancel_enrollment"})).await;
}

/// The enrollment server stopped. Drop the session so the Vault tab stops
/// showing a QR that nothing can answer.
pub(super) fn note_server_stopped() {
    let _ = current().lock().unwrap_or_else(|e| e.into_inner()).take();
}

pub(super) async fn call(frame: Value) -> Result<Value, String> {
    #[cfg(target_os = "macos")]
    {
        tauri::async_runtime::spawn_blocking(move || vault_client::request(frame))
            .await
            .map_err(|e| format!("vault task join failed: {e}"))?
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = frame;
        Err("vault is not available on this platform".to_string())
    }
}

fn hex(bytes: impl AsRef<[u8]>) -> String {
    bytes.as_ref().iter().map(|b| format!("{b:02x}")).collect()
}
