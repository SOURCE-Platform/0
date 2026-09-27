//! The axum adapter: every request goes to `Provider::handle` on a
//! blocking thread (the core and its stores are synchronous). Logs carry
//! the route, status, vault id and key id only — never bodies (§11.1).

use std::net::SocketAddr;
use std::sync::Arc;

use axum::body::{to_bytes, Body};
use axum::extract::{ConnectInfo, Request, State};
use axum::http::{HeaderValue, StatusCode};
use axum::response::Response;
use vault_provider_core::{Provider, Request as CoreRequest};

/// Index blobs are the largest (8 MiB); a create body ≤ 1 MiB + framing.
const MAX_BODY: usize = (8 << 20) + (64 << 10);

fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

pub async fn handle(State(p): State<Arc<Provider>>, ConnectInfo(addr): ConnectInfo<SocketAddr>, req: Request) -> Response {
    let (parts, body) = req.into_parts();
    let Ok(body) = to_bytes(body, MAX_BODY).await else {
        return status(StatusCode::PAYLOAD_TOO_LARGE, b"{\"error\":\"TOO_LARGE\"}".to_vec(), true);
    };
    let method = parts.method.to_string();
    let path = parts.uri.path().to_string();
    let auth = parts.headers.get("ov0-auth").and_then(|v| v.to_str().ok()).map(String::from);
    let ip = addr.ip().to_string();
    let log_path = path.clone();
    let r = tokio::task::spawn_blocking(move || {
        p.handle(&CoreRequest { method: &method, path: &path, auth: auth.as_deref(), body: &body, now: now(), client_ip: &ip })
    })
    .await;
    match r {
        Ok(r) => {
            eprintln!("{} {} {}", parts.method, route_of(&log_path), r.status);
            status(StatusCode::from_u16(r.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR), r.body, r.json)
        }
        Err(_) => status(StatusCode::INTERNAL_SERVER_ERROR, Vec::new(), true),
    }
}

/// Route with the vault id kept and the blob name dropped.
fn route_of(path: &str) -> String {
    match path.find("/blobs/") {
        Some(i) => format!("{}/blobs/…", &path[..i]),
        None => path.to_string(),
    }
}

fn status(code: StatusCode, body: Vec<u8>, json: bool) -> Response {
    let mut r = Response::new(Body::from(body));
    *r.status_mut() = code;
    let ct = if json { "application/json" } else { "application/octet-stream" };
    r.headers_mut().insert("content-type", HeaderValue::from_static(ct));
    r.headers_mut().insert("cache-control", HeaderValue::from_static("no-store"));
    r
}
