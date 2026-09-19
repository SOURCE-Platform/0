//! Unix-socket server (spec §1.4) and process lifecycle (spec §1.6).
//!
//! - Socket file lives at `<vault_dir>/helper.sock`, directory 0700,
//!   socket 0600 (same permission model as the main app's vault dir).
//! - Auth runs before any frame is read: getpeereid UID gate, then
//!   SecCode check (see `peer_auth`). Fail-closed: any failure closes
//!   the connection without a response frame.
//! - One connection per client class (`hub`); per-connection reader
//!   threads (`conn`); one global ops executor (`executor`); a 1 s tick
//!   applies the auto-lock and system lock triggers (§1.6, §13.3).
//! - Lifecycle: exit after `idle_timeout` with zero clients; on shutdown,
//!   stop accepting and give connected clients `shutdown_grace` to drain.

use std::io;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::sync_channel;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::ipc::conn::{self, ConnCtx};
use crate::ipc::executor::{self, Inbound};
use crate::ipc::hub::Hub;
use crate::ipc::peer_auth::{self, AuthError};
use crate::notify::LockTriggers;
use crate::panel::HelperPanel;
use crate::state::VaultState;
use crate::vault::{lock_core, Deps, EventSink, LockReason, VaultCore};
use crate::{IDLE_EXIT_SECS, SHUTDOWN_GRACE_SECS};

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

pub struct Server {
    listener: UnixListener,
    config: ServerConfig,
    shutdown: Arc<AtomicBool>,
    ctx: Arc<ConnCtx>,
    #[allow(dead_code)] // held to keep the verifier alive with the server
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

        let core = Arc::new(Mutex::new(VaultCore::boot(config.vault_dir.clone())));
        let hub = Arc::new(Hub::new());
        let panel_cancel = Arc::new(AtomicBool::new(false));
        let triggers = LockTriggers::new();
        crate::notify::install(Arc::clone(&triggers));

        let deps = Deps {
            panel: Arc::new(HelperPanel {
                cancel: Arc::clone(&panel_cancel),
            }),
            la: Arc::new(crate::la::LaPresence),
            capture: hub.clone(),
            events: hub.clone(),
        };
        let (inbound_tx, inbound_rx) = sync_channel::<Inbound>(64);
        executor::spawn(Arc::clone(&core), deps, Arc::clone(&panel_cancel), inbound_rx);
        spawn_tick(
            Arc::clone(&core),
            Arc::clone(&hub),
            Arc::clone(&triggers),
            Arc::clone(&panel_cancel),
            Arc::clone(&shutdown),
        );

        let ctx = Arc::new(ConnCtx {
            hub,
            core,
            inbound: inbound_tx,
            panel_cancel,
            verifier: Arc::clone(&verifier),
        });
        Ok(Server {
            listener,
            config,
            shutdown,
            ctx,
            verifier,
        })
    }

    pub fn boot_state(&self) -> VaultState {
        lock_core(&self.ctx.core).state
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
                crate::hlog!("vault-helper: idle timeout with zero clients, exiting");
                return 0;
            }
            match self.listener.accept() {
                Ok((stream, _)) => {
                    // macOS accepted sockets inherit the listener's
                    // O_NONBLOCK; workers do blocking reads on their own
                    // thread, so force blocking mode here.
                    let _ = stream.set_nonblocking(false);
                    let ctx = Arc::clone(&self.ctx);
                    std::thread::spawn(move || conn::handle_connection(stream, ctx));
                }
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => std::thread::sleep(TICK),
                Err(e) => {
                    crate::hlog!("vault-helper: accept error: {e}");
                    std::thread::sleep(TICK);
                }
            }
        }
    }

    fn idle_expired(&self) -> bool {
        self.ctx.hub.active() == 0
            && self.ctx.hub.zero_clients_since().elapsed() >= self.config.idle_timeout
    }

    /// Stop accepting; zeroize the vault (§1.6 shutdown ⇒ lock), then
    /// wait up to `shutdown_grace` for clients to drain (§1.6: 5 s).
    fn drain_and_exit(&self) -> i32 {
        let events = lock_core(&self.ctx.core).lock(LockReason::Explicit);
        for event in events {
            self.ctx.hub.emit(event);
        }
        let deadline = Instant::now() + self.config.shutdown_grace;
        loop {
            if self.ctx.hub.active() == 0 {
                return 0;
            }
            if Instant::now() >= deadline {
                crate::hlog!("vault-helper: shutdown grace expired with clients attached");
                return 0;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}

/// §1.6 auto-lock + §13.3 system-trigger lock, checked once per second.
/// Zeroize-only work runs here precisely so it can land under an
/// in-flight op (see `executor` module docs).
fn spawn_tick(
    core: Arc<Mutex<VaultCore>>,
    hub: Arc<Hub>,
    triggers: Arc<LockTriggers>,
    panel_cancel: Arc<AtomicBool>,
    shutdown: Arc<AtomicBool>,
) {
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_secs(1));
        if shutdown.load(Ordering::SeqCst) {
            return;
        }
        let reason = match triggers.take() {
            Some(reason) => Some(reason),
            None if lock_core(&core).auto_lock_due() => Some(LockReason::Timeout),
            None => None,
        };
        if let Some(reason) = reason {
            panel_cancel.store(true, Ordering::SeqCst);
            let events = lock_core(&core).lock(reason);
            for event in events {
                hub.emit(event);
            }
        }
    });
}
