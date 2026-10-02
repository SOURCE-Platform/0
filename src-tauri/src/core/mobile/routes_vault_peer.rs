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

/// Any non-200 answer from this route carries a zero-length body,
/// including the 413 axum's body limit produces itself.
pub async fn bare_refusals(r: Response) -> Response {
    if r.status() == StatusCode::OK { r } else { empty(r.status()) }
}

/// RFC 1918, link-local, unique-local — the phone's private network
/// (annex A.2.1; no loopback: the phone is never on this Mac).
pub fn private(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => v4.is_private() || v4.is_link_local(),
        IpAddr::V6(v6) => {
            let seg = v6.segments()[0];
            (seg & 0xfe00) == 0xfc00 || (seg & 0xffc0) == 0xfe80 || v6.to_ipv4_mapped().is_some_and(|v4| v4.is_private() || v4.is_link_local())
        }
    }
}

/// `peer_endpoint.host_hints` (annex A.4): IP literals of this Mac's
/// private-network addresses, at most 8 — here the address of the default
/// route's interface, found by a UDP `connect` that sends nothing. Empty
/// when there is none; the phone then browses mDNS, and the SPKI pin
/// decides either way.
pub fn private_addresses() -> Vec<String> {
    let probe = |bind: &str, to: &str| {
        let s = std::net::UdpSocket::bind(bind).ok()?;
        s.connect(to).ok()?;
        s.local_addr().ok().map(|a| a.ip()).filter(|ip| private(*ip))
    };
    [probe("0.0.0.0:0", "192.0.2.1:9"), probe("[::]:0", "[2001:db8::1]:9")].into_iter().flatten().map(|ip| ip.to_string()).take(8).collect()
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
    use axum::body::Bytes;
    use axum::extract::ConnectInfo;
    use axum::http::{HeaderMap, StatusCode};

    async fn status_and_body(r: axum::response::Response) -> (StatusCode, usize) {
        let s = r.status();
        (s, axum::body::to_bytes(r.into_body(), usize::MAX).await.unwrap().len())
    }

    fn carriage(sender: [u8; 16]) -> Bytes {
        let req = vault_proto::peer::PeerRequest {
            vault_id: [1; 16],
            sender_device_id: sender,
            receiver_device_id: [3; 16],
            operation: vault_proto::peer::PeerOp::Hello,
            body_sha256: vault_proto::peer::body_hash(&vault_proto::peer::body::empty()),
            t: 1_790_000_000,
            n: [4; 16],
        };
        let e = vault_proto::crypto::tlv::EntryBuilder::new()
            .field_bytes(0x01, &req.encode())
            .and_then(|b| b.field_bytes(0x02, &[0u8; 64]))
            .and_then(|b| b.field_bytes(0x03, &vault_proto::peer::body::empty()))
            .unwrap()
            .build();
        Bytes::from(e)
    }

    fn bearer(t: &str) -> HeaderMap {
        let mut h = HeaderMap::new();
        h.insert("authorization", format!("Bearer {t}").parse().unwrap());
        h
    }

    /// PW-06 / PW-12 on main (review VER-I8): every refusal is decided
    /// before any helper call, and carries a zero-length body.
    #[tokio::test]
    async fn tokens_are_scoped_bound_to_their_device_and_refused_bare() {
        let pool = sqlx::sqlite::SqlitePoolOptions::new().max_connections(1).connect("sqlite::memory:").await.unwrap();
        let db = crate::core::database::Database { pool };
        db.run_migrations().await.unwrap();
        crate::core::mobile::peer_tokens::install(std::sync::Arc::new(db), "00".repeat(32));
        let tokens = crate::core::mobile::peer_tokens::shared().unwrap();
        let phone = [7u8; 16];
        let token = tokens.issue(&vault_proto::crypto::hex::encode(phone)).await.unwrap();
        let lan = ConnectInfo("192.168.1.20:5000".parse().unwrap());

        // No token, or one from any other scope: 401.
        let r = super::peer(lan, HeaderMap::new(), carriage(phone)).await;
        assert_eq!(status_and_body(r).await, (StatusCode::UNAUTHORIZED, 0));
        let r = super::peer(lan, bearer("a-source-mobile-token"), carriage(phone)).await;
        assert_eq!(status_and_body(r).await, (StatusCode::UNAUTHORIZED, 0));
        // A real token presented for another sender: 401.
        let r = super::peer(lan, bearer(&token), carriage([8; 16])).await;
        assert_eq!(status_and_body(r).await, (StatusCode::UNAUTHORIZED, 0));
        // Over the size cap: 413, before the token is even looked at.
        let big = Bytes::from(vec![0u8; super::MAX_HTTP_BODY + 1]);
        let r = super::peer(lan, bearer(&token), big).await;
        assert_eq!(status_and_body(r).await, (StatusCode::PAYLOAD_TOO_LARGE, 0));
        // From outside the private network: 403.
        let r = super::peer(ConnectInfo("8.8.8.8:5000".parse().unwrap()), bearer(&token), carriage(phone)).await;
        assert_eq!(status_and_body(r).await, (StatusCode::FORBIDDEN, 0));
        // Forgotten: the token stops working.
        tokens.forget(&vault_proto::crypto::hex::encode(phone)).await.unwrap();
        let r = super::peer(lan, bearer(&token), carriage(phone)).await;
        assert_eq!(status_and_body(r).await, (StatusCode::UNAUTHORIZED, 0));
    }

    #[tokio::test]
    async fn axums_own_refusals_lose_their_body() {
        use axum::response::IntoResponse;
        let r = (StatusCode::PAYLOAD_TOO_LARGE, "length limit exceeded").into_response();
        assert_eq!(status_and_body(super::bare_refusals(r).await).await, (StatusCode::PAYLOAD_TOO_LARGE, 0));
    }

    #[test]
    fn only_private_networks_reach_the_peer_route() {
        for ok in ["10.1.2.3", "172.16.0.9", "192.168.1.20", "169.254.3.4", "fd12::1", "fe80::1"] {
            assert!(private(ok.parse().unwrap()), "{ok}");
        }
        for bad in ["8.8.8.8", "100.64.1.1", "172.32.0.1", "2001:db8::1", "2606:4700::1", "127.0.0.1", "::1"] {
            assert!(!private(bad.parse().unwrap()), "{bad}");
        }
    }

    #[test]
    fn host_hints_are_private_ip_literals() {
        let hints = super::private_addresses();
        assert!(hints.len() <= 8);
        for h in hints {
            assert!(private(h.parse::<std::net::IpAddr>().expect("an IP literal")), "{h}");
        }
    }
}
