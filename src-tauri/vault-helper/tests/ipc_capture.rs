//! §14.4 capture_check reverse query over a real socket, on the SAME app
//! connection that issued the `reveal` — the production shape. Regression
//! for a deadlock where the connection's op loop, blocked on the executor
//! for the reveal, could not read the app's capture_check reply: every
//! reveal timed out to CAPTURE_UNSAFE and the late reply was then run as
//! an op. Op-level tests stub the capture checker and cannot see this.
//!
//! Drives setup/unlock through the debug-only scripted panel and LA stub
//! (env, process-global — hence its own test binary), with an isolated
//! keychain prefix. Synthetic credentials only.

use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::Duration;

use serde_json::{json, Value};
use vault_helper::ipc::framing;
use vault_helper::ipc::server::{Server, ServerConfig};
use vault_helper::keychain;

const MP: &str = "synthetic-ipc-master-password-0001";
const PASSWORD: &str = "synthetic-ipc-login-password (test fixture)";

/// Send one op and read frames until its response, answering any
/// capture_check with `suppressed` and collecting events on the way.
fn call(stream: &mut UnixStream, frame: Value, suppressed: bool) -> (Value, Vec<Value>) {
    framing::write_frame(stream, &frame).unwrap();
    let mut events = Vec::new();
    loop {
        let incoming = framing::read_frame(stream).unwrap();
        if incoming["op"] == "capture_check" {
            let reply = json!({"reply_to": incoming["id"], "suppressed": suppressed});
            framing::write_frame(stream, &reply).unwrap();
        } else if incoming.get("event").is_some() {
            events.push(incoming);
        } else {
            return (incoming, events);
        }
    }
}

#[test]
fn reveal_capture_check_is_answered_on_the_requesting_connection() {
    std::env::set_var("OV0_VAULT_PANEL_SCRIPT", format!("submit:{MP}"));
    std::env::set_var("OV0_VAULT_LA_STUB", "allow");
    std::env::set_var("OV0_VAULT_KEYCHAIN_PREFIX", format!("ov0ipccc-{}-", std::process::id()));
    keychain::delete_item("com.racker.zero.vault.state");
    keychain::delete_item("com.racker.zero.vault.helper-prefs");

    // Short path: macOS SUN_LEN caps socket paths at ~104 bytes.
    let dir = PathBuf::from(format!("/tmp/vhcc{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let socket = dir.join("helper.sock");
    let cfg = ServerConfig {
        socket_path: socket.clone(),
        vault_dir: dir.clone(),
        idle_timeout: Duration::from_secs(3600),
        shutdown_grace: Duration::from_millis(500),
    };
    let shutdown = Arc::new(AtomicBool::new(false));
    let server = Server::with_verifier(cfg, Arc::clone(&shutdown), Arc::new(|_| Ok(()))).unwrap();
    let handle = std::thread::spawn(move || server.run());
    for _ in 0..100 {
        if socket.exists() {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }

    let mut s = UnixStream::connect(&socket).unwrap();
    framing::write_frame(&mut s, &json!({"op": "hello", "proto": 1, "client": "app"})).unwrap();
    assert_eq!(framing::read_frame(&mut s).unwrap()["op"], "hello_ok");

    assert_eq!(call(&mut s, json!({"op": "setup_vault"}), true).0["ok"], true);
    let (resp, _) = call(&mut s, json!({"op": "begin_recovery_unlock", "kind": "mp"}), true);
    assert_eq!(resp["ok"], true, "{resp}");
    let (resp, _) = call(
        &mut s,
        json!({"op": "add_item", "kind": "login", "title": "IPC", "hosts": ["example.test"],
               "password": PASSWORD}),
        true,
    );
    let r = resp["ref"].as_str().expect("ref").to_string();

    // Suppression confirmed → the secret is released.
    let (resp, _) = call(&mut s, json!({"op": "reveal", "ref": r}), true);
    assert_eq!(resp["ok"], true, "reveal must succeed when capture is suppressed: {resp}");
    assert_eq!(resp["secret"]["password"], PASSWORD);

    // Suppression denied → fail closed, with the §14.4 event.
    let (resp, events) = call(&mut s, json!({"op": "reveal", "ref": r}), false);
    assert_eq!(resp["error"], "CAPTURE_UNSAFE", "{resp}");
    assert!(events.iter().any(|e| e["event"] == "capture_unsafe"));

    // A stray/late reply is consumed, never answered as an op: the next
    // response on the wire belongs to the next real request.
    framing::write_frame(&mut s, &json!({"reply_to": "cc-stale", "suppressed": true})).unwrap();
    let (resp, _) = call(&mut s, json!({"op": "get_state"}), true);
    assert_eq!(resp["state"], "unlocked", "{resp}");

    drop(s);
    shutdown.store(true, std::sync::atomic::Ordering::SeqCst);
    let _ = handle.join();
    keychain::delete_item("com.racker.zero.vault.state");
    keychain::delete_item("com.racker.zero.vault.helper-prefs");
    std::fs::remove_dir_all(&dir).ok();
}
