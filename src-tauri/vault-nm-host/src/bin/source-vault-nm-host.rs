//! `source-vault-nm-host` (spec §9.2; Phase H): spawned by Chrome for the
//! pinned extension, it relays the extension's frames to the vault helper
//! as client class `nm-host` and the answers back. The logic lives in
//! `vault_nm_host::broker`; this file is only the I/O. It exits when stdin
//! closes, and before reading anything if the caller is not the pinned
//! extension (§9.2 layer 2).

use std::io::{Read, Write};
use std::process::ExitCode;

use serde_json::json;
use vault_nm_host::broker::{caller_allowed, frame, translate, MAX_FRAME};
use vault_nm_host::helper::Helper;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    if !caller_allowed(&args) {
        return ExitCode::from(2);
    }
    let Some(mut helper) = Helper::connect() else {
        return ExitCode::from(3);
    };
    let (mut input, mut output) = (std::io::stdin().lock(), std::io::stdout().lock());
    loop {
        let mut len = [0u8; 4];
        if input.read_exact(&mut len).is_err() {
            return ExitCode::SUCCESS; // Chrome closed the port
        }
        let n = u32::from_le_bytes(len) as usize;
        if n > MAX_FRAME {
            return ExitCode::from(4); // never buffer an oversized frame
        }
        let mut body = vec![0u8; n];
        if input.read_exact(&mut body).is_err() {
            return ExitCode::SUCCESS;
        }
        let answer = match translate(&body) {
            Ok(request) => helper.request(&request).unwrap_or_else(|| json!({ "ok": false, "error": "VAULT_UNAVAILABLE" })),
            Err(code) => json!({ "ok": false, "error": code }),
        };
        if output.write_all(&frame(&answer)).and_then(|_| output.flush()).is_err() {
            return ExitCode::SUCCESS;
        }
    }
}
