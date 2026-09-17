//! Development/gate client for the Phase A vault helper. Never shipped.
//!
//! Exercises the real client path (`VaultClient`), including the reverse
//! SecCode check, so `scripts/phase-a-gate.sh` can run the signed
//! acceptance/rejection matrix without hand-rolled sockets.
//!
//! Usage: vault-test-client <socket> <mode> [class]
//!   mode = handshake          hello + get_state + lock + get_state, print results
//!   mode = state              hello + get_state, print STATE=<state>
//!   mode = expect-reject-client   we are an impostor client; expect the helper to drop us
//!   mode = expect-reject-server   the helper is an impostor; expect our reverse check to fail
//!   mode = hold <secs>        connect, hello, then hold the connection open
//!
//! Exit codes: 0 = expectation met; 1 = expectation violated (security
//! failure); 2 = harness error (e.g., socket missing).

use std::os::unix::net::UnixStream;
use std::path::Path;
use std::process::ExitCode;

use vault_helper::ipc::client::{ClientError, VaultClient};
use vault_helper::ipc::framing;
use vault_helper::ops::ClientClass;

fn parse_class(arg: Option<&str>) -> ClientClass {
    match arg {
        Some("nm-host") => ClientClass::NmHost,
        _ => ClientClass::App,
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        eprintln!("usage: {} <socket> <mode> [class]", args[0]);
        return ExitCode::from(2);
    }
    let socket = Path::new(&args[1]);
    let mode = args[2].as_str();
    let class = parse_class(args.get(3).map(String::as_str));

    match mode {
        "handshake" => handshake(socket, class),
        "state" => state(socket, class),
        "expect-reject-client" => expect_reject_client(socket, class),
        "expect-reject-server" => expect_reject_server(socket, class),
        "hold" => {
            let secs = args.get(3).and_then(|s| s.parse::<u64>().ok()).unwrap_or(5);
            hold(socket, class, secs)
        }
        _ => {
            eprintln!("unknown mode: {mode}");
            ExitCode::from(2)
        }
    }
}

fn handshake(socket: &Path, class: ClientClass) -> ExitCode {
    match VaultClient::connect(socket, class) {
        Ok(mut client) => {
            println!("HELLO_STATE={}", client.state());
            match client.get_state() {
                Ok(s) => println!("STATE={s}"),
                Err(e) => return fail(&format!("get_state: {e}")),
            }
            match client.lock() {
                Ok(s) => println!("LOCK_STATE={s}"),
                Err(e) => return fail(&format!("lock: {e}")),
            }
            match client.get_state() {
                Ok(s) => println!("STATE={s}"),
                Err(e) => return fail(&format!("get_state(2): {e}")),
            }
            println!("RESULT=OK");
            ExitCode::SUCCESS
        }
        Err(e) => fail(&format!("connect: {e}")),
    }
}

fn state(socket: &Path, class: ClientClass) -> ExitCode {
    match VaultClient::connect(socket, class).and_then(|mut c| c.get_state()) {
        Ok(s) => {
            println!("STATE={s}");
            ExitCode::SUCCESS
        }
        Err(e) => fail(&format!("{e}")),
    }
}

/// We are signed incorrectly (or not at all). The helper must authenticate
/// us and drop the connection: the handshake must fail with EOF/framing,
/// never with a hello_ok. Any other outcome is a security failure.
fn expect_reject_client(socket: &Path, class: ClientClass) -> ExitCode {
    let mut stream = match UnixStream::connect(socket) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("harness: cannot connect: {e}");
            return ExitCode::from(2);
        }
    };
    // Skip VaultClient: even if the reverse check passes (legit helper),
    // the helper must close before/without hello_ok.
    let hello = serde_json::json!({"op": "hello", "proto": 1, "client": class.as_str()});
    if framing::write_frame(&mut stream, &hello).is_err() {
        println!("REJECTED=write");
        return ExitCode::SUCCESS;
    }
    match framing::read_frame(&mut stream) {
        Ok(frame) => {
            eprintln!("SECURITY FAILURE: impostor client received a frame: {frame}");
            ExitCode::from(1)
        }
        Err(e) => {
            println!("REJECTED={e}");
            ExitCode::SUCCESS
        }
    }
}

/// The helper on the other end is an impostor. Our reverse SecCode check
/// must fail before a single byte is sent.
fn expect_reject_server(socket: &Path, class: ClientClass) -> ExitCode {
    match VaultClient::connect(socket, class) {
        Ok(_) => {
            eprintln!("SECURITY FAILURE: impostor helper passed reverse authentication");
            ExitCode::from(1)
        }
        Err(ClientError::Auth(e)) => {
            println!("REJECTED={e}");
            ExitCode::SUCCESS
        }
        Err(ClientError::Io(e)) => {
            eprintln!("harness: cannot connect: {e}");
            ExitCode::from(2)
        }
        Err(e) => {
            eprintln!("harness: unexpected error shape: {e}");
            ExitCode::from(2)
        }
    }
}

/// Connect (full auth + hello) and hold the connection open for `secs`.
/// Used by the gate to prove the 5 s shutdown grace: the helper must exit
/// on SIGTERM even while an authenticated client stays attached.
fn hold(socket: &Path, class: ClientClass, secs: u64) -> ExitCode {
    match VaultClient::connect(socket, class) {
        Ok(client) => {
            println!("HOLDING state={}", client.state());
            std::thread::sleep(std::time::Duration::from_secs(secs));
            println!("RESULT=OK");
            ExitCode::SUCCESS
        }
        Err(e) => fail(&format!("hold connect: {e}")),
    }
}

fn fail(msg: &str) -> ExitCode {
    eprintln!("FAIL: {msg}");
    ExitCode::from(1)
}
