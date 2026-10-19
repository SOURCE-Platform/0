use axum::{
    http::{HeaderValue, StatusCode},
    response::{Html, IntoResponse, Response},
};
use base64::{engine::general_purpose::STANDARD, Engine};
use sha2::{Digest, Sha256};

// Clear authorization parameters from the visible URL and current history entry.
const SCRIPT: &str = r#"history.replaceState(null, "", "/banking/callback");"#;
const STYLE: &str = "body{margin:0;padding:4rem 1.25rem;font:1rem/1.65 system-ui,sans-serif}main{max-width:32rem;margin:auto}p{max-width:60ch}";

pub(crate) fn response(status: StatusCode, message: &'static str) -> Response {
    // The message is selected by the callback handler; no request values enter HTML.
    let body = format!(
        "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\">\
         <meta name=\"viewport\" content=\"width=device-width,initial-scale=1\">\
         <title>SOURCE bank authorization</title><style>{STYLE}</style></head>\
         <body><main><h1>SOURCE bank authorization</h1><p>{message}</p></main>\
         <script>{SCRIPT}</script></body></html>"
    );
    let mut response = (status, Html(body)).into_response();
    for (name, value) in [
        ("cache-control", "no-store, max-age=0"),
        ("pragma", "no-cache"),
        ("referrer-policy", "no-referrer"),
        ("x-content-type-options", "nosniff"),
        ("x-frame-options", "DENY"),
        ("x-robots-tag", "noindex, nofollow, noarchive"),
    ] {
        response
            .headers_mut()
            .insert(name, HeaderValue::from_static(value));
    }
    let script_hash = STANDARD.encode(Sha256::digest(SCRIPT.as_bytes()));
    let style_hash = STANDARD.encode(Sha256::digest(STYLE.as_bytes()));
    let csp = format!(
        "default-src 'none'; script-src 'sha256-{script_hash}'; \
         style-src 'sha256-{style_hash}'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'"
    );
    response.headers_mut().insert(
        "content-security-policy",
        HeaderValue::from_str(&csp).expect("fixed callback policy is valid ASCII"),
    );
    response
}
