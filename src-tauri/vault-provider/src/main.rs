//! `vault-provider` — the credential-vault backup service (spec v0.4 §1.1,
//! §11). Untrusted for confidentiality and not a root of trust: it stores
//! ciphertext and public data and enforces structure as defense in depth.
//!
//! - `vault-provider serve` — the HTTP service (plain HTTP; TLS is
//!   terminated by the hosting platform in front of it).
//! - `vault-provider gc` — one retention pass over every vault (§11.2);
//!   run it on a schedule (e.g. daily).

use std::net::SocketAddr;

use vault_provider::config::Config;
use vault_provider::{build, gc, router};

#[tokio::main]
async fn main() {
    let cfg = match Config::from_env() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("vault-provider: {e}");
            std::process::exit(2);
        }
    };
    if std::env::args().nth(1).as_deref() == Some("gc") {
        let r = tokio::task::spawn_blocking(move || gc(cfg)).await.unwrap_or(Err("gc panicked".into()));
        if let Err(e) = r {
            eprintln!("vault-provider: {e}");
            std::process::exit(1);
        }
        return;
    }
    let (port, origin) = (cfg.port, cfg.origin.clone());
    let (p, _) = build(cfg);
    let app = router(p);
    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    let listener = tokio::net::TcpListener::bind(addr).await.expect("bind");
    eprintln!("vault-provider listening on {addr} as {origin}");
    axum::serve(listener, app.into_make_service_with_connect_info::<SocketAddr>())
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await
        .expect("serve");
}
