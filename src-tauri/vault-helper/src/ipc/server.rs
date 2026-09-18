//! Unix-socket server (spec §1.4) and process lifecycle (spec §1.6).
//!
//! - Socket file lives at `<vault_dir>/helper.sock`, directory 0700,
//!   socket 0600 (same permission model as the main app's vault dir).
//! - One connection per client class (app, nm-host); a second `hello` from
//!   the same class replaces the first.
//! - Auth runs before any frame is read: getpeereid UID gate, then SecCode
//!   check (see `peer_auth`). Fail-closed: any failure closes the
//!   connection without a response frame.
//! - Lifecycle: exit after `idle_timeout` with zero clients; on shutdown,
//!   stop accepting and give connected clients `shutdown_grace` to drain.

use std::collections::HashMap;
use std::io;
use std::net::Shutdown;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::ipc::framing::{self, FrameError};
use crate::ipc::peer_auth::{self, AuthError};
use crate::ops::{self, ClientClass};
use crate::state::{self, VaultState};
use crate::{IDLE_EXIT_SECS, SHUTDOWN_GRACE_SECS};
use std::path::PathBuf;

/// Injected so tests can exercise the full connection path without code
/// signing; production binaries always pass `peer_auth::verify_client`.
pub type Verifier = Arc<dyn Fn(&UnixStream) -> Result<(), AuthError> + Send + Sync>;

#[derive(Clone)]
pub struct ServerConfig {
    pub socket_path: PathBuf,
    pub vault_dir: PathBuf,
    pub idle_timeout: Duration,
    pub shutdown_grace: Duration,
}

impl Default for ServerConfig {
    fn default() -> Self {
        ServerConfig {
            socket_path: crate::socket_path(),
            vault_dir: crate::vault_dir(),
            idle_timeout: Duration::from_secs(IDLE_EXIT_SECS),
            shutdown_grace: Duration::from_secs(SHUTDOWN_GRACE_SECS),
        }
    }
}

struct SlotEntry {
    conn_id: u64,
    stream: UnixStream,
}

struct Shared {
    vault_state: VaultState,
    slots: HashMap<ClientClass, SlotEntry>,
    active: usize,
    last_zero_clients: Instant,
    next_conn_id: u64,
}

impl Shared {
    fn lock(shared: &Mutex<Shared>) -> std::sync::MutexGuard<'_, Shared> {
        shared.lock().unwrap_or_else(|e| e.into_inner())
    }
}

pub struct Server {
    listener: UnixListener,
    config: ServerConfig,
    shared: Arc<Mutex<Shared>>,
    shutdown: Arc<AtomicBool>,
    verifier: Verifier,
}

impl Server {
    pub fn new(config: ServerConfig, shutdown: Arc<AtomicBool>) -> io::Result<Server> {
        Self::with_verifier(config, shutdown, Arc::new(peer_auth::verify_client))
    }

    pub fn with_verifier(
        config: ServerConfig,
        shutdown: Arc<AtomicBool>,
        verifier: Verifier,
    ) -> io::Result<Server> {
        std::fs::create_dir_all(&config.vault_dir)?;
        std::fs::set_permissions(&config.vault_dir, std::fs::Permissions::from_mode(0o700))?;
        if config.socket_path.exists() {
            std::fs::remove_file(&config.socket_path)?; // stale socket from a prior run
        }
        let listener = UnixListener::bind(&config.socket_path)?;
        std::fs::set_permissions(&config.socket_path, std::fs::Permissions::from_mode(0o600))?;
        listener.set_nonblocking(true)?;
        let shared = Arc::new(Mutex::new(Shared {
            vault_state: state::detect_boot_state(&config.vault_dir),
            slots: HashMap::new(),
            active: 0,
            last_zero_clients: Instant::now(),
            next_conn_id: 0,
        }));
        Ok(Server {
            listener,
            config,
            shared,
            shutdown,
            verifier,
        })
    }

    pub fn boot_state(&self) -> VaultState {
        Shared::lock(&self.shared).vault_state
    }

    /// Accept loop: runs until `shutdown` is set or the idle timeout
    /// elapses with zero clients. Returns the intended process exit code.
    pub fn run(self) -> i32 {
        const TICK: Duration = Duration::from_millis(100);
        loop {
            if self.shutdown.load(Ordering::SeqCst) {
                return self.drain_and_exit();
            }
            if self.idle_expired() {
                eprintln!("vault-helper: idle timeout with zero clients, exiting");
                return 0;
            }
            match self.listener.accept() {
                Ok((stream, _)) => {
                    // macOS accepted sockets inherit the listener's
                    // O_NONBLOCK; workers do blocking reads on their own
                    // thread, so force blocking mode here.
                    let _ = stream.set_nonblocking(false);
                    self.spawn_worker(stream);
                }
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => std::thread::sleep(TICK),
                Err(e) => {
                    eprintln!("vault-helper: accept error: {e}");
                    std::thread::sleep(TICK);
                }
            }
        }
    }

    fn idle_expired(&self) -> bool {
        let shared = Shared::lock(&self.shared);
        shared.active == 0 && shared.last_zero_clients.elapsed() >= self.config.idle_timeout
    }

    /// Stop accepting; wait up to `shutdown_grace` for clients to drain,
    /// then exit regardless (spec §1.6: 5 s grace).
    fn drain_and_exit(&self) -> i32 {
        let deadline = Instant::now() + self.config.shutdown_grace;
        loop {
            if Shared::lock(&self.shared).active == 0 {
                return 0;
            }
            if Instant::now() >= deadline {
                eprintln!("vault-helper: shutdown grace expired with clients attached");
                return 0;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    fn spawn_worker(&self, stream: UnixStream) {
        let shared = Arc::clone(&self.shared);
        let verifier = Arc::clone(&self.verifier);
        std::thread::spawn(move || handle_connection(stream, shared, verifier));
    }
}

fn handle_connection(mut stream: UnixStream, shared: Arc<Mutex<Shared>>, verifier: Verifier) {
    if let Err(e) = verifier(&stream) {
        eprintln!("vault-helper: peer authentication failed: {e}");
        return;
    }
    let Some((class, hello_ok)) = read_hello(&mut stream, &shared) else {
        eprintln!("vault-helper: first frame was not a valid hello; closing");
        return;
    };
    let conn_id = register(&shared, class, &stream);
    if framing::write_frame(&mut stream, &hello_ok).is_err() {
        unregister(&shared, class, conn_id);
        return;
    }
    serve_ops(&mut stream, &shared);
    unregister(&shared, class, conn_id);
}

/// The first frame must be a spec-conformant `hello` (proto 1, known
/// client class); anything else closes the connection without a response
/// (protocol major mismatch is a disconnect per spec §1.4).
fn read_hello(
    stream: &mut UnixStream,
    shared: &Arc<Mutex<Shared>>,
) -> Option<(ClientClass, serde_json::Value)> {
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
    let state = Shared::lock(shared).vault_state;
    Some((class, ops::hello_ok(state)))
}

/// Insert the connection into its class slot, evicting any predecessor
/// (spec §1.4: one connection per class, second hello replaces the first).
fn register(shared: &Arc<Mutex<Shared>>, class: ClientClass, stream: &UnixStream) -> u64 {
    let mut shared = Shared::lock(shared);
    shared.next_conn_id += 1;
    let conn_id = shared.next_conn_id;
    if let Some(old) = shared.slots.remove(&class) {
        eprintln!("vault-helper: replacing {} connection", class.as_str());
        let _ = old.stream.shutdown(Shutdown::Both);
    }
    if let Ok(clone) = stream.try_clone() {
        shared.slots.insert(
            class,
            SlotEntry {
                conn_id,
                stream: clone,
            },
        );
    }
    shared.active += 1;
    conn_id
}

fn unregister(shared: &Arc<Mutex<Shared>>, class: ClientClass, conn_id: u64) {
    let mut shared = Shared::lock(shared);
    if shared
        .slots
        .get(&class)
        .is_some_and(|s| s.conn_id == conn_id)
    {
        shared.slots.remove(&class);
    }
    shared.active = shared.active.saturating_sub(1);
    if shared.active == 0 {
        shared.last_zero_clients = Instant::now();
    }
}

fn serve_ops(stream: &mut UnixStream, shared: &Arc<Mutex<Shared>>) {
    loop {
        match framing::read_frame(stream) {
            Ok(frame) => {
                let response = {
                    let mut shared = Shared::lock(shared);
                    let (response, next) = ops::dispatch_op(shared.vault_state, &frame);
                    shared.vault_state = next;
                    response
                };
                if framing::write_frame(stream, &response).is_err() {
                    return;
                }
            }
            Err(FrameError::Eof) => return,
            Err(e) => {
                // Oversize / malformed: fail-closed per spec §1.4.
                eprintln!("vault-helper: framing violation ({e}); closing connection");
                return;
            }
        }
    }
}
