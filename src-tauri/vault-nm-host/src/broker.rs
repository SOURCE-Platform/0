//! The Chrome native messaging broker's frame logic (spec §9.2, §9.3; Phase H),
//! kept apart from its I/O so it is testable. nm-host is a dumb broker: it
//! checks the caller origin Chrome passed, validates each frame's shape
//! and size, and forwards it to the helper as the matching `browser_*` op.
//! It holds no keys, matches nothing, and never logs a field value.
//!
//! Layer 2 (§9.2): Chrome passes the calling extension's origin as the
//! first argument; it must be exactly the pinned extension's
//! `chrome-extension://<id>/` — the production id in a release build, the
//! compiled-in development id only in a debug build. Layer 3 (the parent's
//! code identity) waits for the NM-05/NM-06 prototype's verdict.

use serde_json::{json, Map, Value};

/// §9.3: the extension's frames are at most 32 KiB.
pub const MAX_FRAME: usize = 32 * 1024;

/// The production extension id (Chrome Web Store); set when published.
pub const PRODUCTION_ID: &str = "";

/// Debug builds only: the unpacked development extension's pinned id.
#[cfg(debug_assertions)]
pub const DEV_ID: Option<&str> = option_env!("SOURCE_NM_DEV_EXTENSION_ID");
#[cfg(not(debug_assertions))]
pub const DEV_ID: Option<&str> = None;

/// Layer 2: the argument Chrome passed names the pinned extension.
pub fn caller_allowed(args: &[String]) -> bool {
    let Some(origin) = args.get(1) else { return false };
    let ok = |id: &str| !id.is_empty() && origin == &format!("chrome-extension://{id}/");
    ok(PRODUCTION_ID) || DEV_ID.is_some_and(ok)
}

/// The extension's op → the helper op, and the fields it may carry.
fn route(op: &str) -> Option<(&'static str, &'static [&'static str])> {
    Some(match op {
        "hello" => ("browser_hello", &["proto", "session_echo"]),
        "request_unlock" => ("browser_request_unlock", &[]),
        "get_candidates" => ("browser_candidates", &["origin", "tab_url"]),
        "authorize" => ("browser_authorize", &["request_id", "ref", "method"]),
        "fill" => ("browser_fill", &["request_id"]),
        "save_new" => ("browser_save_new", &["origin", "tab_url", "username", "password", "title"]),
        "save_update" => ("browser_save_update", &["ref", "origin", "tab_url", "password", "method"]),
        "save_update_finish" => ("browser_save_update_finish", &["ref", "origin", "tab_url", "password", "approval_id"]),
        _ => return None,
    })
}

/// A frame from the extension, checked and translated, or the refusal to
/// answer it with. Only listed fields, only strings or small numbers.
pub fn translate(bytes: &[u8]) -> Result<Value, &'static str> {
    if bytes.len() > MAX_FRAME {
        return Err("TOO_LARGE");
    }
    let v: Value = serde_json::from_slice(bytes).map_err(|_| "FORMAT_INVALID")?;
    let obj = v.as_object().ok_or("FORMAT_INVALID")?;
    let (helper_op, allowed) = obj.get("op").and_then(Value::as_str).and_then(route).ok_or("UNKNOWN_OP")?;
    let mut out = Map::new();
    out.insert("op".into(), json!(helper_op));
    for (k, val) in obj {
        if k == "op" {
            continue;
        }
        if !allowed.contains(&k.as_str()) {
            return Err("FORMAT_INVALID");
        }
        let fine = match val {
            Value::String(s) => s.len() <= 4096,
            Value::Number(n) => n.as_u64().is_some_and(|n| n < 1 << 32),
            _ => false,
        };
        if !fine {
            return Err("FORMAT_INVALID");
        }
        out.insert(k.clone(), val.clone());
    }
    Ok(Value::Object(out))
}

/// Chrome's framing: a 4-byte native-endian (little on Apple silicon)
/// length, then the JSON.
pub fn frame(v: &Value) -> Vec<u8> {
    let body = serde_json::to_vec(v).unwrap_or_default();
    let mut out = (body.len() as u32).to_le_bytes().to_vec();
    out.extend(body);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_documented_frames_pass() {
        let ok = translate(br#"{"op":"get_candidates","origin":"https://a.test","tab_url":"https://a.test/x"}"#).unwrap();
        assert_eq!(ok["op"], "browser_candidates");
        for bad in [
            &br#"{"op":"reveal","ref":"x"}"#[..],
            br#"{"op":"get_candidates","origin":"https://a.test","extra":"x"}"#,
            br#"{"op":"authorize","request_id":{"nested":true}}"#,
            br#"[1,2]"#,
            br#"not json"#,
        ] {
            assert!(translate(bad).is_err(), "{}", String::from_utf8_lossy(bad));
        }
        assert_eq!(translate(&vec![b' '; MAX_FRAME + 1]), Err("TOO_LARGE"));
    }

    #[test]
    fn only_the_pinned_extension_may_call() {
        assert!(!caller_allowed(&["nm-host".into()]));
        assert!(!caller_allowed(&["nm-host".into(), "chrome-extension://someoneelse/".into()]));
        assert!(!caller_allowed(&["nm-host".into(), "chrome-extension:///".into()]), "an empty id never matches");
    }
}
