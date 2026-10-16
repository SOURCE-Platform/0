//! Joining a vault from an iPhone (F.2e, spec §22.13; design
//! `phase-f2e-design.md` revision 2): this Mac has no vault; the iPhone
//! adds it as a replacement. This process serves the ephemeral TLS
//! listener and the `v:3` QR, and relays frames to the helper, which
//! decides everything: the secret, the transcript, the code (shown only in
//! its own "Join a Vault" panel), the bundle checks and the ACK.
//!
//! Routes: `POST /v1/vault/renroll/hello`, `POST /v1/vault/renroll/bundle`
//! (route proof; waits for the panel; answers the ACK and this Mac's peer
//! endpoint), `POST /v1/vault/renroll/done` (route proof; the iPhone
//! committed the entry → the vault moves into place).

use std::sync::{Mutex, OnceLock};
use std::time::Instant;

use axum::body::Bytes;
use axum::extract::{DefaultBodyLimit, Json};
use axum::http::{HeaderMap, StatusCode};
use axum::routing::post;
use axum::Router;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use super::server::{self, EnrollHandle};
use super::session::{call, SESSION_TTL};

const MAX_BUNDLE: usize = 256 << 20;
const CHUNK: usize = 48 * 1024;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Stage {
    Waiting,
    Comparing,
    Confirmed,
    Joined,
    Failed,
}

struct Join {
    started: Instant,
    handle: EnrollHandle,
    stage: Stage,
    /// The iPhone's id, from its hello: its peer-sync token is issued to it.
    authorizer: Option<String>,
    error: Option<String>,
}

fn current() -> &'static Mutex<Option<Join>> {
    static CURRENT: OnceLock<Mutex<Option<Join>>> = OnceLock::new();
    CURRENT.get_or_init(|| Mutex::new(None))
}

fn with<T>(f: impl FnOnce(&mut Join) -> T) -> Option<T> {
    let mut g = current().lock().unwrap_or_else(|e| e.into_inner());
    if g.as_ref().is_some_and(|j| j.started.elapsed() >= SESSION_TTL && j.stage != Stage::Joined) {
        if let Some(j) = g.take() {
            j.handle.shutdown();
        }
        return None;
    }
    g.as_mut().map(f)
}

fn stopped() {
    let mut g = current().lock().unwrap_or_else(|e| e.into_inner());
    if g.as_ref().is_some_and(|j| j.stage != Stage::Joined) {
        *g = None;
    }
}

/// Start: fresh certificate, helper keys and commitment, the QR.
pub async fn begin(name: &str) -> Result<Value, String> {
    cancel().await;
    let hosts = crate::core::mobile::routes_vault_peer::private_addresses();
    let host = hosts.first().cloned().ok_or("This Mac has no local network address. Connect to Wi-Fi and try again.")?;
    let (cert_pem, key_pem, der) = server::fresh_cert()?;
    let fp = hex(Sha256::digest(&der));
    let begun = call(json!({"op": "renroll_begin", "fp": fp, "name": name})).await?;
    if begun["ok"] != true {
        return Err(begun["error"].as_str().unwrap_or("REFUSED").to_string());
    }
    let app = Router::new()
        .route("/v1/vault/renroll/hello", post(hello))
        .route("/v1/vault/renroll/bundle", post(bundle).layer(DefaultBodyLimit::max(MAX_BUNDLE + 4096)))
        .route("/v1/vault/renroll/done", post(done));
    let (port, handle) = server::start_router(cert_pem, key_pem, app, stopped).await?;
    let payload = json!({ "v": 3, "host": host, "port": port, "fp": fp, "secret": begun["secret"], "commit": begun["commit"] });
    let qr = render(&payload)?;
    *current().lock().unwrap_or_else(|e| e.into_inner()) = Some(Join { started: Instant::now(), handle, stage: Stage::Waiting, authorizer: None, error: None });
    Ok(json!({ "ok": true, "qr": qr, "expires_in": begun["expires_in"] }))
}

fn render(payload: &Value) -> Result<String, String> {
    let code = qrcode::QrCode::with_error_correction_level(payload.to_string().as_bytes(), qrcode::EcLevel::M).map_err(|e| format!("join QR: {e}"))?;
    let svg = code.render::<qrcode::render::svg::Color>().min_dimensions(260, 260).quiet_zone(true).build();
    Ok(svg.find("<svg").map_or(svg.clone(), |i| svg[i..].to_string()))
}

pub fn status() -> Value {
    let stage = |s: Stage| match s {
        Stage::Waiting => "waiting",
        Stage::Comparing => "comparing",
        Stage::Confirmed => "confirmed",
        Stage::Joined => "joined",
        Stage::Failed => "failed",
    };
    with(|j| json!({ "ok": true, "active": true, "stage": stage(j.stage), "error": j.error, "expires_in": SESSION_TTL.saturating_sub(j.started.elapsed()).as_secs() }))
        .unwrap_or(json!({ "ok": true, "active": false }))
}

pub async fn cancel() {
    if let Some(j) = current().lock().unwrap_or_else(|e| e.into_inner()).take() {
        j.handle.shutdown();
    }
    let _ = call(json!({"op": "renroll_abort"})).await;
}

fn fail(error: &str) {
    with(|j| {
        j.stage = Stage::Failed;
        j.error = Some(error.to_string());
    });
}

type Reply = (StatusCode, Json<Value>);

fn refused(message: &str) -> Reply {
    (StatusCode::FORBIDDEN, Json(json!({ "ok": false, "error": message })))
}

async fn hello(Json(body): Json<Value>) -> Reply {
    let resp = match call(json!({"op": "renroll_hello", "hello": body})).await {
        Ok(r) if r["ok"] == true => r,
        Ok(r) => return refused(r["error"].as_str().unwrap_or("REFUSED")),
        Err(_) => return refused("HELPER_UNAVAILABLE"),
    };
    with(|j| {
        j.stage = Stage::Comparing;
        j.authorizer = body["authorizer_device_id"].as_str().map(String::from);
    });
    // The helper's own panel shows the code and asks for the master
    // password; the iPhone's bundle waits for it.
    tokio::spawn(async {
        match call(json!({"op": "renroll_confirm"})).await {
            Ok(r) if r["ok"] == true => {
                with(|j| j.stage = Stage::Confirmed);
            }
            _ => fail("CANCELLED"),
        }
    });
    (StatusCode::OK, Json(json!({ "ok": true, "reply": resp["reply"] })))
}

async fn proven(headers: &HeaderMap) -> bool {
    let Some(proof) = headers.get("x-ov0-proof").and_then(|v| v.to_str().ok()) else { return false };
    matches!(call(json!({"op": "renroll_proof", "proof": proof})).await, Ok(r) if r["ok"] == true)
}

/// The panel's Continue, then the bundle into the helper, then the ACK
/// and this Mac's peer endpoint.
async fn bundle(headers: HeaderMap, body: Bytes) -> Reply {
    if !proven(&headers).await {
        return refused("PROOF");
    }
    loop {
        match with(|j| j.stage) {
            Some(Stage::Confirmed) => break,
            Some(Stage::Comparing) => tokio::time::sleep(std::time::Duration::from_millis(300)).await,
            _ => return refused("CANCELLED"),
        }
    }
    let sha = hex(Sha256::digest(&body));
    let r = match relay(&body, &sha).await {
        Ok(r) => r,
        Err(e) => {
            fail(&e);
            return refused(&e);
        }
    };
    let authorizer = with(|j| j.authorizer.clone()).flatten();
    let endpoint = match authorizer {
        Some(id) => peer_endpoint(&id).await,
        None => Value::Null,
    };
    (StatusCode::OK, Json(json!({ "ok": true, "ack": r["ack"], "registry_head": r["registry_head"], "peer_endpoint": endpoint })))
}

async fn relay(body: &[u8], sha: &str) -> Result<Value, String> {
    let code = |r: &Value| r["error"].as_str().unwrap_or("REFUSED").to_string();
    let b = call(json!({"op": "renroll_bundle_begin", "sha256": sha, "size": body.len()})).await?;
    let session = b["session"].as_str().ok_or_else(|| code(&b))?.to_string();
    let s = call(json!({"op": "stream_begin", "session": session, "sha256": sha, "size": body.len()})).await?;
    let stream = s["stream_id"].as_str().ok_or_else(|| code(&s))?.to_string();
    for (seq, chunk) in body.chunks(CHUNK).enumerate() {
        let w = call(json!({"op": "stream_write", "session": session, "stream_id": stream, "seq": seq, "offset": seq * CHUNK, "data": vault_proto::b64::encode(chunk)})).await?;
        if w["ok"] != true {
            return Err(code(&w));
        }
    }
    let e = call(json!({"op": "stream_end", "session": session, "stream_id": stream})).await?;
    if e["ok"] != true {
        return Err(code(&e));
    }
    let r = call(json!({"op": "renroll_complete", "session": session})).await?;
    if r["ok"] != true {
        return Err(code(&r));
    }
    Ok(r)
}

/// Wire annex A.4, as forward: the pin, a token bound to the iPhone's id,
/// the port, the address hints. Absent if the mobile server is off.
async fn peer_endpoint(authorizer: &str) -> Value {
    let (Some(tokens), Some(spki)) = (crate::core::mobile::peer_tokens::shared(), crate::core::mobile::peer_tokens::spki_sha256()) else {
        return Value::Null;
    };
    let port = crate::core::mobile::mobile_port();
    match tokens.issue(authorizer).await {
        Ok(token) if port != 0 => json!({ "spki_sha256": spki, "token": token, "port": port, "host_hints": crate::core::mobile::routes_vault_peer::private_addresses() }),
        _ => Value::Null,
    }
}

async fn done(headers: HeaderMap, Json(body): Json<Value>) -> Reply {
    if !proven(&headers).await {
        return refused("PROOF");
    }
    match call(json!({"op": "renroll_finish", "registry_head": body["registry_head"]})).await {
        Ok(r) if r["ok"] == true => {
            with(|j| {
                j.stage = Stage::Joined;
                j.handle.shutdown();
            });
            (StatusCode::OK, Json(json!({ "ok": true })))
        }
        Ok(r) => refused(r["error"].as_str().unwrap_or("REFUSED")),
        Err(_) => refused("HELPER_UNAVAILABLE"),
    }
}

fn hex(bytes: impl AsRef<[u8]>) -> String {
    bytes.as_ref().iter().map(|b| format!("{b:02x}")).collect()
}
