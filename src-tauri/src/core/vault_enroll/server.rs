//! The ephemeral enrollment server (§5): a fresh self-signed certificate,
//! a port the OS picks, three routes, and a shutdown the moment the flow
//! ends. It is bound only for the session, so the always-on mobile server
//! is never enrollment attack surface.
//!
//! The routes are a thin HTTP shape over the §5.2 messages. This process
//! reads only enough of each body to route it; the helper decides
//! everything, including whether the secret is right.

use std::net::SocketAddr;

use axum::extract::Json;
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::Router;
use axum_server::tls_rustls::RustlsConfig;
use axum_server::Handle;

/// The server's shutdown handle, typed for a TCP listener.
pub type EnrollHandle = Handle<std::net::SocketAddr>;
use serde_json::{json, Value};

use super::session::{self, EnrollmentPayloadV2};

/// §5.2: per-message timeout. The bundle wait is the one long poll — the
/// user has to compare the SAS and pass Touch ID in between.
const MESSAGE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

/// A certificate for exactly one enrollment. Returns PEM cert, PEM key,
/// and the DER the QR's fingerprint is taken over.
pub fn fresh_cert() -> Result<(Vec<u8>, Vec<u8>, Vec<u8>), String> {
    let certified = rcgen::generate_simple_self_signed(vec!["source-vault.local".to_string()])
        .map_err(|e| format!("enrollment certificate: {e}"))?;
    let der = certified.cert.der().to_vec();
    Ok((
        certified.cert.pem().into_bytes(),
        certified.signing_key.serialize_pem().into_bytes(),
        der,
    ))
}

pub fn render_qr(payload: &EnrollmentPayloadV2) -> Result<String, String> {
    let json = serde_json::to_string(payload).map_err(|e| format!("enrollment payload: {e}"))?;
    let code = qrcode::QrCode::with_error_correction_level(json.as_bytes(), qrcode::EcLevel::M)
        .map_err(|e| format!("enrollment QR: {e}"))?;
    let rendered = code
        .render::<qrcode::render::svg::Color>()
        .min_dimensions(260, 260)
        .quiet_zone(true)
        .dark_color(qrcode::render::svg::Color("#1c1917"))
        .light_color(qrcode::render::svg::Color("#ffffff"))
        .build();
    Ok(match rendered.find("<svg") {
        Some(i) => rendered[i..].to_string(),
        None => rendered,
    })
}

/// Bind on an OS-chosen port and serve until the handle is shut down.
/// Returns the port the QR must advertise.
pub async fn start(cert_pem: Vec<u8>, key_pem: Vec<u8>) -> Result<(u16, EnrollHandle), String> {
    let tls = RustlsConfig::from_pem(cert_pem, key_pem)
        .await
        .map_err(|e| format!("enrollment TLS config: {e}"))?;
    let listener = std::net::TcpListener::bind(SocketAddr::from(([0, 0, 0, 0], 0)))
        .map_err(|e| format!("enrollment listener: {e}"))?;
    // tokio refuses to register a blocking socket, and axum-server panics
    // on the spot if we hand it one.
    listener
        .set_nonblocking(true)
        .map_err(|e| format!("enrollment listener: {e}"))?;
    let port = listener
        .local_addr()
        .map_err(|e| format!("enrollment port: {e}"))?
        .port();
    let app = Router::new()
        .route("/v1/vault/enroll/hello", post(hello))
        .route("/v1/vault/enroll/bundle", get(bundle))
        .route("/v1/vault/enroll/ack", post(ack));
    let handle = EnrollHandle::new();
    let serve_handle = handle.clone();
    let serving = tokio::spawn(async move {
        if let Ok(server) = axum_server::from_tcp_rustls(listener, tls) {
            let _ = server
                .handle(serve_handle)
                .serve(app.into_make_service())
                .await;
        }
    });
    // If the server stops for any reason — error, panic, or shutdown —
    // the session goes with it. Showing a QR for a listener that is not
    // listening is exactly the failure this supervisor makes visible.
    tokio::spawn(async move {
        let _ = serving.await;
        session::note_server_stopped();
    });
    Ok((port, handle))
}

type Reply = (StatusCode, Json<Value>);

fn refused(message: &str) -> Reply {
    (StatusCode::FORBIDDEN, Json(json!({"ok": false, "error": message})))
}

/// §5.2 ENROLL_HELLO, relayed verbatim. The helper authenticates it; a
/// refusal here carries no detail beyond the helper's own error code.
async fn hello(Json(body): Json<Value>) -> Reply {
    let mut frame = body;
    frame["op"] = json!("enroll_hello");
    let Ok(resp) = tokio::time::timeout(MESSAGE_TIMEOUT, session::call(frame)).await else {
        return refused("TIMEOUT");
    };
    match resp {
        Ok(resp) if resp["ok"] == Value::Bool(true) => {
            if let Some(s) = resp.get("sas").and_then(Value::as_str) {
                eprintln!("vault-enroll: hello accepted, SAS shown on both screens");
                session::record_sas(s.to_string());
            }
            // The SAS stays on the Mac: the phone derives its own from
            // the reply, which is what makes comparing them meaningful.
            (StatusCode::OK, Json(json!({"ok": true, "reply": resp["reply"]})))
        }
        Ok(resp) => {
            eprintln!("vault-enroll: hello refused: {}", resp["error"]);
            refused(resp["error"].as_str().unwrap_or("REFUSED"))
        }
        Err(e) => {
            eprintln!("vault-enroll: hello could not reach the helper: {e}");
            refused("HELPER_UNAVAILABLE")
        }
    }
}

/// The phone waits here while the user compares the SAS on both screens
/// and confirms on the Mac (§5.1). Nothing is sent until that happens.
async fn bundle() -> Reply {
    eprintln!("vault-enroll: device is waiting for the bundle");
    let Some(mut rx) = session::bundle_receiver() else {
        return refused("NO_SESSION");
    };
    if let Some(ready) = rx.borrow_and_update().clone() {
        return (StatusCode::OK, Json(json!({"ok": true, "bundle": ready})));
    }
    let waited = tokio::time::timeout(session::SESSION_TTL, rx.changed()).await;
    match waited {
        Ok(Ok(())) => match rx.borrow().clone() {
            Some(bundle) => (StatusCode::OK, Json(json!({"ok": true, "bundle": bundle}))),
            None => refused("CANCELLED"),
        },
        Ok(Err(_)) => refused("CANCELLED"),
        Err(_) => refused("TIMEOUT"),
    }
}

/// §5.2 ENROLL_ACK: the phone's signature over the registry head. The
/// helper verifies it and only then writes the entry.
async fn ack(Json(body): Json<Value>) -> Reply {
    let mut frame = body;
    frame["op"] = json!("enroll_ack");
    let Ok(resp) = tokio::time::timeout(MESSAGE_TIMEOUT, session::call(frame)).await else {
        return refused("TIMEOUT");
    };
    match resp {
        Ok(resp) if resp["ok"] == Value::Bool(true) => {
            eprintln!("vault-enroll: ACK verified, device enrolled");
            session::record_ack();
            (StatusCode::OK, Json(resp))
        }
        Ok(resp) => refused(resp["error"].as_str().unwrap_or("REFUSED")),
        Err(_) => refused("HELPER_UNAVAILABLE"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The listener handed to axum must be non-blocking: tokio refuses a
    /// blocking socket and the serve task panics immediately, which looks
    /// from the outside like a QR that simply never answers. A real
    /// enrollment run is what found this; this test is what keeps it
    /// found.
    #[tokio::test]
    async fn server_survives_being_started() {
        let (cert, key, _) = fresh_cert().expect("certificate");
        let (port, handle) = start(cert, key).await.expect("server starts");
        assert_ne!(port, 0, "an OS-chosen port is reported back for the QR");

        // A blocking listener would have taken the serve task down before
        // this point; a live one accepts the connection.
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        let connected = tokio::net::TcpStream::connect(("127.0.0.1", port)).await;
        assert!(connected.is_ok(), "enrollment server is not accepting: {connected:?}");

        handle.shutdown();
    }

    /// The QR carries the fingerprint the phone pins, so it has to be the
    /// SHA-256 of the certificate actually served (§5.2).
    #[test]
    fn certificates_are_fresh_per_enrollment() {
        let (_, _, first) = fresh_cert().expect("certificate");
        let (_, _, second) = fresh_cert().expect("certificate");
        assert_ne!(first, second, "each enrollment binds its own certificate");
    }
}
