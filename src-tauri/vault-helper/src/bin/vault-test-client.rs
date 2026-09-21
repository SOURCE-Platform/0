//! Development/gate client for the vault helper. Never shipped.
//!
//! Exercises the real client path (`VaultClient`), including the reverse
//! SecCode check, so gate scripts can run the signed acceptance/rejection
//! matrix and the Phase C flows without hand-rolled sockets.
//!
//! Phase A modes (unchanged, used by scripts/phase-a-gate.sh):
//!   handshake | state | expect-reject-client | expect-reject-server | hold <secs>
//!
//! Phase C modes (synthetic data only; MP entry happens helper-side via
//! the scripted panel hook `OV0_VAULT_PANEL_SCRIPT`):
//!   setup                         setup_vault
//!   unlock-mp                     begin_recovery_unlock {kind:"mp"}
//!   unlock-rk-expect-unknown      begin_recovery_unlock {kind:"rk"} must fail UNKNOWN_OP
//!   change-mp                     change_master_password
//!   add-login <title> <user> <host> <password>
//!   update <ref> <field> <value>  field in title|username|password|host
//!   delete <ref>
//!   list                          prints LIST=<json>
//!   reveal <ref> allow|deny       capture handler forced on/off
//!   watch-events <secs>           prints EVENT=<json> lines
//!   set-autolock <minutes>        internal prefs op (documented deviation)
//!   raw <json>                    send one arbitrary frame, print RESP=<json>
//!
//! Flow modes also print `EVENT=<json>` for every event their own
//! connection received (the hub delivers events to the newest app-class
//! connection only, so a concurrent `watch-events` would be displaced).
//!
//! Exit codes: 0 = expectation met; 1 = expectation violated (security
//! failure); 2 = harness error (e.g., socket missing).

use std::os::unix::net::UnixStream;
use std::path::Path;
use std::process::ExitCode;

use serde_json::{json, Value};
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
        eprintln!("usage: {} <socket> <mode> [args...]", args[0]);
        return ExitCode::from(2);
    }
    let socket = Path::new(&args[1]);
    let mode = args[2].as_str();
    let rest: Vec<&str> = args[3..].iter().map(String::as_str).collect();

    match mode {
        // Phase A matrix
        "handshake" => handshake(socket),
        "state" => state(socket),
        "expect-reject-client" => expect_reject_client(socket, parse_class(rest.first().copied())),
        "expect-reject-server" => expect_reject_server(socket),
        "hold" => hold(socket, rest.first().and_then(|s| s.parse().ok()).unwrap_or(5)),
        // Phase C flows
        "setup" => op(socket, json!({"op": "setup_vault"})),
        "unlock-mp" => op(socket, json!({"op": "begin_recovery_unlock", "kind": "mp"})),
        "unlock-kind-expect-unknown" => {
            expect_unknown_op(socket, json!({"op": "begin_recovery_unlock", "kind": "device"}))
        }
        // Phase D flows (the helper's own panels hold every secret)
        "unlock-rk" => op(socket, json!({"op": "begin_recovery_unlock", "kind": "rk"})),
        "rotate-rk" => op(socket, json!({"op": "rotate_recovery_key"})),
        "change-mp" => op(socket, json!({"op": "change_master_password"})),
        "add-login" => {
            let &[title, user, host, password] = expect_args(&rest, 4, "add-login") else { unreachable!() };
            op(
                socket,
                json!({"op": "add_item", "kind": "login", "title": title,
                       "username": user, "hosts": [host], "password": password}),
            )
        }
        "update" => {
            let &[ref_, field, value] = expect_args(&rest, 3, "update") else { unreachable!() };
            op(socket, json!({"op": "update_item", "ref": ref_, field: value}))
        }
        "delete" => {
            let &[ref_] = expect_args(&rest, 1, "delete") else { unreachable!() };
            op(socket, json!({"op": "delete_item", "ref": ref_}))
        }
        "list" => op(socket, json!({"op": "list_items"})),
        "reveal" => {
            let &[ref_, policy] = expect_args(&rest, 2, "reveal") else { unreachable!() };
            reveal(socket, ref_, policy == "allow")
        }
        "watch-events" => watch_events(
            socket,
            rest.first().and_then(|s| s.parse().ok()).unwrap_or(10),
        ),
        "set-autolock" => {
            let &[minutes] = expect_args(&rest, 1, "set-autolock") else { unreachable!() };
            let Ok(minutes) = minutes.parse::<u64>() else {
                return harness("minutes must be a number");
            };
            op(socket, json!({"op": "set_auto_lock_minutes", "minutes": minutes}))
        }
        "raw" => {
            let &[payload] = expect_args(&rest, 1, "raw") else { unreachable!() };
            let Ok(frame) = serde_json::from_str::<Value>(payload) else {
                return harness("raw payload is not valid JSON");
            };
            op_with(socket, frame, true)
        }
        _ => {
            eprintln!("unknown mode: {mode}");
            ExitCode::from(2)
        }
    }
}

fn expect_args<'a>(rest: &'a [&str], n: usize, mode: &str) -> &'a [&'a str] {
    if rest.len() < n {
        eprintln!("harness: {mode} needs {n} args, got {}", rest.len());
        std::process::exit(2);
    }
    &rest[..n]
}

fn connect(socket: &Path) -> Result<VaultClient, ExitCode> {
    VaultClient::connect(socket, ClientClass::App).map_err(|e| {
        eprintln!("FAIL: connect: {e}");
        ExitCode::from(1)
    })
}

/// One op, printing OP_OK / OP_ERROR=<code> plus the state; secrets in
/// responses are printed only in `reveal` (synthetic data, gate binary).
fn op(socket: &Path, frame: Value) -> ExitCode {
    op_with(socket, frame, false)
}

/// `verbose` also prints the whole response frame: what `raw` is for, and
/// where the Phase E gate reads device lists and enrollment secrets from.
fn op_with(socket: &Path, frame: Value, verbose: bool) -> ExitCode {
    let client = match connect(socket) {
        Ok(c) => c,
        Err(code) => return code,
    };
    match client.request(frame) {
        Ok(resp) => {
            println!("OP_OK state={}", resp["state"].as_str().unwrap_or("-"));
            if verbose {
                println!("RESP={resp}");
            }
            if let Some(items) = resp.get("items") {
                println!("LIST={items}");
            }
            if let Some(r) = resp.get("ref") {
                println!("REF={}", r.as_str().unwrap_or("-"));
            }
            print_events(&client);
            println!("RESULT=OK");
            ExitCode::SUCCESS
        }
        Err(ClientError::Server(code)) => {
            println!("OP_ERROR={code}");
            print_events(&client);
            println!("RESULT=OK"); // the op ran; the code is the outcome
            ExitCode::SUCCESS
        }
        Err(e) => fail(&format!("request: {e}")),
    }
}

fn reveal(socket: &Path, ref_: &str, allow_capture: bool) -> ExitCode {
    let client = match connect(socket) {
        Ok(c) => c,
        Err(code) => return code,
    };
    client.set_capture_handler(std::sync::Arc::new(move |_surface| allow_capture));
    match client.request(json!({"op": "reveal", "ref": ref_})) {
        Ok(resp) => {
            println!("REVEAL={}", resp["secret"]);
            print_events(&client);
            println!("RESULT=OK");
            ExitCode::SUCCESS
        }
        Err(ClientError::Server(code)) => {
            println!("OP_ERROR={code}");
            print_events(&client);
            println!("RESULT=OK");
            ExitCode::SUCCESS
        }
        Err(e) => fail(&format!("reveal: {e}")),
    }
}

/// Print the events this connection received while the op ran. The hub
/// keeps one app-class slot (a new hello replaces the old), so events
/// reach the connection that issued the op, not a separate watcher; a
/// short grace catches events emitted just after the response.
fn print_events(client: &VaultClient) {
    std::thread::sleep(std::time::Duration::from_millis(150));
    while let Some(event) = client.try_recv_event() {
        println!("EVENT={event}");
    }
}

fn expect_unknown_op(socket: &Path, frame: Value) -> ExitCode {
    let client = match connect(socket) {
        Ok(c) => c,
        Err(code) => return code,
    };
    match client.request(frame) {
        Err(ClientError::Server(code)) if code == "UNKNOWN_OP" => {
            println!("REJECTED=UNKNOWN_OP");
            ExitCode::SUCCESS
        }
        other => {
            eprintln!("SECURITY FAILURE: expected UNKNOWN_OP, got {other:?}");
            ExitCode::from(1)
        }
    }
}

fn watch_events(socket: &Path, secs: u64) -> ExitCode {
    let client = match connect(socket) {
        Ok(c) => c,
        Err(code) => return code,
    };
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(secs);
    while std::time::Instant::now() < deadline {
        match client.try_recv_event() {
            Some(event) => println!("EVENT={event}"),
            None => std::thread::sleep(std::time::Duration::from_millis(50)),
        }
    }
    println!("RESULT=OK");
    ExitCode::SUCCESS
}

// ---- Phase A modes (kept byte-compatible with phase-a-gate.sh) ----

fn handshake(socket: &Path) -> ExitCode {
    match VaultClient::connect(socket, ClientClass::App) {
        Ok(client) => {
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

fn state(socket: &Path) -> ExitCode {
    match VaultClient::connect(socket, ClientClass::App).and_then(|c| c.get_state()) {
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
fn expect_reject_server(socket: &Path) -> ExitCode {
    match VaultClient::connect(socket, ClientClass::App) {
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
fn hold(socket: &Path, secs: u64) -> ExitCode {
    match VaultClient::connect(socket, ClientClass::App) {
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

fn harness(msg: &str) -> ExitCode {
    eprintln!("harness: {msg}");
    ExitCode::from(2)
}
