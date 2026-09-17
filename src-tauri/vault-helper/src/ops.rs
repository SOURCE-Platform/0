//! Phase A op catalog (spec §1.5): `hello`, `get_state`, `lock`.
//!
//! Everything else in the §1.5 catalog belongs to later phases and is
//! answered `UNKNOWN_OP`. The §15 user-facing error catalog has no code for
//! "not implemented in this phase"; `UNKNOWN_OP` is an internal,
//! non-user-facing code used only by this skeleton and documented in the
//! Phase A verification report.
//!
//! Frame shapes (spec §1.4):
//! - request:  `{"op": "...", ...}`
//! - response: `{"ok": bool, "error": <code|null>, ...}` — no secret
//!   material is ever carried, only codes and human-safe context.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::state::{self, VaultState};
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

/// Dispatch one post-hello op frame against the current state.
/// Pure function (no I/O, no globals) so the op surface is directly
/// unit-testable. Returns (response, new state).
pub fn dispatch_op(state: VaultState, frame: &Value) -> (Value, VaultState) {
    let op = frame.get("op").and_then(Value::as_str).unwrap_or("");
    match op {
        "get_state" => (ok_with_state(state), state),
        "lock" => {
            let next = state::apply_lock(state);
            (ok_with_state(next), next)
        }
        _ => (err("UNKNOWN_OP"), state),
    }
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
    fn get_state_and_lock_behave() {
        let (resp, next) = dispatch_op(VaultState::Locked, &json!({"op": "get_state"}));
        assert_eq!(resp["ok"], true);
        assert_eq!(resp["state"], "locked");
        assert_eq!(next, VaultState::Locked);

        let (resp, next) = dispatch_op(VaultState::Uninitialized, &json!({"op": "lock"}));
        assert_eq!(resp["ok"], true);
        assert_eq!(resp["state"], "uninitialized");
        assert_eq!(next, VaultState::Uninitialized);
    }

    #[test]
    fn unknown_ops_are_refused() {
        for op in ["unlock", "reveal", "export_vault", "sign_backup_request", "", "HELLO"] {
            let (resp, next) = dispatch_op(VaultState::Locked, &json!({"op": op}));
            assert_eq!(resp["ok"], false, "op {op}");
            assert_eq!(resp["error"], "UNKNOWN_OP");
            assert_eq!(next, VaultState::Locked);
        }
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
