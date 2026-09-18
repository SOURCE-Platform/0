//! Shared op protocol pieces (spec §1.5): `hello` parsing, client classes,
//! and response frame shapes.
//!
//! Op dispatch itself lives in `vault::dispatch` (Phase C ops) and
//! `ipc::conn` (`get_state`/`lock`, answered inline so `lock` can preempt
//! in-flight ops per §13.3). Ops beyond the Phase C catalog are answered
//! `UNKNOWN_OP` (internal, non-user-facing; the §15 catalog has no code
//! for "not implemented in this phase").
//!
//! Frame shapes (spec §1.4):
//! - request:  `{"op": "...", ...}`
//! - response: `{"ok": bool, "error": <code|null>, ...}` — no secret
//!   material is ever carried, only codes and human-safe context.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::state::VaultState;
use crate::PROTO_VERSION;

/// Client classes with one connection slot each (spec §1.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ClientClass {
    #[serde(rename = "app")]
    App,
    #[serde(rename = "nm-host")]
    NmHost,
}

impl ClientClass {
    pub fn as_str(self) -> &'static str {
        match self {
            ClientClass::App => "app",
            ClientClass::NmHost => "nm-host",
        }
    }
}

/// Parsed `hello` frame: `{"op":"hello", proto, client}` (spec §1.5).
#[derive(Debug, Deserialize)]
pub struct HelloRequest {
    pub proto: u32,
    pub client: String,
}

pub fn parse_hello(frame: &Value) -> Option<HelloRequest> {
    if frame.get("op")?.as_str()? != "hello" {
        return None;
    }
    serde_json::from_value(frame.clone()).ok()
}

pub fn parse_client_class(raw: &str) -> Option<ClientClass> {
    match raw {
        "app" => Some(ClientClass::App),
        "nm-host" => Some(ClientClass::NmHost),
        _ => None,
    }
}

/// Successful `hello` response (`hello_ok`, spec §1.4).
pub fn hello_ok(state: VaultState) -> Value {
    json!({
        "op": "hello_ok",
        "ok": true,
        "error": null,
        "proto": PROTO_VERSION,
        "state": state.as_str(),
    })
}

pub fn ok_with_state(state: VaultState) -> Value {
    json!({ "ok": true, "error": null, "state": state.as_str() })
}

pub fn err(code: &str) -> Value {
    json!({ "ok": false, "error": code })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hello_parses_valid_frame() {
        let frame = json!({"op": "hello", "proto": 1, "client": "app"});
        let hello = parse_hello(&frame).expect("valid hello");
        assert_eq!(hello.proto, 1);
        assert_eq!(hello.client, "app");
    }

    #[test]
    fn hello_rejects_non_hello_and_missing_fields() {
        assert!(parse_hello(&json!({"op": "get_state"})).is_none());
        assert!(parse_hello(&json!({"op": "hello", "proto": 1})).is_none());
        assert!(parse_hello(&json!({"op": "hello", "proto": "1", "client": "app"})).is_none());
    }

    #[test]
    fn client_classes_are_exactly_app_and_nm_host() {
        assert_eq!(parse_client_class("app"), Some(ClientClass::App));
        assert_eq!(parse_client_class("nm-host"), Some(ClientClass::NmHost));
        assert_eq!(parse_client_class("extension"), None);
        assert!(parse_client_class("").is_none());
    }

    #[test]
    fn hello_ok_shape_matches_spec() {
        let resp = hello_ok(VaultState::Locked);
        assert_eq!(resp["op"], "hello_ok");
        assert_eq!(resp["ok"], true);
        assert_eq!(resp["proto"], 1);
        assert_eq!(resp["state"], "locked");
    }
}
