//! PR-01 canary scan (spec v0.4 §11.4, §16): every IPC frame main and
//! the helper exchange, every helper event, and every HTTP request and
//! response main sends to the provider are recorded across setup, the
//! `create` while LOCKED, publish, sync, BK-28 `HANDLE_TAKEN` + retry
//! through the coordinator, and total-loss recovery. The canaries — the
//! MP, PK, VK, the RK (bytes and words) and both classes' `ikm_c` — appear
//! in none of it, raw or encoded. (A record's own fields cross IPC once,
//! main → helper, by design, §1.5 `add_item`; BK-18 covers provider
//! storage.) Synthetic data only.

#[path = "../../vault-helper/tests/vault_fx/mod.rs"]
mod vault_fx;
mod mfx;

use std::sync::Mutex;

use serde_json::{json, Value};
use vault_coordinator::flows::Flows;
use vault_coordinator::{Failure, Helper, HttpResponse, Transport, TransportError};
use vault_fx::{submitted, Fx};
use vault_helper::crypto::recovery_auth::RecoveryClass;
use vault_helper::crypto::{hex, kdf};
use vault_helper::storage::header::kdf_params;
use vault_helper::storage::VaultStore;

const HANDLE: &str = "synthetic-transcript@example.test";

#[derive(Default)]
struct Tape(Mutex<Vec<u8>>);
impl Tape {
    fn put(&self, b: &[u8]) {
        let mut t = self.0.lock().unwrap();
        t.extend_from_slice(b);
        t.push(b'\n');
    }
}

struct Rec<'a>(&'a Fx, &'a Tape);
impl Helper for Rec<'_> {
    fn op(&self, frame: Value) -> Result<Value, String> {
        self.1.put(&serde_json::to_vec(&frame).unwrap());
        let r = self.0.op(frame);
        self.1.put(&serde_json::to_vec(&r).unwrap());
        Ok(r)
    }
}

struct Net<'a>(&'a mfx::Cloud, &'a Tape);
impl Transport for Net<'_> {
    fn send(&self, _origin: &str, method: &str, path: &str, auth: Option<&str>, body: &[u8]) -> Result<HttpResponse, TransportError> {
        self.1.put(format!("{method} {path} {}", auth.unwrap_or("")).as_bytes());
        self.1.put(body);
        let r = self.0.send(method, path, auth, body);
        self.1.put(&r.body);
        Ok(HttpResponse { status: r.status, body: r.body, date: Some(mfx::now()) })
    }
}

fn forms(secret: &[u8]) -> Vec<Vec<u8>> {
    vec![secret.to_vec(), hex::encode(secret).into_bytes(), vault_proto::b64::encode(secret).into_bytes()]
}

#[test]
fn pr01_no_canary_in_any_transcript() {
    let _g = vault_fx::serial();
    let cloud = mfx::Cloud::new("pr01");
    let tape = Tape::default();
    let net = Net(&cloud, &tape);

    // A: setup, create while LOCKED, unlock, add, publish, sync.
    let a = vault_fx::fx();
    let fa = Flows { helper: &Rec(&a, &tape), transport: &net };
    a.push_panel(submitted(vault_fx::MP));
    let setup = fa.helper.op(json!({ "op": "setup_vault", "handle": HANDLE })).unwrap();
    assert_eq!(fa.run_publication(&setup["publication"]).unwrap()["committed"], true);
    let words = a.panel.shown_rk.lock().unwrap().clone().expect("RK shown");
    a.push_panel(submitted(vault_fx::MP));
    assert_eq!(fa.helper.op(json!({"op": "begin_recovery_unlock", "kind": "mp"})).unwrap()["ok"], true);
    fa.helper.op(json!({"op": "add_item", "kind": "login", "title": "canary", "username": "u@example.test", "hosts": ["example.test"], "password": "synthetic-canary-pw-5d1e"})).unwrap();
    assert_eq!(fa.backup_now().unwrap()["committed"], true);
    fa.run_sync().unwrap();
    let vk = vault_helper::crypto::secret::SecretBytes::new(*a.core.lock().unwrap().vk.as_ref().unwrap().expose());

    // B: the same handle is taken; retry with a new one (BK-28).
    let b = vault_fx::fx();
    let fb = Flows { helper: &Rec(&b, &tape), transport: &net };
    b.push_panel(submitted(vault_fx::MP));
    let setup_b = fb.helper.op(json!({ "op": "setup_vault", "handle": HANDLE })).unwrap();
    match fb.run_publication(&setup_b["publication"]) {
        Err(Failure::Provider(409, code)) => assert_eq!(code, "HANDLE_TAKEN"),
        other => panic!("expected HANDLE_TAKEN, got {other:?}"),
    }
    vault_fx::unlock(&b, vault_fx::MP);
    b.push_panel(submitted(vault_fx::MP));
    let retry = fb.helper.op(json!({ "op": "setup_retry_handle", "handle": "synthetic-transcript-two@example.test" })).unwrap();
    assert_eq!(retry["ok"], true, "{retry}");
    assert_eq!(fb.run_publication(&retry["publication"]).unwrap()["committed"], true);
    let st = fb.helper.op(json!({"op": "remote_update_status"})).unwrap();
    assert_eq!(st["pending"], false, "the retried create committed: {st}");

    // C: total-loss recovery of A's vault with the MP.
    let c = vault_fx::fx();
    let fc = Flows { helper: &Rec(&c, &tape), transport: &net };
    c.push_panel(submitted(vault_fx::MP));
    fc.recovery_start(mfx::ORIGIN, HANDLE, "mp").unwrap();
    assert_eq!(fc.recovery_finish().unwrap()["committed"], true);

    // The canaries.
    let h = VaultStore::open(&a.dir).unwrap().header;
    let vid = h.vault_id.0;
    let pk = kdf::derive_pk(vault_fx::MP, &h.kdf.salt.0, kdf_params(&h.kdf)).unwrap();
    let rk = vault_helper::crypto::bip39::decode_rk(&words).unwrap();
    let ikm = |class: RecoveryClass, s: &[u8], salt: &[u8; 16]| {
        let mut info = class.domain().to_vec();
        info.extend_from_slice(&vid);
        vault_proto::crypto::hkdf::hkdf32(s, salt, &info).unwrap()
    };
    let ikm_mp = ikm(RecoveryClass::Mp, pk.expose(), &h.auth_salt_mp.0);
    let ikm_rk = ikm(RecoveryClass::Rk, rk.expose(), &h.auth_salt_rk.0);
    let mut canaries: Vec<Vec<u8>> = Vec::new();
    for s in [vault_fx::MP, pk.expose(), vk.expose(), rk.expose(), ikm_mp.expose(), ikm_rk.expose()] {
        canaries.extend(forms(s));
    }
    canaries.push(words.clone().into_bytes());
    for f in [&a, &b, &c] {
        for e in f.events.log.lock().unwrap().iter() {
            tape.put(&serde_json::to_vec(e).unwrap());
        }
    }
    let t = tape.0.lock().unwrap();
    assert!(t.len() > 10_000, "the transcript covers the flows ({} bytes)", t.len());
    for (i, c) in canaries.iter().enumerate() {
        assert!(!t.windows(c.len()).any(|w| w == c.as_slice()), "canary #{i} appears in the transcript");
    }
    a.remove_dir();
    b.remove_dir();
    c.remove_dir();
}
