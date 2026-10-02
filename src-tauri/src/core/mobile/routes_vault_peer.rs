//! `POST /v1/vault/peer` (spec v0.5 §22.8; wire annex A.2.1): the only
//! route a SOURCE Vault phone uses. Main is a dumb, defensive pipe:
//!
//! - private-network source addresses only;
//! - the peer token in the `Authorization` header only (never a query
//!   string), from its own store — a SOURCE Mobile token never passes,
//!   and the token must have been issued to the request's sender;
//! - an HTTP body over 1 MiB + 4 KiB is `413` before any helper call;
//! - the carriage entry is passed to the helper as opaque bytes; every
//!   vault decision and every signature is the helper's. Refusals carry a
//!   zero-length body.

use std::net::{IpAddr, SocketAddr};

use axum::body::Bytes;
use axum::extract::ConnectInfo;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use serde_json::{json, Value};
use vault_proto::b64;
use vault_proto::crypto::tlv::{EntryBuilder, EntryReader};
use vault_proto::peer::PeerRequest;

pub const MAX_HTTP_BODY: usize = (1 << 20) + 4096;
/// Bodies above this go through `peer_serve_begin` and a stream session.
const INLINE: usize = 24 * 1024;

fn empty(code: StatusCode) -> Response {
    (code, Bytes::new()).into_response()
}

/// RFC 1918, link-local, unique-local, loopback.
pub fn private(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => v4.is_private() || v4.is_link_local() || v4.is_loopback(),
        IpAddr::V6(v6) => {
            let seg = v6.segments()[0];
            v6.is_loopback() || (seg & 0xfe00) == 0xfc00 || (seg & 0xffc0) == 0xfe80 || v6.to_ipv4_mapped().is_some_and(|v4| v4.is_private() || v4.is_loopback())
        }
    }
}

pub async fn peer(ConnectInfo(addr): ConnectInfo<SocketAddr>, headers: HeaderMap, body: Bytes) -> Response {
    if !private(addr.ip()) {
        return empty(StatusCode::FORBIDDEN);
    }
    if body.len() > MAX_HTTP_BODY {
        return empty(StatusCode::PAYLOAD_TOO_LARGE);
    }
    let token = headers.get("authorization").and_then(|v| v.to_str().ok()).and_then(|v| v.strip_prefix("Bearer "));
    let (Some(token), Some(store)) = (token, super::peer_tokens::shared()) else {
        return empty(StatusCode::UNAUTHORIZED);
    };
    let Some(issued_to) = store.verify(token).await else {
        return empty(StatusCode::UNAUTHORIZED);
    };
    let Ok(e) = EntryReader::parse(&body) else {
        return empty(StatusCode::FORBIDDEN);
    };
    let (Some(tlv), Some(sig), Some(inner)) = (e.get(0x01), e.get(0x02), e.get(0x03)) else {
        return empty(StatusCode::FORBIDDEN);
    };
    // The token is bound to its device: the sender must be that device.
    match PeerRequest::decode(tlv) {
        Ok(req) if vault_proto::crypto::hex::encode(req.sender_device_id) == issued_to => {}
        _ => return empty(StatusCode::UNAUTHORIZED),
    }
    let answered = if inner.len() <= INLINE {
        relay(json!({ "op": "peer_serve", "request_tlv": b64::encode(tlv), "signature": b64::encode(sig), "body": b64::encode(inner) })).await
    } else {
        streamed(tlv, sig, inner).await
    };
    match answered {
        Ok(bytes) => (StatusCode::OK, [("content-type", "application/octet-stream")], bytes).into_response(),
        Err(code) => empty(code),
    }
}

/// A large request: the helper checks the envelope at `peer_serve_begin`
/// (it may answer at once), then the body streams in and `peer_serve
/// {session}` completes it.
async fn streamed(tlv: &[u8], sig: &[u8], body: &[u8]) -> Result<Vec<u8>, StatusCode> {
    let begun = call(json!({ "op": "peer_serve_begin", "request_tlv": b64::encode(tlv), "signature": b64::encode(sig), "size": body.len() }))
        .await
        .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
    let Some(session) = begun["session"].as_str().filter(|_| begun.get("need").is_some()).map(str::to_string) else {
        return answer(begun).await; // a refusal or an immediate signed status
    };
    let sha = vault_proto::crypto::hex::encode(vault_proto::peer::body_hash(body));
    let s = call(json!({"op": "stream_begin", "session": session, "sha256": sha, "size": body.len()})).await.map_err(|_| StatusCode::FORBIDDEN)?;
    let stream = s["stream_id"].as_str().ok_or(StatusCode::FORBIDDEN)?.to_string();
    for (i, chunk) in body.chunks(INLINE).enumerate() {
        let w = call(json!({"op": "stream_write", "session": session, "stream_id": stream, "seq": i, "offset": i * INLINE, "data": b64::encode(chunk)})).await;
        if !w.is_ok_and(|w| w["ok"] == true) {
            return Err(StatusCode::FORBIDDEN);
        }
    }
    let _ = call(json!({"op": "stream_end", "session": session, "stream_id": stream})).await;
    relay(json!({ "op": "peer_serve", "session": session })).await
}

/// The helper's answer as the carriage entry (streamed bodies are read
/// back and their session closed).
async fn relay(frame: Value) -> Result<Vec<u8>, StatusCode> {
    answer(call(frame).await.map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?).await
}

async fn answer(r: Value) -> Result<Vec<u8>, StatusCode> {
    if let Some(code) = r.get("refused").and_then(Value::as_u64) {
        return Err(StatusCode::from_u16(code as u16).unwrap_or(StatusCode::FORBIDDEN));
    }
    if r["ok"] != true {
        return Err(StatusCode::SERVICE_UNAVAILABLE);
    }
    let field = |k: &str| r[k].as_str().and_then(b64::decode).ok_or(StatusCode::SERVICE_UNAVAILABLE);
    let (tlv, sig) = (field("response_tlv")?, field("signature")?);
    let body = match r.get("body") {
        Some(_) => field("body")?,
        None => read_stream(&r).await?,
    };
    EntryBuilder::new()
        .field_bytes(0x01, &tlv)
        .and_then(|b| b.field_bytes(0x02, &sig))
        .and_then(|b| b.field_bytes(0x03, &body))
        .map(EntryBuilder::build)
        .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)
}

async fn read_stream(r: &Value) -> Result<Vec<u8>, StatusCode> {
    let (Some(session), Some(sha)) = (r["session"].as_str(), r["stream"].as_str()) else {
        return Err(StatusCode::SERVICE_UNAVAILABLE);
    };
    let mut out = Vec::new();
    loop {
        let c = call(json!({"op": "stream_read", "session": session, "sha256": sha, "offset": out.len()})).await.map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
        out.extend(c["data"].as_str().and_then(b64::decode).ok_or(StatusCode::SERVICE_UNAVAILABLE)?);
        if c["eof"] == true {
            break;
        }
    }
    let _ = call(json!({"op": "session_close", "session": session})).await;
    Ok(out)
}

async fn call(frame: Value) -> Result<Value, String> {
    #[cfg(target_os = "macos")]
    {
        tauri::async_runtime::spawn_blocking(move || crate::core::vault_client::request(frame)).await.map_err(|e| e.to_string())?
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = frame;
        Err("vault is not available on this platform".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::private;

    #[test]
    fn only_private_networks_reach_the_peer_route() {
        for ok in ["10.1.2.3", "172.16.0.9", "192.168.1.20", "169.254.3.4", "127.0.0.1", "fd12::1", "fe80::1", "::1"] {
            assert!(private(ok.parse().unwrap()), "{ok}");
        }
        for bad in ["8.8.8.8", "100.64.1.1", "172.32.0.1", "2001:db8::1", "2606:4700::1"] {
            assert!(!private(bad.parse().unwrap()), "{bad}");
        }
    }
}
