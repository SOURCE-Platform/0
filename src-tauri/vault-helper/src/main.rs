//! `source-vault-helper` binary entry point.
//!
//! Thread layout (Phase C): the MAIN thread belongs to AppKit — the §1.7
//! secure panel is presented from the main dispatch queue, which only the
//! AppKit event loop drains. The IPC server (accept loop, executor, tick)
//! runs on worker threads. When the server decides the process should
//! exit (idle timeout / shutdown drain, §1.6), it exits the process from
//! the worker thread; VK was already zeroized by the drain-time lock.

use std::process::ExitCode;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy};
use objc2_foundation::MainThreadMarker;

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
    // §2.11: the helper must never dump core (VK zeroization policy).
    vault_helper::crypto::secret::disable_core_dumps();
    install_signal_handlers();

    // AppKit takes the main thread before any worker spawns; panel jobs
    // dispatched to the main queue require this event loop.
    let Some(mtm) = MainThreadMarker::new() else {
        vault_helper::hlog!("vault-helper: must start on the main thread");
        return ExitCode::from(2);
    };
    let app = NSApplication::sharedApplication(mtm);
    // No Dock icon / menu bar: the helper is UI background-only (§1.7 —
    // it owns a panel, not an app presence).
    app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);

    let config = ServerConfig {
        idle_timeout: idle_timeout(),
        ..ServerConfig::default()
    };
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
            vault_helper::hlog!("vault-helper: startup failed: {e}");
            return ExitCode::from(2);
        }
    };
    vault_helper::hlog!(
        "vault-helper: listening (state: {})",
        server.boot_state().as_str()
    );
    std::thread::spawn(move || {
        // The server has finished its drain (which includes the §1.6
        // lock/zeroize); end the process from here — the AppKit loop on
        // the main thread has no other exit path. A panic on this thread
        // must still end the process: otherwise the helper would ignore
        // SIGTERM and never idle-exit (70 = EX_SOFTWARE).
        let code = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| server.run()))
            .unwrap_or(70);
        std::process::exit(code);
    });
    // SAFETY: called once on the main thread after full initialization;
    // standard NSApplication event-loop entry.
    unsafe { app.run() }; // never returns under normal operation
    ExitCode::SUCCESS
}
