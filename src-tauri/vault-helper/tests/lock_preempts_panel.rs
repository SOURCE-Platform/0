//! Explicit `lock` preempts an in-flight panel op on the SAME connection
//! (Phase C.1 regression, spec §13.3) — real socket, real AppKit panel.
//!
//! Before the fix, `lock` queued behind the open panel on the connection's
//! sequential op loop (and behind the client's one-request-at-a-time
//! lock), so it landed only when the panel closed — up to the 120 s panel
//! timeout. Required now: the helper enters LOCKED at once, the panel is
//! aborted promptly (its fields zeroized by the abort path), the panel op
//! ends PANEL_CANCELLED, and `secure_panel_visible:false` — which releases
//! the app's capture suppression — is emitted only after the panel has
//! actually left the screen.
//!
//! `harness = false`: AppKit needs the process main thread, which the
//! libtest harness keeps for itself. The test shows a real panel for
//! about half a second, so it runs only with OV0_VAULT_APPKIT_TESTS=1
//! (scripts/phase-c-gate.sh sets it); plain `cargo test` reports a skip.

use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::{channel, Receiver};
use std::sync::Arc;
use std::time::{Duration, Instant};

use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy};
use objc2_foundation::MainThreadMarker;
use serde_json::{json, Value};
use vault_helper::ipc::framing;
use vault_helper::ipc::server::{Server, ServerConfig};
use vault_helper::keychain;
use vault_helper::panel::appkit::panel_on_screen;

const MP: &str = "synthetic-lockpreempt-master-password-0001";

fn main() {
    if std::env::var("OV0_VAULT_APPKIT_TESTS").as_deref() != Ok("1") {
        println!("lock_preempts_panel: skipped (set OV0_VAULT_APPKIT_TESTS=1; shows a real panel)");
        return;
    }
    let mtm = MainThreadMarker::new().expect("test main runs on the main thread");
    let app = NSApplication::sharedApplication(mtm);
    app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);
    std::thread::spawn(|| {
        let result = std::panic::catch_unwind(scenario);
        cleanup();
        match result {
            Ok(()) => {
                println!("lock_preempts_panel: PASS");
                std::process::exit(0);
            }
            Err(_) => {
                println!("lock_preempts_panel: FAIL");
                std::process::exit(1);
            }
        }
    });
    // SAFETY: main thread, after setup; the scenario thread exits the process.
    unsafe { app.run() };
}

fn dir() -> PathBuf {
    PathBuf::from(format!("/tmp/vhlp{}", std::process::id()))
}

fn cleanup() {
    keychain::delete_item("com.racker.zero.vault.state");
    keychain::delete_item("com.racker.zero.vault.helper-prefs");
    let _ = std::fs::remove_dir_all(dir());
}

/// Frames from the helper, read on a separate thread so the scenario can
/// write `lock` while the unlock request is still outstanding.
fn spawn_reader(stream: &UnixStream) -> Receiver<Value> {
    let mut read_side = stream.try_clone().unwrap();
    let (tx, rx) = channel();
    std::thread::spawn(move || {
        while let Ok(frame) = framing::read_frame(&mut read_side) {
            if tx.send(frame).is_err() {
                return;
            }
        }
    });
    rx
}

fn next(rx: &Receiver<Value>, within: Duration, what: &str) -> Value {
    rx.recv_timeout(within)
        .unwrap_or_else(|_| panic!("timed out after {within:?} waiting for {what}"))
}

/// Next non-event frame (an op response); events on the way are passed to
/// `on_event`.
fn response(rx: &Receiver<Value>, within: Duration, mut on_event: impl FnMut(&Value)) -> Value {
    let deadline = Instant::now() + within;
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        let frame = next(rx, left, "an op response");
        if frame.get("event").is_some() {
            on_event(&frame);
        } else {
            return frame;
        }
    }
}

fn scenario() {
    std::env::set_var("OV0_VAULT_KEYCHAIN_PREFIX", format!("ov0lockp-{}-", std::process::id()));
    cleanup();
    std::fs::create_dir_all(dir()).unwrap();
    let socket = dir().join("s");
    let cfg = ServerConfig {
        socket_path: socket.clone(),
        vault_dir: dir().join("v"),
        idle_timeout: Duration::from_secs(3600),
        shutdown_grace: Duration::from_millis(500),
    };
    let server =
        Server::with_verifier(cfg, Arc::new(AtomicBool::new(false)), Arc::new(|_| Ok(()))).unwrap();
    std::thread::spawn(move || server.run());
    for _ in 0..100 {
        if socket.exists() {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }

    let mut s = UnixStream::connect(&socket).unwrap();
    let rx = spawn_reader(&s);
    framing::write_frame(&mut s, &json!({"op": "hello", "proto": 1, "client": "app"})).unwrap();
    assert_eq!(next(&rx, Duration::from_secs(5), "hello_ok")["op"], "hello_ok");

    // Create the vault headlessly (scripted panel), then use the REAL panel.
    std::env::set_var("OV0_VAULT_PANEL_SCRIPT", format!("submit:{MP}"));
    framing::write_frame(&mut s, &json!({"op": "setup_vault"})).unwrap();
    let setup = response(&rx, Duration::from_secs(60), |_| {});
    assert_eq!(setup["ok"], true, "setup: {setup}");
    std::env::remove_var("OV0_VAULT_PANEL_SCRIPT");

    framing::write_frame(&mut s, &json!({"op": "begin_recovery_unlock", "kind": "mp"})).unwrap();
    // Wait for the real panel to be on screen.
    let shown = Instant::now() + Duration::from_secs(10);
    while !panel_on_screen() {
        assert!(Instant::now() < shown, "real panel never appeared");
        std::thread::sleep(Duration::from_millis(20));
    }
    std::thread::sleep(Duration::from_millis(300));

    // Lock on the same connection while the unlock op is in flight.
    let t0 = Instant::now();
    framing::write_frame(&mut s, &json!({"op": "lock"})).unwrap();

    let mut locked_event_at = None;
    let mut hidden_event_at = None;
    let mut panel_up_when_hidden = false;
    let unlock = response(&rx, Duration::from_secs(10), |event| {
        if event["event"] == "locked" && locked_event_at.is_none() {
            locked_event_at = Some(t0.elapsed());
        }
        if event["event"] == "secure_panel_visible" && event["visible"] == false {
            hidden_event_at = Some(t0.elapsed());
            panel_up_when_hidden = panel_on_screen();
        }
    });
    let unlock_at = t0.elapsed();
    let lock = response(&rx, Duration::from_secs(5), |_| {});

    println!(
        "locked event +{:?}; panel hidden event +{:?}; unlock response +{:?}; lock response {}",
        locked_event_at, hidden_event_at, unlock_at, lock
    );
    let locked_at = locked_event_at.expect("no `locked` event");
    assert!(locked_at < Duration::from_secs(1), "lock applied late: {locked_at:?}");
    assert_eq!(unlock["error"], "PANEL_CANCELLED", "unlock: {unlock}");
    assert!(unlock_at < Duration::from_secs(5), "panel not aborted promptly: {unlock_at:?}");
    assert!(hidden_event_at.is_some(), "no secure_panel_visible:false event");
    assert!(!panel_up_when_hidden, "visible:false emitted while the panel was still on screen");
    assert!(!panel_on_screen());
    assert_eq!(lock["ok"], true, "lock: {lock}");
    assert_eq!(lock["state"], "locked", "lock: {lock}");
}
