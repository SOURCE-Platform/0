//! Phase G (spec §7.2): wake the vault's iPhones for an approval request.
//! The helper signs one `push/send` per active iPhone (the body is its own:
//! the device to wake and the request id); this process only carries the
//! signed requests to the provider, which sends the fixed generic alert.
//! Best effort: an unreachable provider or no APNs key just means the user
//! opens SOURCE Vault themselves (the request is fetched directly, §7.1).

use serde_json::{json, Value};
use vault_coordinator::{Helper, Transport};

use super::{transport::ProviderHttp, AppHelper};

/// How many iPhones were woken.
pub fn wake_phones(request_id: &str) -> usize {
    let Ok(signed) = AppHelper.op(json!({"op": "push_wake_request", "request_id": request_id})) else { return 0 };
    let requests = signed["requests"].as_array().cloned().unwrap_or_default();
    requests
        .iter()
        .filter(|r| send(r))
        .count()
}

fn send(r: &Value) -> bool {
    let (Some(origin), Some(method), Some(path), Some(auth), Some(body)) = (r["origin"].as_str(), r["method"].as_str(), r["path"].as_str(), r["auth"].as_str(), r["body"].as_str()) else {
        return false;
    };
    matches!(ProviderHttp.send(origin, method, path, Some(auth), body.as_bytes()), Ok(resp) if resp.status == 200)
}
