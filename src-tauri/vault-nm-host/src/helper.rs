//! The one connection to the vault helper (spec §1.4): its socket, its
//! code identity checked first (`codesign`), a `hello` as class `nm-host`,
//! then one request at a time. Frames: a 4-byte big-endian length, then
//! JSON, at most 64 KiB (the helper's framing).

use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;

use serde_json::{json, Value};

const MAX_FRAME: usize = 64 * 1024;

/// `~/.observer_data/vault/helper.sock`; debug builds honour the same
/// overrides as the helper.
pub fn socket_path() -> PathBuf {
    #[cfg(debug_assertions)]
    for var in ["OV0_VAULT_SOCKET_PATH"] {
        if let Some(p) = std::env::var(var).ok().filter(|p| !p.is_empty()) {
            return PathBuf::from(p);
        }
    }
    #[cfg(debug_assertions)]
    if let Some(d) = std::env::var("OV0_VAULT_DIR").ok().filter(|d| !d.is_empty()) {
        return PathBuf::from(d).join("helper.sock");
    }
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
    PathBuf::from(home).join(".observer_data/vault/helper.sock")
}

fn write_frame(s: &mut UnixStream, v: &Value) -> Option<()> {
    let body = serde_json::to_vec(v).ok()?;
    (body.len() <= MAX_FRAME).then_some(())?;
    s.write_all(&(body.len() as u32).to_be_bytes()).ok()?;
    s.write_all(&body).ok()?;
    s.flush().ok()
}

fn read_frame(s: &mut UnixStream) -> Option<Value> {
    let mut len = [0u8; 4];
    s.read_exact(&mut len).ok()?;
    let n = u32::from_be_bytes(len) as usize;
    if n > MAX_FRAME {
        return None;
    }
    let mut body = vec![0u8; n];
    s.read_exact(&mut body).ok()?;
    serde_json::from_slice(&body).ok().filter(Value::is_object)
}

pub struct Helper(UnixStream);

impl Helper {
    /// Connect, verify the helper's code identity, say hello as `nm-host`.
    pub fn connect() -> Option<Helper> {
        let mut s = UnixStream::connect(socket_path()).ok()?;
        if !crate::codesign::peer_is_helper(&s) {
            return None;
        }
        write_frame(&mut s, &json!({ "op": "hello", "proto": 1, "client": "nm-host" }))?;
        let hello = read_frame(&mut s)?;
        (hello["op"] == "hello_ok" && hello["ok"] == true && hello["proto"] == 1).then_some(Helper(s))
    }

    /// One request; events the helper broadcasts are skipped.
    pub fn request(&mut self, frame: &Value) -> Option<Value> {
        write_frame(&mut self.0, frame)?;
        loop {
            let answer = read_frame(&mut self.0)?;
            if answer.get("event").is_none() {
                return Some(answer);
            }
        }
    }
}
