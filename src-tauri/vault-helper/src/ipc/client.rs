//! Client side of the vault IPC boundary (spec §1.4). The main app and the
//! nm-host use this (or a port of it) to reach the helper; the Phase A gate
//! uses it from `vault-test-client`.
//!
//! Connect order is fixed and fail-closed:
//! 1. connect the socket;
//! 2. verify the *helper's* code identity (reverse direction, spec §1.4
//!    item 4) before any byte is sent;
//! 3. `hello` / `hello_ok` handshake carrying protocol major 1;
//! 4. one request → one response per op.

use std::io;
use std::os::unix::net::UnixStream;
use std::path::Path;

use serde_json::{json, Value};

use crate::ipc::framing::{self, FrameError};
use crate::ipc::peer_auth::{self, AuthError};
use crate::ops::ClientClass;
use crate::PROTO_VERSION;

#[derive(Debug)]
pub enum ClientError {
    Io(io::Error),
    Auth(AuthError),
    Frame(FrameError),
    /// hello_ok was malformed or carried a protocol mismatch.
    Protocol(String),
    /// The helper answered `{ok:false}`; carries the error code.
    Server(String),
}

impl std::fmt::Display for ClientError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ClientError::Io(e) => write!(f, "io: {e}"),
            ClientError::Auth(e) => write!(f, "peer authentication: {e}"),
            ClientError::Frame(e) => write!(f, "framing: {e}"),
            ClientError::Protocol(m) => write!(f, "protocol: {m}"),
            ClientError::Server(code) => write!(f, "helper error: {code}"),
        }
    }
}

impl std::error::Error for ClientError {}

impl From<io::Error> for ClientError {
    fn from(e: io::Error) -> Self {
        ClientError::Io(e)
    }
}
impl From<AuthError> for ClientError {
    fn from(e: AuthError) -> Self {
        ClientError::Auth(e)
    }
}
impl From<FrameError> for ClientError {
    fn from(e: FrameError) -> Self {
        ClientError::Frame(e)
    }
}

pub struct VaultClient {
    stream: UnixStream,
    class: ClientClass,
    state: String,
}

impl VaultClient {
    /// Connect, authenticate the helper, and complete the hello handshake.
    pub fn connect(socket_path: &Path, class: ClientClass) -> Result<Self, ClientError> {
        let mut stream = UnixStream::connect(socket_path)?;
        peer_auth::verify_helper(&stream)?;
        let hello = json!({
            "op": "hello",
            "proto": PROTO_VERSION,
            "client": class.as_str(),
        });
        framing::write_frame(&mut stream, &hello)?;
        let resp = framing::read_frame(&mut stream)?;
        if resp.get("op").and_then(Value::as_str) != Some("hello_ok")
            || resp.get("ok").and_then(Value::as_bool) != Some(true)
        {
            return Err(ClientError::Protocol(format!("expected hello_ok, got {resp}")));
        }
        if resp.get("proto").and_then(Value::as_u64) != Some(PROTO_VERSION as u64) {
            return Err(ClientError::Protocol("protocol major mismatch".to_string()));
        }
        let state = resp
            .get("state")
            .and_then(Value::as_str)
            .unwrap_or("unknown")
            .to_string();
        Ok(VaultClient { stream, class, state })
    }

    pub fn class(&self) -> ClientClass {
        self.class
    }

    /// State as reported by the most recent hello/get_state/lock.
    pub fn state(&self) -> &str {
        &self.state
    }

    /// Send one op frame and return the response, enforcing
    /// `{ok, error}` shape (spec §1.4).
    pub fn request(&mut self, op: &str) -> Result<Value, ClientError> {
        framing::write_frame(&mut self.stream, &json!({ "op": op }))?;
        let resp = framing::read_frame(&mut self.stream)?;
        match resp.get("ok").and_then(Value::as_bool) {
            Some(true) => {
                if let Some(state) = resp.get("state").and_then(Value::as_str) {
                    self.state = state.to_string();
                }
                Ok(resp)
            }
            Some(false) => Err(ClientError::Server(
                resp.get("error")
                    .and_then(Value::as_str)
                    .unwrap_or("UNKNOWN")
                    .to_string(),
            )),
            None => Err(ClientError::Protocol(format!("response lacks ok: {resp}"))),
        }
    }

    pub fn get_state(&mut self) -> Result<String, ClientError> {
        self.request("get_state")?;
        Ok(self.state.clone())
    }

    pub fn lock(&mut self) -> Result<String, ClientError> {
        self.request("lock")?;
        Ok(self.state.clone())
    }
}
