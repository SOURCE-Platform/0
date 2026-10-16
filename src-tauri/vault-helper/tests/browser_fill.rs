//! Phase H helper side (spec §9.3–§9.9): candidates for a page, the
//! one-shot fill behind presence or an iPhone approval, saves, and updates
//! that need a confirmation outside the extension. Synthetic data only.

mod device_fx;
mod join_fx;
mod sync_fx;
mod vault_fx;

use serde_json::{json, Value};
use sync_fx::*;
use vault_fx::*;
use vault_proto::b64;

fn login(fx: &Fx, title: &str, host: &str, user: &str) -> String {
    let r = fx.op(json!({"op": "add_item", "kind": "login", "title": title, "username": user, "password": "synthetic-old", "urls": [{"host": host, "match": "exact", "allow_http": false}]}));
    assert_eq!(r["ok"], true, "{r}");
    r["ref"].as_str().unwrap().to_string()
}

fn candidates(fx: &Fx, origin: &str, tab: &str) -> Value {
    fx.op(json!({"op": "browser_candidates", "origin": origin, "tab_url": tab}))
}

fn reset() {
    vault_helper::vault::browser_ops::reset();
    vault_helper::approval::reset();
}

#[test]
fn candidates_follow_the_policy_and_a_fill_goes_out_once() {
    let _g = serial();
    reset();
    let fx = fx();
    setup_and_unlock(&fx);
    let r = login(&fx, "GitHub", "github.com", "alice@example.test");
    let c = candidates(&fx, "https://github.com", "https://github.com/login");
    assert_eq!(c["accounts"][0]["ref"], r.as_str(), "{c}");
    assert!(c["accounts"][0].get("password").is_none(), "metadata only");
    let sub = candidates(&fx, "https://gist.github.com", "https://gist.github.com/");
    assert_eq!((sub["accounts"].as_array().unwrap().len(), sub["similar"].as_u64()), (0, Some(1)), "a hint, never a fill");
    assert_eq!(fx.op(json!({"op": "browser_candidates", "origin": "https://github.com", "tab_url": "https://evil.test/"}))["error"], "APPROVAL_MISMATCH", "BR-11");
    assert_eq!(candidates(&fx, "http://github.com", "http://github.com/")["accounts"].as_array().unwrap().len(), 0, "no https → http");
    // The fill: presence, once.
    let id = c["request_id"].as_str().unwrap();
    let calls = fx.la.calls.load(std::sync::atomic::Ordering::SeqCst);
    let f = fx.op(json!({"op": "browser_authorize", "request_id": id, "ref": r, "method": "local"}));
    assert_eq!(f["fill"]["password"], "synthetic-old", "{f}");
    assert!(fx.la.calls.load(std::sync::atomic::Ordering::SeqCst) > calls);
    assert_eq!(fx.op(json!({"op": "browser_authorize", "request_id": id, "ref": r, "method": "local"}))["error"], "APPROVAL_REPLAY");
    // Another ref than the request offered → mismatch.
    let c2 = candidates(&fx, "https://github.com", "https://github.com/");
    let other = login(&fx, "Other", "other.example.test", "bob@example.test");
    assert_eq!(fx.op(json!({"op": "browser_authorize", "request_id": c2["request_id"], "ref": other, "method": "local"}))["error"], "APPROVAL_MISMATCH");
    // Locked: no candidates at all.
    fx.core.lock().unwrap().lock(vault_helper::vault::LockReason::Explicit);
    assert_eq!(candidates(&fx, "https://github.com", "https://github.com/")["locked"], true);
    fx.remove_dir();
}

#[test]
fn saves_never_overwrite_and_updates_need_a_confirmation() {
    let _g = serial();
    reset();
    let fx = fx();
    setup_and_unlock(&fx);
    let r = login(&fx, "Shop", "shop.example.test", "alice@example.test");
    let page = |extra: Value| {
        let mut f = json!({"origin": "https://shop.example.test", "tab_url": "https://shop.example.test/account"});
        f.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
        f
    };
    let mut new = page(json!({"op": "browser_save_new", "username": "carol@example.test", "password": "synthetic-new"}));
    assert_eq!(fx.op(new.take())["ok"], true);
    let dup = fx.op(page(json!({"op": "browser_save_new", "username": "alice@example.test", "password": "synthetic-x"})));
    assert_eq!(dup["needs_update"], r.as_str(), "an existing login is never overwritten here: {dup}");
    // Update on another site's page: refused before any prompt.
    let wrong = fx.op(json!({"op": "browser_save_update", "origin": "https://evil.test", "tab_url": "https://evil.test/", "ref": r, "password": "synthetic-evil", "method": "local"}));
    assert_eq!(wrong["error"], "APPROVAL_MISMATCH");
    let ok = fx.op(page(json!({"op": "browser_save_update", "ref": r, "password": "synthetic-updated", "method": "local"})));
    assert_eq!(ok["ok"], true, "{ok}");
    assert!(fx.op(json!({"op": "reveal", "ref": r})).to_string().contains("synthetic-updated"));
    fx.remove_dir();
}

#[test]
fn an_iphone_approved_fill_goes_out_once() {
    let _g = serial();
    reset();
    let (mac, phone, item) = paired();
    let c = candidates(&mac, "https://example.test", "https://example.test/login");
    assert_eq!(c["accounts"][0]["ref"], item.as_str(), "{c}");
    let a = mac.op(json!({"op": "browser_authorize", "request_id": c["request_id"], "ref": item, "method": "iphone"}));
    assert!(a["approval_id"].is_string(), "{a}");
    assert_eq!(mac.op(json!({"op": "browser_fill", "request_id": c["request_id"]}))["pending"], true);
    // The phone approves over the channel (Phase G).
    let begun = phone.op(json!({"op": "approvals_fetch_begin"}));
    let answer = relay(&mac, &b64::decode(begun["request"].as_str().unwrap()).unwrap()).unwrap();
    let list = phone.op(json!({"op": "approvals_fetch_step", "response": b64::encode(&answer)}));
    assert_eq!(list["approvals"][0]["origin"], "https://example.test");
    let d = phone.op(json!({"op": "approval_decide", "request_id": a["approval_id"], "approve": true}));
    let answer = relay(&mac, &b64::decode(d["request"].as_str().unwrap()).unwrap()).unwrap();
    assert_eq!(phone.op(json!({"op": "approval_submit_step", "response": b64::encode(&answer)}))["result"], "approved");
    let f = mac.op(json!({"op": "browser_fill", "request_id": c["request_id"]}));
    assert!(f["fill"]["password"].is_string(), "{f}");
    assert_eq!(mac.op(json!({"op": "browser_fill", "request_id": c["request_id"]}))["error"], "APPROVAL_REPLAY");
    mac.remove_dir();
    phone.remove_dir();
}

#[test]
fn candidate_queries_are_rate_limited() {
    let _g = serial();
    reset();
    let fx = fx();
    setup_and_unlock(&fx);
    let answers: Vec<Value> = (0..21).map(|_| candidates(&fx, "https://example.test", "https://example.test/")).collect();
    assert!(answers[..20].iter().all(|a| a["ok"] == true));
    assert_eq!(answers[20]["error"], "PEER_LIMIT", "§9.9: 20 a minute");
    fx.remove_dir();
}
