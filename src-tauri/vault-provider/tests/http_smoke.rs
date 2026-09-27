//! The real HTTP service over `FsStores` on a loopback port (the adapter
//! must pass method, path, `Ov0-Auth`, body and status through intact).
//! Synthetic data only.

use std::net::SocketAddr;
use std::sync::Arc;

use vault_provider_core::fs::FsStores;
use vault_provider_core::{Config, Provider};

fn serve() -> (String, std::path::PathBuf) {
    let dir = std::env::temp_dir().join(format!("vp-smoke-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let std_listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = std_listener.local_addr().unwrap().port();
    let origin = format!("http://127.0.0.1:{port}");
    let p = Arc::new(Provider::with_fs(Config::new(&origin, [0x5e; 32]), Arc::new(FsStores::new(&dir))));
    std::thread::spawn(move || {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async move {
            std_listener.set_nonblocking(true).unwrap();
            let l = tokio::net::TcpListener::from_std(std_listener).unwrap();
            axum::serve(l, vault_provider::router(p).into_make_service_with_connect_info::<SocketAddr>()).await.unwrap();
        });
    });
    (origin, dir)
}

#[test]
fn http_adapter_smoke() {
    let (origin, dir) = serve();
    let _ = rustls::crypto::ring::default_provider().install_default();
    let c = reqwest::blocking::Client::new();
    // Locate: unauthenticated, fake-shaped answer for an unknown handle.
    let r = c.post(format!("{origin}/v2/recover/locate")).body(format!("{{\"handle_key\":\"{}\"}}", "ab".repeat(32))).send().unwrap();
    assert_eq!(r.status(), 200);
    assert!(r.headers().get("date").is_some(), "Date header for the clock check");
    let v: serde_json::Value = serde_json::from_slice(&r.bytes().unwrap()).unwrap();
    assert!(v["vault_id"].is_string() && v["kdf"]["m_kib"] == 65536);
    // A garbage auth header on a vault route: the generic 401.
    let vid = "a0".repeat(16);
    let r = c.get(format!("{origin}/v2/vaults/{vid}/state")).header("Ov0-Auth", "v2.AAAA.AAAA").send().unwrap();
    assert_eq!(r.status(), 401);
    let v: serde_json::Value = serde_json::from_slice(&r.bytes().unwrap()).unwrap();
    assert_eq!(v["error"], "AUTH_INVALID");
    // Unknown route and a non-canonical path.
    assert_eq!(c.get(format!("{origin}/v1/anything")).send().unwrap().status(), 404);
    assert_eq!(c.get(format!("{origin}/v2/vaults/{}/state", "A0".repeat(16))).send().unwrap().status(), 404);
    // Oversize body.
    let big = vec![0u8; (8 << 20) + (128 << 10)];
    assert_eq!(c.put(format!("{origin}/v2/vaults/{vid}/blobs/{}", "00".repeat(32))).body(big).send().unwrap().status(), 413);
    let _ = std::fs::remove_dir_all(dir);
}
