//! §11.3.1 handle claims (HC-01…HC-05): crashes between C2/C3/C4, lost
//! responses, concurrent creates across provider instances sharing one
//! store, and a reclaim racing the owner's late retry. Synthetic data only.

mod pfx;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use pfx::*;
use vault_proto::crypto::hex;
use vault_proto::state::TransitionKind;
use vault_provider_core::claim::{claim_key, CLAIM_GRACE};
use vault_provider_core::fs::FsStores;
use vault_provider_core::stores::{Etag, OpsStore, StoreError, StoreResult};
use vault_provider_core::{Config, Provider};

type Hook = Box<dyn FnOnce() + Send>;

/// Delegating ops store with one-shot fault/race injection on claim keys.
struct HookOps {
    inner: Arc<FsStores>,
    on_claim_get: Mutex<Option<Hook>>,
    fail_bind: AtomicBool,
}

impl OpsStore for HookOps {
    fn get(&self, key: &str) -> StoreResult<Option<(Vec<u8>, Etag)>> {
        let r = self.inner.get(key);
        if key.starts_with("v2/handles/") {
            if let Some(h) = self.on_claim_get.lock().unwrap().take() {
                h();
            }
        }
        r
    }
    fn create(&self, key: &str, bytes: &[u8]) -> StoreResult<Option<Etag>> {
        self.inner.create(key, bytes)
    }
    fn replace(&self, key: &str, bytes: &[u8], etag: &Etag) -> StoreResult<Option<Etag>> {
        let binding = key.starts_with("v2/handles/") && String::from_utf8_lossy(bytes).contains("\"bound\"");
        if binding && self.fail_bind.swap(false, Ordering::SeqCst) {
            return Err(StoreError); // crash after C3, before C4
        }
        self.inner.replace(key, bytes, etag)
    }
    fn delete(&self, key: &str) -> StoreResult<()> {
        OpsStore::delete(&*self.inner, key)
    }
    fn list_prefix(&self, prefix: &str) -> StoreResult<Vec<String>> {
        self.inner.list_prefix(prefix)
    }
}

fn hooked(fs: &Arc<FsStores>) -> (Arc<Provider>, Arc<HookOps>) {
    let ops = Arc::new(HookOps { inner: fs.clone(), on_claim_get: Mutex::new(None), fail_bind: AtomicBool::new(false) });
    let p = Provider::new(Config::new(ORIGIN, [0x5e; 32]), fs.clone(), fs.clone(), ops.clone());
    (Arc::new(p), ops)
}

const HANDLE: &str = "synthetic-claims@example.test";

fn pair(tag: &str) -> (Sim, Sim) {
    let dir = tmp(tag);
    let fs = Arc::new(FsStores::new(&dir));
    (Sim::on(fs.clone(), dir.clone(), HANDLE), Sim::on(fs, dir, HANDLE))
}

fn create(s: &Sim) -> vault_provider_core::Response {
    let d = s.genesis_draft();
    let (t, _) = s.build(&d, TransitionKind::Create, &s.mac);
    s.commit(&t, Who::Dev(&s.mac))
}

fn pending_claim(s: &Sim, created_at: u64) {
    let claim = serde_json::json!({ "vault_id": hex::encode(s.vault_id), "claim_id": "ab".repeat(16), "created_at": created_at, "status": "pending" });
    s.fs.create(&claim_key(&s.handle_key), claim.to_string().as_bytes()).unwrap().unwrap();
}

/// HC-01: a crash after C2 leaves a pending claim. The same vault's retry
/// completes; another vault gets 409 within G and may reclaim after it.
#[test]
fn hc01_crash_after_claim() {
    let (a, mut b) = pair("hc01");
    pending_claim(&a, T0);
    b.now = T0 + 60;
    assert_eq!(Sim::error(&create(&b)), "HANDLE_TAKEN", "live within G");
    b.now = T0 + CLAIM_GRACE + 1;
    assert_eq!(create(&b).status, 200, "reclaimed after G");
    let (a2, _) = pair("hc01b");
    pending_claim(&a2, T0);
    assert_eq!(create(&a2).status, 200, "the owner's retry completes");
}

/// HC-02: two concurrent creates for one handle on two instances.
#[test]
fn hc02_concurrent_creates() {
    let (a, b) = pair("hc02");
    let pb = provider_on(b.fs.clone());
    let mut b = b;
    b.p = pb;
    let (ra, rb) = std::thread::scope(|sc| {
        let ha = sc.spawn(|| create(&a));
        let hb = sc.spawn(|| create(&b));
        (ha.join().unwrap(), hb.join().unwrap())
    });
    let mut codes = vec![(ra.status, Sim::error(&ra)), (rb.status, Sim::error(&rb))];
    codes.sort();
    assert_eq!(codes, vec![(200, String::new()), (409, "HANDLE_TAKEN".into())]);
}

/// HC-03: a lost create response — the retry returns the same result.
#[test]
fn hc03_lost_response() {
    let (a, _) = pair("hc03");
    let d = a.genesis_draft();
    let (t, _) = a.build(&d, TransitionKind::Create, &a.mac);
    let r1 = a.commit(&t, Who::Dev(&a.mac));
    let r2 = a.commit(&t, Who::Dev(&a.mac));
    assert_eq!((r1.status, &r1.body), (200, &r2.body));
    let (claim, _) = a.fs.get(&claim_key(&a.handle_key)).unwrap().unwrap();
    assert!(String::from_utf8_lossy(&claim).contains("\"bound\""));
}

/// HC-04: a crash after C3 (state written) before C4 (bind): another
/// vault's reclaim is refused even after G; the owner's retry binds.
#[test]
fn hc04_crash_before_bind() {
    let (mut a, mut b) = pair("hc04");
    let (p, ops) = hooked(&a.fs);
    ops.fail_bind.store(true, Ordering::SeqCst);
    a.p = p;
    let d = a.genesis_draft();
    let (t, _) = a.build(&d, TransitionKind::Create, &a.mac);
    assert_eq!(a.commit(&t, Who::Dev(&a.mac)).status, 503);
    b.now = T0 + CLAIM_GRACE + 10;
    assert_eq!(Sim::error(&create(&b)), "HANDLE_TAKEN", "claim live: its vault state exists");
    a.now = T0 + 5;
    let r = a.commit(&t, Who::Dev(&a.mac));
    assert_eq!(r.status, 200, "{}", String::from_utf8_lossy(&r.body));
}

/// HC-05: a stale pending claim; the owner's late retry reads it, then
/// another vault reclaims before the owner binds. Exactly one wins; the
/// loser's generation-1 state is rolled back.
#[test]
fn hc05_reclaim_races_late_retry() {
    let (mut a, mut b) = pair("hc05");
    pending_claim(&a, T0 - CLAIM_GRACE - 100);
    b.now = T0;
    let b = Arc::new(b);
    let (p, ops) = hooked(&a.fs);
    a.p = p;
    let b2 = b.clone();
    *ops.on_claim_get.lock().unwrap() = Some(Box::new(move || {
        assert_eq!(create(&b2).status, 200, "the reclaim wins");
    }));
    let r = create(&a);
    assert_eq!((r.status, Sim::error(&r)), (409, "HANDLE_TAKEN".into()));
    assert!(!a.dir.join(format!("v2/vaults/{}/state", hex::encode(a.vault_id))).exists(), "loser rolled back");
}
