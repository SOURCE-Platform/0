//! Connection registry and helper→app plumbing (spec §1.4/§1.5/§14.4).
//!
//! The hub owns the per-class connection slots (one per client class; a
//! second `hello` replaces the first) and provides the two reverse
//! channels the vault core needs:
//!
//! - **Events** (`EventSink`): `state`, `locked`, `secure_panel_visible`,
//!   `capture_unsafe` are written to the app connection the moment they
//!   happen. Events are advisory — a disconnected app re-syncs with
//!   `get_state` — so emission is best-effort and never blocks an op.
//! - **Capture check** (`CaptureChecker`): §14.4 requires the helper to
//!   query the main app before any display-class release; the query is a
//!   `capture_check` frame with a correlation id, answered by
//!   `{"reply_to": id, "suppressed": bool}` within 500 ms. No app
//!   connection, write failure, timeout, or malformed answer all resolve
//!   to `false` — refusal is the default.

use std::collections::HashMap;
use std::net::Shutdown;
use std::os::unix::net::UnixStream;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{sync_channel, SyncSender};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use crate::ipc::framing;
use crate::ops::ClientClass;
use crate::vault::{CaptureChecker, EventSink};

/// §14.4: "If the check fails or times out (500 ms), the helper returns
/// CAPTURE_UNSAFE."
const CAPTURE_CHECK_TIMEOUT: Duration = Duration::from_millis(500);

struct SlotEntry {
    conn_id: u64,
    writer: Arc<Mutex<UnixStream>>,
}

struct HubInner {
    slots: HashMap<ClientClass, SlotEntry>,
    active: usize,
    last_zero_clients: Instant,
    next_conn_id: u64,
}

pub struct Hub {
    inner: Mutex<HubInner>,
    pending_capture: Mutex<HashMap<String, SyncSender<bool>>>,
    capture_seq: AtomicU64,
}

impl Default for Hub {
    fn default() -> Self {
        Self::new()
    }
}

impl Hub {
    pub fn new() -> Hub {
        Hub {
            inner: Mutex::new(HubInner {
                slots: HashMap::new(),
                active: 0,
                last_zero_clients: Instant::now(),
                next_conn_id: 0,
            }),
            pending_capture: Mutex::new(HashMap::new()),
            capture_seq: AtomicU64::new(1),
        }
    }

    fn lock(&self) -> MutexGuard<'_, HubInner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn lock_pending(&self) -> MutexGuard<'_, HashMap<String, SyncSender<bool>>> {
        self.pending_capture
            .lock()
            .unwrap_or_else(|e| e.into_inner())
    }

    /// Insert the connection into its class slot, evicting any
    /// predecessor (§1.4). Returns the new connection id.
    pub fn register(&self, class: ClientClass, stream: &UnixStream) -> u64 {
        let mut inner = self.lock();
        inner.next_conn_id += 1;
        let conn_id = inner.next_conn_id;
        if let Some(old) = inner.slots.remove(&class) {
            eprintln!("vault-helper: replacing {} connection", class.as_str());
            let _ = old.writer.lock().map(|w| w.shutdown(Shutdown::Both));
            if class == ClientClass::App {
                drop(inner);
                self.clear_pending_capture();
                inner = self.lock();
            }
        }
        if let Ok(clone) = stream.try_clone() {
            inner.slots.insert(
                class,
                SlotEntry {
                    conn_id,
                    writer: Arc::new(Mutex::new(clone)),
                },
            );
        }
        inner.active += 1;
        conn_id
    }

    pub fn unregister(&self, class: ClientClass, conn_id: u64) {
        let mut inner = self.lock();
        if inner
            .slots
            .get(&class)
            .is_some_and(|s| s.conn_id == conn_id)
        {
            inner.slots.remove(&class);
        }
        inner.active = inner.active.saturating_sub(1);
        if inner.active == 0 {
            inner.last_zero_clients = Instant::now();
        }
        if class == ClientClass::App {
            drop(inner);
            self.clear_pending_capture();
        }
    }

    pub fn active(&self) -> usize {
        self.lock().active
    }

    pub fn zero_clients_since(&self) -> Instant {
        self.lock().last_zero_clients
    }

    fn app_writer(&self) -> Option<Arc<Mutex<UnixStream>>> {
        self.lock()
            .slots
            .get(&ClientClass::App)
            .map(|s| Arc::clone(&s.writer))
    }

    /// Route a `{"reply_to": ..., ...}` frame from the app connection to
    /// a pending reverse query. Returns true when the frame was consumed
    /// (it is not an op and must not reach the executor).
    pub fn route_reply(&self, frame: &Value) -> bool {
        let Some(id) = frame.get("reply_to").and_then(Value::as_str) else {
            return false;
        };
        let Some(waiter) = self.lock_pending().remove(id) else {
            return false;
        };
        let suppressed = frame.get("suppressed").and_then(Value::as_bool).unwrap_or(false);
        let _ = waiter.send(suppressed);
        true
    }

    /// Drop every pending reverse-query waiter without answering; the
    /// waiters' timeouts resolve them to refusal (fail-closed).
    fn clear_pending_capture(&self) {
        self.lock_pending().clear();
    }
}

impl EventSink for Hub {
    fn emit(&self, event: Value) {
        let Some(writer) = self.app_writer() else {
            return;
        };
        if let Ok(mut stream) = writer.lock() {
            let _ = framing::write_frame(&mut *stream, &event);
        };
    }
}

impl CaptureChecker for Hub {
    fn suppressed(&self, surface: &str) -> bool {
        let Some(writer) = self.app_writer() else {
            return false;
        };
        let id = format!("cc-{}", self.capture_seq.fetch_add(1, Ordering::SeqCst));
        let (tx, rx) = sync_channel(1);
        self.lock_pending().insert(id.clone(), tx);
        let query = json!({"op": "capture_check", "id": id, "surface": surface});
        let sent = writer
            .lock()
            .map(|mut stream| framing::write_frame(&mut *stream, &query).is_ok())
            .unwrap_or(false);
        let answer = sent && rx.recv_timeout(CAPTURE_CHECK_TIMEOUT).unwrap_or(false);
        self.lock_pending().remove(&id);
        answer
    }
}
