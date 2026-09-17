//! `source-vault-helper` binary entry point (Phase A, spec §18).
//!
//! Wires the lifecycle spec §1.6 requires: boot state detection, signal
//! driven shutdown with client-drain grace, and idle exit. All heavy
//! lifting lives in the library; this file stays thin.

use std::process::ExitCode;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use vault_helper::ipc::server::{Server, ServerConfig};

static SHUTDOWN: AtomicBool = AtomicBool::new(false);

/// SIGTERM/SIGINT handler: flips the shutdown flag only.
/// Async-signal-safe: an atomic store, nothing else.
extern "C" fn handle_signal(_sig: libc::c_int) {
    SHUTDOWN.store(true, Ordering::SeqCst);
}

fn install_signal_handlers() {
    // SAFETY: `handle_signal` is a valid C-ABI function performing a single
    // async-signal-safe atomic store; sigaction structs are zero-initialized
    // then fully specified (handler + no flags + default mask).
    unsafe {
        let mut action: libc::sigaction = std::mem::zeroed();
        action.sa_sigaction = handle_signal as *const () as usize;
        libc::sigemptyset(&mut action.sa_mask);
        libc::sigaction(libc::SIGTERM, &action, std::ptr::null_mut());
        libc::sigaction(libc::SIGINT, &action, std::ptr::null_mut());
    }
}

/// Debug-only idle-timeout override (seconds) so the gate can exercise the
/// 30-minute zero-client exit without waiting. Compiled out in release:
/// release always uses the spec value.
fn idle_timeout() -> std::time::Duration {
    #[cfg(debug_assertions)]
    if let Ok(secs) = std::env::var("OV0_VAULT_IDLE_SECS") {
        if let Ok(secs) = secs.parse::<u64>() {
            return std::time::Duration::from_secs(secs);
        }
    }
    std::time::Duration::from_secs(vault_helper::IDLE_EXIT_SECS)
}

fn main() -> ExitCode {
    install_signal_handlers();
    let mut config = ServerConfig::default();
    config.idle_timeout = idle_timeout();
    let shutdown = Arc::new(AtomicBool::new(false));
    // Bridge the process-wide handler flag into the server's Arc.
    {
        let shutdown = Arc::clone(&shutdown);
        std::thread::spawn(move || loop {
            if SHUTDOWN.load(Ordering::SeqCst) {
                shutdown.store(true, Ordering::SeqCst);
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        });
    }
    let server = match Server::new(config, shutdown) {
        Ok(server) => server,
        Err(e) => {
            eprintln!("vault-helper: startup failed: {e}");
            return ExitCode::from(2);
        }
    };
    eprintln!("vault-helper: listening (state: {})", server.boot_state().as_str());
    let code = server.run();
    ExitCode::from(code as u8)
}
