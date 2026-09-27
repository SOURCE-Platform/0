//! The §1.5 provider ops end to end, driven exactly as the main process
//! drives them (spec v0.4 §1.3, §11.1): the helper's op dispatcher with
//! scripted panels, a simulated main that pulls and pushes ciphertext in
//! §1.3 chunks and asks the helper for every signature, and the provider
//! core in process. Covers: setup's `create` finishing while LOCKED,
//! `backup_prepare` → publish, sync (`backup_state_offer` / streams /
//! `backup_apply`), and total-loss recovery on an empty machine through
//! `recovery_begin` / `recovery_complete`. Synthetic data only.

#[path = "../../vault-helper/tests/vault_fx/mod.rs"]
mod vault_fx;
mod mfx;

use serde_json::{json, Value};
use vault_fx::{submitted, Fx};
use vault_helper::state::VaultState;
use vault_proto::b64;
use vault_proto::request::body_hash;

const HANDLE: &str = "synthetic-fixture@example.test";

fn hx(b: &[u8]) -> String {
    vault_helper::crypto::hex::encode(b)
}

fn ok(v: Value) -> Value {
    assert_eq!(v["ok"], true, "{v}");
    v
}

/// Ask the helper to sign, then send to the provider.
fn call(fx: &Fx, cloud: &mfx::Cloud, operation: &str, sha: Option<&str>, body: &[u8], expected: Option<&str>) -> vault_provider_core::Response {
    let mut f = json!({ "op": "sign_provider_request", "operation": operation, "body_sha256": hx(&body_hash(body)) });
    if let Some(s) = sha {
        f["sha256"] = json!(s);
    }
    if let Some(e) = expected {
        f["expected_state"] = json!(e);
    }
    let s = ok(fx.op(f));
    assert_eq!(s["origin"], mfx::ORIGIN);
    cloud.send(s["method"].as_str().unwrap(), s["path"].as_str().unwrap(), s["auth"].as_str(), body)
}

fn pull(fx: &Fx, session: &str, sha: &str) -> Vec<u8> {
    let mut out = Vec::new();
    loop {
        let r = ok(fx.op(json!({ "op": "stream_read", "session": session, "sha256": sha, "offset": out.len() })));
        out.extend(b64::decode(r["data"].as_str().unwrap()).unwrap());
        if r["eof"] == true {
            return out;
        }
    }
}

fn push(fx: &Fx, session: &str, bytes: &[u8]) {
    let sha = hx(&mfx::sha(bytes));
    let s = ok(fx.op(json!({ "op": "stream_begin", "session": session, "sha256": sha, "size": bytes.len() })));
    let id = s["stream_id"].as_str().unwrap().to_string();
    const CHUNK: usize = 24 * 1024;
    for (seq, chunk) in bytes.chunks(CHUNK).enumerate() {
        ok(fx.op(json!({ "op": "stream_write", "session": session, "stream_id": id, "seq": seq, "offset": seq * CHUNK, "data": b64::encode(chunk) })));
    }
    ok(fx.op(json!({ "op": "stream_end", "session": session, "stream_id": id })));
}

/// Main's side of a staged publication (publish, create or finalize).
fn run_publication(fx: &Fx, cloud: &mfx::Cloud, p: &Value) -> Value {
    let session = p["session"].as_str().unwrap();
    let expected = p["expected_state"].as_str().unwrap();
    if p["kind"] != "create" {
        let list = ok(fx.op(json!({ "op": "backup_blob_list", "session": session, "page": 0 })));
        for b in list["blobs"].as_array().unwrap() {
            let sha = b["sha256"].as_str().unwrap();
            let bytes = pull(fx, session, sha);
            assert_eq!(hx(&mfx::sha(&bytes)), sha, "main verifies before upload");
            let r = call(fx, cloud, "blob_put", Some(sha), &bytes, None);
            assert_eq!(r.status, 200, "{}", mfx::err(&r));
        }
    }
    let tb = ok(fx.op(json!({ "op": "backup_transition_body", "session": session })));
    let body = match tb["body"].as_str() {
        Some(b) => b64::decode(b).unwrap(),
        None => pull(fx, session, tb["stream"].as_str().unwrap()),
    };
    let r = call(fx, cloud, "state_commit", None, &body, Some(expected));
    ok(fx.op(json!({ "op": "backup_commit_result", "session": session, "status": r.status, "body": String::from_utf8_lossy(&r.body) })))
}

/// Main's side of a sync (or of recovery's download): offer, then feed
/// whatever the helper asks for until it reports.
fn run_sync(fx: &Fx, cloud: &mfx::Cloud) -> Value {
    let s = call(fx, cloud, "state_get", None, b"", None);
    assert_eq!(s.status, 200, "{}", mfx::err(&s));
    let offer = ok(fx.op(json!({ "op": "backup_state_offer", "state": String::from_utf8_lossy(&s.body) })));
    if offer["up_to_date"] == true {
        return offer;
    }
    let session = offer["session"].as_str().unwrap().to_string();
    let mut need: Vec<String> = offer["need"].as_array().unwrap().iter().map(|h| h.as_str().unwrap().to_string()).collect();
    loop {
        for h in &need {
            let r = call(fx, cloud, "blob_get", Some(h), b"", None);
            assert_eq!(r.status, 200);
            push(fx, &session, &r.body);
        }
        let applied = ok(fx.op(json!({ "op": "backup_apply", "session": session })));
        match applied["need"].as_array() {
            Some(n) if !n.is_empty() => need = n.iter().map(|h| h.as_str().unwrap().to_string()).collect(),
            _ => return applied,
        }
    }
}

#[test]
fn setup_publish_sync_and_recover_through_ops() {
    let _g = vault_fx::serial();
    let cloud = mfx::Cloud::new("ipc");
    let fx = vault_fx::fx();
    // Setup stages the `create`; it finishes while LOCKED (§11.3.2).
    fx.push_panel(submitted(vault_fx::MP));
    let setup = ok(fx.op(json!({ "op": "setup_vault", "handle": HANDLE })));
    assert_eq!(fx.core.lock().unwrap().reported_state(), VaultState::BackingUp);
    let done = run_publication(&fx, &cloud, &setup["publication"]);
    assert_eq!(done["committed"], true, "{done}");
    assert_eq!(fx.state(), VaultState::Locked);
    // Unlock, add, publish.
    assert_eq!(vault_fx::unlock(&fx, vault_fx::MP)["ok"], true);
    vault_fx::add_login(&fx);
    let prep = ok(fx.op(json!({ "op": "backup_prepare" })));
    assert_eq!(run_publication(&fx, &cloud, &prep)["committed"], true);
    assert_eq!(run_sync(&fx, &cloud)["up_to_date"], true);

    // A fresh machine: total-loss recovery with the master password.
    let fresh = vault_fx::fx();
    let locate = String::from_utf8(cloud.locate(HANDLE)).unwrap();
    fresh.push_panel(submitted(vault_fx::MP));
    ok(fresh.op(json!({ "op": "recovery_begin", "kind": "mp", "locate_response": locate })));
    assert_eq!(fresh.state(), VaultState::Recovering);
    let preview = run_sync(&fresh, &cloud);
    assert_eq!(preview["preview"]["item_count"], 1, "FR-01 before completion");
    let staged = ok(fresh.op(json!({ "op": "recovery_complete" })));
    let fin = run_publication(&fresh, &cloud, &staged);
    assert_eq!(fin["committed"], true, "{fin}");
    assert_eq!(fresh.state(), VaultState::Unlocked);
    let items = ok(fresh.op(json!({ "op": "list_items" })));
    assert_eq!(items["items"].as_array().unwrap().len(), 1);
    // The original Mac is cut off (S-4): its reads get the generic 401.
    let r = call(&fx, &cloud, "state_get", None, b"", None);
    assert_eq!(r.status, 401);
    fx.remove_dir();
    fresh.remove_dir();
}
