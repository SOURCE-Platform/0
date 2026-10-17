//! The §16 browser rules BR-01…BR-19, one test per rule the helper
//! decides (spec §9.3–§9.9). BR-08 (iframes) and BR-15 (Rust vs JS PSL)
//! live in the extension: see `docs/security/phase-h-design.md`.
//! Synthetic data only.

mod device_fx;
mod join_fx;
mod sync_fx;
mod vault_fx;

use serde_json::{json, Value};
use sync_fx::*;
use vault_fx::*;
use vault_proto::b64;

fn login(fx: &Fx, host: &str, mode: &str, http: bool) -> String {
    let r = fx.op(json!({"op": "add_item", "kind": "login", "title": "T", "username": "alice@example.test", "password": "synthetic-old", "urls": [{"host": host, "match": mode, "allow_http": http}]}));
    assert_eq!(r["ok"], true, "{r}");
    r["ref"].as_str().unwrap().to_string()
}

fn cands(fx: &Fx, origin: &str) -> Value {
    fx.op(json!({"op": "browser_candidates", "origin": origin, "tab_url": format!("{origin}/login")}))
}

fn n(v: &Value) -> usize {
    v["accounts"].as_array().map_or(0, Vec::len)
}

fn vault() -> Fx {
    vault_helper::vault::browser_ops::reset();
    vault_helper::approval::reset();
    let fx = fx();
    setup_and_unlock(&fx);
    fx
}

#[test]
fn br_01_to_07_and_09_the_matching_table() {
    let _g = serial();
    let fx = vault();
    login(&fx, "github.com", "exact", false);
    login(&fx, "example.com", "domain", false);
    login(&fx, "192.168.1.10", "exact", true);
    login(&fx, "google.example", "exact", false);
    assert_eq!(n(&cands(&fx, "https://github.com")), 1, "BR-01");
    let c = cands(&fx, "https://app.github.com");
    assert_eq!((n(&c), c["similar"].as_u64()), (0, Some(1)), "BR-02: {c}");
    assert_eq!(n(&cands(&fx, "https://app.example.com")), 1, "BR-03");
    assert_eq!(n(&cands(&fx, "https://github.com.evil.example")), 0, "BR-04");
    let c = cands(&fx, "https://xn--80ak6aa92e.com");
    assert_eq!((n(&c), c["warning"].as_str()), (0, Some("lookalike")), "BR-05: {c}");
    let c = cands(&fx, "https://g00gle.example");
    assert_eq!((n(&c), c["warning"].as_str()), (0, Some("lookalike")), "BR-06: {c}");
    assert_eq!(n(&cands(&fx, "http://github.com")), 0, "BR-07: https → http refused");
    assert_eq!(n(&cands(&fx, "http://192.168.1.10")), 1, "BR-07: the local allow_http exception");
    for opaque in ["data:text/html,x", "file:///tmp/x", "about:blank", "null"] {
        let r = fx.op(json!({"op": "browser_candidates", "origin": opaque, "tab_url": opaque}));
        assert_eq!(r["ok"], false, "BR-09 {opaque}: {r}");
    }
    assert!(cands(&fx, "https://github.com")["warning"].is_null(), "no banner on the real site");
    fx.remove_dir();
}

#[test]
fn br_10_11_13_14_one_shot_bound_and_locked() {
    let _g = serial();
    let fx = vault();
    let r = login(&fx, "github.com", "exact", false);
    let other = login(&fx, "other.example.test", "exact", false);
    let c = cands(&fx, "https://github.com");
    let auth = |reff: &str| fx.op(json!({"op": "browser_authorize", "request_id": c["request_id"], "ref": reff, "method": "local"}));
    assert_eq!(auth(&other)["error"], "APPROVAL_MISMATCH", "BR-13");
    assert_eq!(auth(&r)["fill"]["password"], "synthetic-old");
    assert_eq!(auth(&r)["error"], "APPROVAL_REPLAY", "BR-10");
    let lie = fx.op(json!({"op": "browser_candidates", "origin": "https://github.com", "tab_url": "https://evil.test/"}));
    assert_eq!(lie["error"], "APPROVAL_MISMATCH", "BR-11");
    fx.core.lock().unwrap().lock(vault_helper::vault::LockReason::Explicit);
    let locked = cands(&fx, "https://github.com");
    assert_eq!((locked["locked"].as_bool(), n(&locked)), (Some(true), 0), "BR-14");
    fx.remove_dir();
}

#[test]
fn br_12_rate_limit() {
    let _g = serial();
    let fx = vault();
    let all: Vec<Value> = (0..21).map(|_| cands(&fx, "https://example.test")).collect();
    assert!(all[..20].iter().all(|a| a["ok"] == true));
    assert_eq!(all[20]["error"], "PEER_LIMIT", "BR-12");
    fx.remove_dir();
}

fn password(fx: &Fx, r: &str) -> Value {
    fx.op(json!({"op": "reveal", "ref": r}))["secret"]["password"].clone()
}

fn history(fx: &Fx, r: &str) -> Vec<String> {
    let c = fx.core.lock().unwrap();
    let tip = c.store.as_ref().unwrap().read_tip(c.vk.as_ref().unwrap(), r).unwrap();
    let rec: Value = serde_json::from_slice(&tip.plaintext).unwrap();
    rec["password_history"].as_array().unwrap().iter().map(|h| h["password"].as_str().unwrap().to_string()).collect()
}

#[test]
fn br_16_17_updates_need_a_trusted_confirmation() {
    let _g = serial();
    let fx = vault();
    let r = login(&fx, "shop.example.test", "exact", false);
    let page = json!({"origin": "https://shop.example.test", "tab_url": "https://shop.example.test/account"});
    let update = |method: &str| {
        let mut f = page.clone();
        f.as_object_mut().unwrap().extend(json!({"op": "browser_save_update", "ref": r, "password": "synthetic-new", "method": method}).as_object().unwrap().clone());
        fx.op(f)
    };
    // No iPhone in this vault: the iPhone path is refused (VER-O3) and the
    // record stays as it was; held-pending is BR-16 in the paired test.
    assert_eq!(update("iphone")["error"], "DEVICE_NOT_AUTHORIZED");
    assert_eq!(password(&fx, &r), "synthetic-old");
    assert_eq!(update("local")["ok"], true);
    let reason = LAST_REASON.lock().unwrap().clone();
    assert_eq!(password(&fx, &r), "synthetic-new", "BR-17: applied");
    assert_eq!(history(&fx, &r), ["synthetic-old"], "BR-17: old password in history");
    assert!(reason.contains("alice@example.test") && reason.contains("shop.example.test"), "{reason}");
    assert!(!reason.contains("synthetic-"), "BR-17: the sheet shows no password: {reason}");
    fx.remove_dir();
}

#[test]
fn br_18_19_iphone_update_approvals_are_bound_and_one_shot() {
    let _g = serial();
    vault_helper::vault::browser_ops::reset();
    vault_helper::approval::reset();
    let (mac, phone, item) = paired();
    let other = mac.op(json!({"op": "add_item", "kind": "login", "title": "O", "username": "bob@example.test", "password": "synthetic-o", "hosts": ["example.test"]}))["ref"].as_str().unwrap().to_string();
    let page = |extra: Value| {
        let mut f = json!({"origin": "https://example.test", "tab_url": "https://example.test/account"});
        f.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
        f
    };
    let a = mac.op(page(json!({"op": "browser_save_update", "ref": item, "password": "synthetic-new", "method": "iphone"})));
    let id = a["approval_id"].as_str().unwrap().to_string();
    assert_ne!(password(&mac, &item), "synthetic-new", "BR-16: held pending, record unchanged");
    let begun = phone.op(json!({"op": "approvals_fetch_begin"}));
    let answer = relay(&mac, &b64::decode(begun["request"].as_str().unwrap()).unwrap()).unwrap();
    phone.op(json!({"op": "approvals_fetch_step", "response": b64::encode(&answer)}));
    let d = phone.op(json!({"op": "approval_decide", "request_id": id, "approve": true}));
    let answer = relay(&mac, &b64::decode(d["request"].as_str().unwrap()).unwrap()).unwrap();
    assert_eq!(phone.op(json!({"op": "approval_submit_step", "response": b64::encode(&answer)}))["result"], "approved");
    let finish = |r: &str| mac.op(page(json!({"op": "browser_save_update_finish", "ref": r, "password": "synthetic-new", "approval_id": id})));
    assert_eq!(finish(&other)["error"], "APPROVAL_MISMATCH", "BR-18: another login");
    let wrong_site = mac.op(json!({"op": "browser_save_update_finish", "origin": "https://evil.test", "tab_url": "https://evil.test/", "ref": item, "password": "synthetic-new", "approval_id": id}));
    assert_eq!(wrong_site["error"], "APPROVAL_MISMATCH", "BR-18: another site");
    assert_eq!(finish(&item)["ok"], true);
    assert_eq!(finish(&item)["error"], "APPROVAL_REPLAY", "BR-19: a replayed approval applies nothing");
    assert_eq!(password(&mac, &item), "synthetic-new");
    mac.remove_dir();
    phone.remove_dir();
}

/// §9.3 `request_unlock`: a locked vault asks the app to come forward,
/// at most once per 10 s; an open vault asks nothing.
#[test]
fn request_unlock_asks_the_app_once_in_a_while() {
    let _g = serial();
    let fx = vault();
    let asked = |fx: &Fx| fx.events_named("unlock_requested").len();
    assert_eq!(fx.op(json!({"op": "browser_request_unlock"}))["vault_state"], "unlocked");
    assert_eq!(asked(&fx), 0);
    fx.core.lock().unwrap().lock(vault_helper::vault::LockReason::Explicit);
    for _ in 0..3 {
        assert_eq!(fx.op(json!({"op": "browser_request_unlock"}))["vault_state"], "locked");
    }
    assert_eq!(asked(&fx), 1, "one request per 10 s");
    fx.remove_dir();
}

/// §22.14 (final review VER-B1): a store behind the floor takes no browser
/// save — the extension's saves are refused like every other write.
#[test]
fn browser_saves_are_refused_while_behind() {
    let _g = serial();
    let fx = vault();
    let r = login(&fx, "shop.example.test", "exact", false);
    fx.core.lock().unwrap().behind = true;
    let page = json!({"origin": "https://shop.example.test", "tab_url": "https://shop.example.test/a"});
    for extra in [
        json!({"op": "browser_save_new", "username": "new@example.test", "password": "synthetic-n"}),
        json!({"op": "browser_save_update", "ref": r, "password": "synthetic-u", "method": "local"}),
        json!({"op": "browser_save_update_finish", "ref": r, "password": "synthetic-u", "approval_id": "00000000000000000000000000000000"}),
    ] {
        let mut f = page.clone();
        f.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
        assert_eq!(fx.op(f)["error"], "VAULT_BEHIND");
    }
    fx.core.lock().unwrap().behind = false;
    assert_eq!(password(&fx, &r), "synthetic-old");
    fx.remove_dir();
}
