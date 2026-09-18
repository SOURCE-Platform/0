//! Per-connection service loop (spec §1.4). Each accepted connection gets
//! one reader thread that authenticates the peer, performs the `hello`
//! handshake, and then routes frames:
//!
//! - `{"reply_to": ...}` on the app connection → pending reverse queries
//!   (`capture_check`), never the executor. These are routed by a
//!   dedicated read-side thread: the op loop blocks on the executor for
//!   the op's whole duration, and a `reveal` is itself waiting on the
//!   capture-check reply — routing it from the op loop would deadlock
//!   until the 500 ms check timed out (every reveal → CAPTURE_UNSAFE).
//!   Late replies (after the timeout) are consumed, never run as ops.
//! - `get_state` / `lock` → answered inline (read-only / zeroize-only;
//!   `lock` must preempt in-flight ops, §13.3, so it never queues behind
//!   a 120 s panel).
//! - everything else → the single ops executor; the reader blocks on the
//!   response, which preserves per-connection request/response ordering.

use std::net::Shutdown;
use std::os::unix::net::UnixStream;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, sync_channel, Sender, SyncSender};
use std::sync::{Arc, Mutex};

use serde_json::Value;

use crate::ipc::executor::Inbound;
use crate::ipc::framing::{self, FrameError};
use crate::ipc::hub::Hub;
use crate::ipc::peer_auth::AuthError;
use crate::ops::{self, ClientClass};
use crate::vault::{lock_core, EventSink, LockReason, VaultCore};

/// Everything a connection thread needs from the server.
pub struct ConnCtx {
    pub hub: Arc<Hub>,
    pub core: Arc<Mutex<VaultCore>>,
    pub inbound: SyncSender<Inbound>,
    pub panel_cancel: Arc<AtomicBool>,
    pub verifier: Arc<dyn Fn(&UnixStream) -> Result<(), AuthError> + Send + Sync>,
}

pub fn handle_connection(mut stream: UnixStream, ctx: Arc<ConnCtx>) {
    if let Err(e) = (ctx.verifier)(&stream) {
        eprintln!("vault-helper: peer authentication failed: {e}");
        return;
    }
    let Some((class, hello_ok)) = read_hello(&mut stream, &ctx) else {
        eprintln!("vault-helper: first frame was not a valid hello; closing");
        return;
    };
    let conn_id = ctx.hub.register(class, &stream);
    if framing::write_frame(&mut stream, &hello_ok).is_err() {
        ctx.hub.unregister(class, conn_id);
        return;
    }
    serve(&mut stream, class, &ctx);
    ctx.hub.unregister(class, conn_id);
}

/// The first frame must be a spec-conformant `hello` (proto 1, known
/// client class); anything else closes the connection without a response
/// (protocol major mismatch is a disconnect per spec §1.4).
fn read_hello(stream: &mut UnixStream, ctx: &ConnCtx) -> Option<(ClientClass, Value)> {
    let frame = framing::read_frame(stream).ok()?;
    let hello = ops::parse_hello(&frame)?;
    if hello.proto != crate::PROTO_VERSION {
        eprintln!(
            "vault-helper: protocol major mismatch ({}), closing",
            hello.proto
        );
        return None;
    }
    let class = ops::parse_client_class(&hello.client)?;
    let state = lock_core(&ctx.core).state;
    Some((class, ops::hello_ok(state)))
}

fn serve(stream: &mut UnixStream, class: ClientClass, ctx: &Arc<ConnCtx>) {
    let Ok(read_side) = stream.try_clone() else {
        return;
    };
    let (frames_tx, frames_rx) = channel::<Value>();
    let reader_ctx = Arc::clone(ctx);
    let reader = std::thread::spawn(move || read_loop(read_side, class, &reader_ctx, frames_tx));
    for frame in frames_rx {
        let op = frame.get("op").and_then(Value::as_str).unwrap_or("");
        let response = match op {
            "get_state" => ops::ok_with_state(lock_core(&ctx.core).state),
            "lock" => do_lock(ctx),
            _ => match forward(ctx, frame) {
                Some(response) => response,
                None => break, // executor gone: server is shutting down
            },
        };
        if framing::write_frame(stream, &response).is_err() {
            break;
        }
    }
    // Unblock the read side (if we left first) and reap it.
    let _ = stream.shutdown(Shutdown::Both);
    let _ = reader.join();
}

/// Read side: route reverse-query replies immediately, queue everything
/// else for the op loop in arrival order. Ends on EOF or a framing
/// violation (fail-closed per spec §1.4), which closes the queue.
fn read_loop(mut stream: UnixStream, class: ClientClass, ctx: &ConnCtx, frames: Sender<Value>) {
    loop {
        let frame = match framing::read_frame(&mut stream) {
            Ok(frame) => frame,
            Err(FrameError::Eof) => return,
            Err(e) => {
                eprintln!("vault-helper: framing violation ({e}); closing connection");
                return;
            }
        };
        if class == ClientClass::App && frame.get("reply_to").is_some() {
            ctx.hub.route_reply(&frame);
            continue;
        }
        if frames.send(frame).is_err() {
            return;
        }
    }
}

/// Explicit `lock` (§1.5): preempt any visible panel, zeroize (§13.3),
/// emit the events live, answer with the resulting state.
fn do_lock(ctx: &ConnCtx) -> Value {
    ctx.panel_cancel.store(true, Ordering::SeqCst);
    let events = lock_core(&ctx.core).lock(LockReason::Explicit);
    for event in events {
        ctx.hub.emit(event);
    }
    ops::ok_with_state(lock_core(&ctx.core).state)
}

/// Hand one op to the single executor and wait for its response. The
/// wait is unbounded: ops own their own timeouts (panel 120 s, LA 90 s,
/// capture 500 ms) and `lock` zeroizes state from the outside rather
/// than interrupting the queue.
fn forward(ctx: &ConnCtx, frame: Value) -> Option<Value> {
    let (tx, rx) = sync_channel(1);
    ctx.inbound.send(Inbound { frame, reply: tx }).ok()?;
    rx.recv().ok()
}
