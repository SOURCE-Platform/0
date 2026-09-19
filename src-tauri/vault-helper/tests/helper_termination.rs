//! Helper termination under a broken stderr (Phase C.1 regression).
//!
//! Observed: an orphaned debug helper — launched by `tauri dev`, whose
//! parent had exited so the helper's stderr was a broken pipe — ignored
//! SIGTERM. Cause: `eprintln!` panics on a failed write; the shutdown and
//! idle-exit paths log before `process::exit`, so the panic killed the
//! server thread and the AppKit main thread kept the process alive
//! forever. These tests run the real helper binary as a child process,
//! break its stderr after startup, and require it to terminate through
//! each exit path: idle timeout, and SIGTERM after rejected (unsigned)
//! peers made it log. The signed-client-attached variant (the exact
//! incident) runs in scripts/phase-c-gate.sh, which can sign a client.

use std::io::{BufRead, BufReader};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const EXIT_BUDGET: Duration = Duration::from_secs(10);

/// Start the helper with an isolated socket/dir/keychain prefix, wait for
/// its "listening" line, then drop the read end of its stderr pipe.
fn start_with_broken_stderr(tag: &str, idle_secs: Option<u64>) -> (Child, PathBuf) {
    // Short path: macOS SUN_LEN caps socket paths at ~104 bytes.
    let dir = PathBuf::from(format!("/tmp/vht{}{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let socket = dir.join("s");
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_source-vault-helper"));
    cmd.env("OV0_VAULT_SOCKET_PATH", &socket)
        .env("OV0_VAULT_DIR", dir.join("v"))
        .env("OV0_VAULT_KEYCHAIN_PREFIX", format!("ov0term-{}-{tag}-", std::process::id()))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    if let Some(secs) = idle_secs {
        cmd.env("OV0_VAULT_IDLE_SECS", secs.to_string());
    }
    let mut child = cmd.spawn().expect("spawn helper");
    let mut line = String::new();
    BufReader::new(child.stderr.as_mut().unwrap())
        .read_line(&mut line)
        .unwrap();
    assert!(line.contains("listening"), "unexpected first line: {line}");
    drop(child.stderr.take()); // every later log write now hits EPIPE
    (child, dir)
}

fn exits_within(child: &mut Child, budget: Duration) -> Option<std::process::ExitStatus> {
    let deadline = Instant::now() + budget;
    while Instant::now() < deadline {
        if let Some(status) = child.try_wait().unwrap() {
            return Some(status);
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let _ = child.kill();
    let _ = child.wait();
    None
}

#[test]
fn idle_exit_survives_broken_stderr() {
    let (mut child, dir) = start_with_broken_stderr("idle", Some(1));
    let status = exits_within(&mut child, EXIT_BUDGET)
        .expect("helper must idle-exit even when stderr is a broken pipe");
    assert!(status.success(), "{status}");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn sigterm_after_rejected_peers_survives_broken_stderr() {
    let (mut child, dir) = start_with_broken_stderr("term", None);
    // Unsigned peers (this test binary) are rejected by SecCode peer auth;
    // each rejection logs, which used to panic its thread on EPIPE.
    for _ in 0..5 {
        if let Ok(stream) = UnixStream::connect(dir.join("s")) {
            drop(stream);
        }
    }
    std::thread::sleep(Duration::from_millis(300));
    assert!(child.try_wait().unwrap().is_none(), "helper died on rejected peers");
    // SAFETY: plain kill(2) on our own child's pid.
    unsafe { libc::kill(child.id() as libc::pid_t, libc::SIGTERM) };
    let status = exits_within(&mut child, EXIT_BUDGET)
        .expect("helper must honor SIGTERM even when stderr is a broken pipe");
    assert!(status.success(), "{status}");
    std::fs::remove_dir_all(&dir).ok();
}
