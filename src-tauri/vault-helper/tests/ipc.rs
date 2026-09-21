//! End-to-end server tests over real Unix sockets. Peer-code verification
//! is injected as accept-all here (the real SecCode matrix — signed accept,
//! unsigned/ad-hoc/wrong-identifier reject, both directions — is exercised
//! against signed binaries by scripts/phase-a-gate.sh, which cargo tests
//! cannot do from an unsigned test harness).

use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use vault_helper::ipc::framing;
use vault_helper::ipc::server::{Server, ServerConfig};

// macOS Unix socket paths are limited to ~104 bytes (SUN_LEN), so test
// dirs must be short: /tmp/vh<pid>-<n>, not the long $TMPDIR + test name.
fn test_dir(_name: &str) -> PathBuf {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let dir = PathBuf::from(format!(
        "/tmp/vh{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::SeqCst)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn config(dir: &std::path::Path, idle_secs: u64) -> ServerConfig {
    ServerConfig {
        socket_path: dir.join("helper.sock"),
        vault_dir: dir.to_path_buf(),
        idle_timeout: Duration::from_secs(idle_secs),
        shutdown_grace: Duration::from_millis(500),
    }
}

fn start(cfg: ServerConfig, shutdown: Arc<AtomicBool>) -> std::thread::JoinHandle<i32> {
    let server = Server::with_verifier(cfg, shutdown, Arc::new(|_| Ok(()))).unwrap();
    std::thread::spawn(move || server.run())
}

fn raw_hello(socket: &std::path::Path, class: &str) -> (UnixStream, Value) {
    let mut stream = UnixStream::connect(socket).unwrap();
    framing::write_frame(
        &mut stream,
        &json!({"op": "hello", "proto": 1, "client": class}),
    )
    .unwrap();
    let resp = framing::read_frame(&mut stream).unwrap();
    (stream, resp)
}

fn op(stream: &mut UnixStream, op: &str) -> Value {
    framing::write_frame(stream, &json!({"op": op})).unwrap();
    framing::read_frame(stream).unwrap()
}

fn wait_for_socket(path: &std::path::Path) {
    for _ in 0..100 {
        if path.exists() {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("socket never appeared: {}", path.display());
}

#[test]
fn hello_then_ops_over_socket() {
    let dir = test_dir("ops");
    let shutdown = Arc::new(AtomicBool::new(false));
    let handle = start(config(&dir, 3600), Arc::clone(&shutdown));
    wait_for_socket(&dir.join("helper.sock"));

    let (mut stream, hello) = raw_hello(&dir.join("helper.sock"), "app");
    assert_eq!(hello["op"], "hello_ok");
    assert_eq!(hello["proto"], 1);
    assert_eq!(hello["state"], "uninitialized");

    let resp = op(&mut stream, "get_state");
    assert_eq!(resp["ok"], true);
    assert_eq!(resp["state"], "uninitialized");

    // lock with no vault: no-op, stays uninitialized, idempotent.
    let resp = op(&mut stream, "lock");
    assert_eq!(resp["ok"], true);
    assert_eq!(resp["state"], "uninitialized");

    // `unlock` is the §2.8 device-envelope path. It exists, so the
    // refusal here is about state, not about the op being unknown:
    // there is no vault to unlock yet.
    let resp = op(&mut stream, "unlock");
    assert_eq!(resp["ok"], false);
    assert_eq!(resp["error"], "BAD_STATE");

    // An op that genuinely does not exist still answers UNKNOWN_OP, so
    // this test keeps covering that path too.
    let resp = op(&mut stream, "no_such_op");
    assert_eq!(resp["ok"], false);
    assert_eq!(resp["error"], "UNKNOWN_OP");

    shutdown.store(true, Ordering::SeqCst);
    drop(stream);
    assert_eq!(handle.join().unwrap(), 0);
}

#[test]
fn vault_header_makes_boot_state_locked() {
    let dir = test_dir("header");
    std::fs::write(dir.join("header.json"), b"{}").unwrap();
    let shutdown = Arc::new(AtomicBool::new(false));
    let handle = start(config(&dir, 3600), Arc::clone(&shutdown));
    wait_for_socket(&dir.join("helper.sock"));

    let (_stream, hello) = raw_hello(&dir.join("helper.sock"), "app");
    assert_eq!(hello["state"], "locked");

    shutdown.store(true, Ordering::SeqCst);
    assert_eq!(handle.join().unwrap(), 0);
}

#[test]
fn same_class_second_hello_replaces_first() {
    let dir = test_dir("replace");
    let shutdown = Arc::new(AtomicBool::new(false));
    let handle = start(config(&dir, 3600), Arc::clone(&shutdown));
    wait_for_socket(&dir.join("helper.sock"));

    let (mut first, _) = raw_hello(&dir.join("helper.sock"), "app");
    let (_second, hello) = raw_hello(&dir.join("helper.sock"), "app");
    assert_eq!(hello["op"], "hello_ok");

    // The evicted connection must be closed by the server.
    framing::write_frame(&mut first, &json!({"op": "get_state"})).ok();
    let result = framing::read_frame(&mut first);
    assert!(
        result.is_err(),
        "evicted connection must not answer: {result:?}"
    );

    // A different class gets its own slot and both stay usable.
    let (mut nm, _) = raw_hello(&dir.join("helper.sock"), "nm-host");
    assert_eq!(op(&mut nm, "get_state")["ok"], true);

    shutdown.store(true, Ordering::SeqCst);
    assert_eq!(handle.join().unwrap(), 0);
}

#[test]
fn first_frame_must_be_hello() {
    let dir = test_dir("firstframe");
    let shutdown = Arc::new(AtomicBool::new(false));
    let handle = start(config(&dir, 3600), Arc::clone(&shutdown));
    wait_for_socket(&dir.join("helper.sock"));

    let mut stream = UnixStream::connect(dir.join("helper.sock")).unwrap();
    framing::write_frame(&mut stream, &json!({"op": "get_state"})).unwrap();
    assert!(framing::read_frame(&mut stream).is_err());

    // Protocol major mismatch: disconnect per spec §1.4.
    let mut stream = UnixStream::connect(dir.join("helper.sock")).unwrap();
    framing::write_frame(
        &mut stream,
        &json!({"op": "hello", "proto": 2, "client": "app"}),
    )
    .unwrap();
    assert!(framing::read_frame(&mut stream).is_err());

    shutdown.store(true, Ordering::SeqCst);
    assert_eq!(handle.join().unwrap(), 0);
}

#[test]
fn oversize_frame_closes_connection() {
    let dir = test_dir("oversize");
    let shutdown = Arc::new(AtomicBool::new(false));
    let handle = start(config(&dir, 3600), Arc::clone(&shutdown));
    wait_for_socket(&dir.join("helper.sock"));

    let (mut stream, _) = raw_hello(&dir.join("helper.sock"), "app");
    use std::io::Write;
    stream.write_all(&(70_000u32).to_be_bytes()).unwrap();
    stream.write_all(&[b'x'; 64]).unwrap();
    assert!(framing::read_frame(&mut stream).is_err());

    shutdown.store(true, Ordering::SeqCst);
    assert_eq!(handle.join().unwrap(), 0);
}

#[test]
fn idle_timeout_exits_with_zero_clients() {
    let dir = test_dir("idle");
    let shutdown = Arc::new(AtomicBool::new(false));
    let started = Instant::now();
    let handle = start(config(&dir, 1), Arc::clone(&shutdown));
    assert_eq!(handle.join().unwrap(), 0);
    assert!(started.elapsed() < Duration::from_secs(10));
}

#[test]
fn shutdown_drains_within_grace_even_with_client_attached() {
    let dir = test_dir("grace");
    let shutdown = Arc::new(AtomicBool::new(false));
    let handle = start(config(&dir, 3600), Arc::clone(&shutdown));
    wait_for_socket(&dir.join("helper.sock"));

    let (_stream, _) = raw_hello(&dir.join("helper.sock"), "app"); // held open
    std::thread::sleep(Duration::from_millis(100));
    let requested = Instant::now();
    shutdown.store(true, Ordering::SeqCst);
    assert_eq!(handle.join().unwrap(), 0);
    // grace is 500ms in this config; well under the 5s production value
    assert!(requested.elapsed() < Duration::from_secs(3));
}
