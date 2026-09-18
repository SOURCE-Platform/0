//! Client side of the vault IPC boundary (spec §1.4), Phase C version.
//! Used by the main app, by `vault-test-client`, and as the reference for
//! the nm-host port.
//!
//! Connect order is fixed and fail-closed:
//! 1. connect the socket;
//! 2. verify the *helper's* code identity (reverse direction, spec §1.4
//!    item 4) before any byte is sent;
//! 3. `hello` / `hello_ok` handshake carrying protocol major 1.
//!
//! After the handshake a reader thread demultiplexes the connection,
//! because three frame kinds now interleave on it (§1.5/§14.4):
//! - **op responses** → the one outstanding request (ops serialize);
//! - **events** (`{"event": ...}`) → the event queue the caller polls;
//! - **`capture_check`** reverse queries → answered through the
//!   registered handler; with no handler the answer is
//!   `suppressed: false` (fail-closed, §14.4).

use std::io;
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::sync::mpsc::{sync_channel, Receiver, SyncSender};
use std::sync::{Arc, Mutex};

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
    /// The helper vanished mid-request.
    Disconnected,
}

impl std::fmt::Display for ClientError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ClientError::Io(e) => write!(f, "io: {e}"),
            ClientError::Auth(e) => write!(f, "peer authentication: {e}"),
            ClientError::Frame(e) => write!(f, "framing: {e}"),
            ClientError::Protocol(m) => write!(f, "protocol: {m}"),
            ClientError::Server(code) => write!(f, "helper error: {code}"),
            ClientError::Disconnected => write!(f, "helper connection lost"),
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

/// Answers §14.4 `capture_check` queries: is Source capture suppression
/// verifiably active for this surface? Must answer fast (< 500 ms).
pub type CaptureHandler = Arc<dyn Fn(&str) -> bool + Send + Sync>;

struct ReaderShared {
    pending: Mutex<Option<SyncSender<Result<Value, ClientError>>>>,
    capture_handler: Mutex<Option<CaptureHandler>>,
}

pub struct VaultClient {
    writer: Arc<Mutex<UnixStream>>,
    class: ClientClass,
    state: Mutex<String>,
    shared: Arc<ReaderShared>,
    /// Mutex-wrapped so the client stays Sync; exactly one consumer (the
    /// event pump) is the intended pattern.
    events: Mutex<Receiver<Value>>,
    /// Serializes requests: one outstanding op per connection.
    op_lock: Mutex<()>,
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
            return Err(ClientError::Protocol(format!(
                "expected hello_ok, got {resp}"
            )));
        }
        if resp.get("proto").and_then(Value::as_u64) != Some(PROTO_VERSION as u64) {
            return Err(ClientError::Protocol("protocol major mismatch".to_string()));
        }
        let state = resp
            .get("state")
            .and_then(Value::as_str)
            .unwrap_or("unknown")
            .to_string();

        let writer = Arc::new(Mutex::new(stream.try_clone()?));
        let shared = Arc::new(ReaderShared {
            pending: Mutex::new(None),
            capture_handler: Mutex::new(None),
        });
        let (events_tx, events_rx) = sync_channel(256);
        let reader_shared = Arc::clone(&shared);
        let reader_writer = Arc::clone(&writer);
        std::thread::spawn(move || reader_loop(stream, reader_shared, reader_writer, events_tx));

        Ok(VaultClient {
            writer,
            class,
            state: Mutex::new(state),
            shared,
            events: Mutex::new(events_rx),
            op_lock: Mutex::new(()),
        })
    }

    pub fn class(&self) -> ClientClass {
        self.class
    }

    /// State as reported by the most recent hello/response frame.
    pub fn state(&self) -> String {
        self.shared_state().clone()
    }

    fn shared_state(&self) -> std::sync::MutexGuard<'_, String> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Register the §14.4 capture-check handler. Until one is registered,
    /// every query is answered `suppressed: false` (fail-closed).
    pub fn set_capture_handler(&self, handler: CaptureHandler) {
        *self
            .shared
            .capture_handler
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = Some(handler);
    }

    /// Next event frame from the helper (blocking). `None` once the
    /// connection is gone.
    pub fn recv_event(&self) -> Option<Value> {
        self.events
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .recv()
            .ok()
    }

    /// Non-blocking event poll, for UI integration.
    pub fn try_recv_event(&self) -> Option<Value> {
        self.events
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .try_recv()
            .ok()
    }

    /// Send one op frame (arbitrary fields) and wait for its response,
    /// enforcing the `{ok, error}` shape (spec §1.4). Events and reverse
    /// queries arriving in between are handled by the reader thread.
    pub fn request(&self, frame: Value) -> Result<Value, ClientError> {
        let _guard = self.op_lock.lock().unwrap_or_else(|e| e.into_inner());
        let (tx, rx) = sync_channel::<Result<Value, ClientError>>(1);
        *self
            .shared
            .pending
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = Some(tx);
        let write_result = self
            .writer
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let mut writer = write_result;
        let sent = framing::write_frame(&mut *writer, &frame);
        drop(writer);
        sent?;
        let resp = rx.recv().map_err(|_| ClientError::Disconnected)??;
        if let Some(state) = resp.get("state").and_then(Value::as_str) {
            *self.shared_state() = state.to_string();
        }
        match resp.get("ok").and_then(Value::as_bool) {
            Some(true) => Ok(resp),
            Some(false) => Err(ClientError::Server(
                resp.get("error")
                    .and_then(Value::as_str)
                    .unwrap_or("UNKNOWN")
                    .to_string(),
            )),
            None => Err(ClientError::Protocol(format!("response lacks ok: {resp}"))),
        }
    }

    /// Convenience for fieldless ops.
    pub fn request_op(&self, op: &str) -> Result<Value, ClientError> {
        self.request(json!({ "op": op }))
    }

    pub fn get_state(&self) -> Result<String, ClientError> {
        self.request_op("get_state")?;
        Ok(self.state())
    }

    pub fn lock(&self) -> Result<String, ClientError> {
        self.request_op("lock")?;
        Ok(self.state())
    }
}

/// Demultiplex the read side. Runs until the helper closes the
/// connection; wakes any pending request with `Disconnected`.
fn reader_loop(
    mut stream: UnixStream,
    shared: Arc<ReaderShared>,
    writer: Arc<Mutex<UnixStream>>,
    events: SyncSender<Value>,
) {
    loop {
        let frame = match framing::read_frame(&mut stream) {
            Ok(frame) => frame,
            Err(_) => break,
        };
        if frame.get("op").and_then(Value::as_str) == Some("capture_check") {
            answer_capture_check(&shared, &writer, &frame);
        } else if frame.get("event").is_some() {
            let _ = events.send(frame);
        } else {
            let waiter = shared
                .pending
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .take();
            match waiter {
                Some(waiter) => {
                    let _ = waiter.send(Ok(frame));
                }
                None => {
                    eprintln!("vault-client: unsolicited frame dropped: no pending op");
                }
            }
        }
    }
    let waiter = shared
        .pending
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .take();
    if let Some(waiter) = waiter {
        let _ = waiter.send(Err(ClientError::Disconnected));
    }
}

/// Answer a §14.4 reverse query. No handler → `suppressed: false`.
fn answer_capture_check(
    shared: &ReaderShared,
    writer: &Arc<Mutex<UnixStream>>,
    frame: &Value,
) {
    let id = frame.get("id").and_then(Value::as_str).unwrap_or("");
    let surface = frame.get("surface").and_then(Value::as_str).unwrap_or("");
    let suppressed = shared
        .capture_handler
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .as_ref()
        .map(|handler| handler(surface))
        .unwrap_or(false);
    let reply = json!({"reply_to": id, "suppressed": suppressed});
    if let Ok(mut stream) = writer.lock() {
        let _ = framing::write_frame(&mut *stream, &reply);
    }
}
