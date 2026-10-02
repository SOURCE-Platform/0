//! `peer_state` (wire annex A.3.2; PW-04): the Mac forwards the verified
//! `state_get` body of the state it accepted, re-encoded from its verified fields, only when
//! newer than the requester's; after its own publication it has none
//! until its next provider check; objects mode answers only for exactly
//! that committed state (and holds no blobs). Synthetic data only.

#[path = "../../vault-helper/tests/vault_fx/mod.rs"]
mod vault_fx;
#[path = "../../vault-helper/tests/device_fx/mod.rs"]
mod device_fx;
#[path = "../../vault-helper/tests/peer_fx/mod.rs"]
mod peer_fx;
mod mfx;

use peer_fx::*;
use serde_json::Value;
use vault_coordinator::flows::Flows;
use vault_coordinator::{Helper, HttpResponse, Transport, TransportError};
use vault_fx::Fx;
use vault_proto::peer::exchange::StateReq;
use vault_proto::peer::{PeerOp, PeerStatus};

struct FxHelper<'a>(&'a Fx);
impl Helper for FxHelper<'_> {
    fn op(&self, frame: Value) -> Result<Value, String> {
        Ok(self.0.op(frame))
    }
}
struct Net<'a>(&'a mfx::Cloud);
impl Transport for Net<'_> {
    fn send(&self, _o: &str, method: &str, path: &str, auth: Option<&str>, body: &[u8]) -> Result<HttpResponse, TransportError> {
        let r = self.0.send(method, path, auth, body);
        Ok(HttpResponse { status: r.status, body: r.body, date: Some(mfx::now()) })
    }
}

fn state_body(cloud_net: &Net<'_>, w: &W) -> Vec<u8> {
    Flows { helper: &FxHelper(&w.fx), transport: cloud_net }.call("state_get", None, b"", None).unwrap().body
}

#[test]
fn the_mac_forwards_only_the_verified_state_it_accepted() {
    use vault_proto::peer::exchange::{ObjectItem, Objects};
    let _g = vault_fx::serial();
    let cloud = mfx::Cloud::new("peer-state");
    let net = Net(&cloud);
    let w = world("peer-state");
    let c = ctx(&w, true, false, false);
    assert_eq!(ask(&w, &c, PeerOp::State, StateReq::State { have_generation: 0 }.encode()).0, PeerStatus::NothingNewer, "no provider state yet");

    // The staged create commits; a provider check then accepts that state.
    let mac = Flows { helper: &FxHelper(&w.fx), transport: &net };
    assert_eq!(mac.backup_now().unwrap()["committed"], true);
    assert_eq!(ask(&w, &c, PeerOp::State, StateReq::State { have_generation: 0 }.encode()).0, PeerStatus::NothingNewer, "own commit: no served body");
    assert_eq!(mac.run_sync().unwrap()["up_to_date"], true);
    let (st, b) = ask(&w, &c, PeerOp::State, StateReq::State { have_generation: 0 }.encode());
    assert_eq!(st, PeerStatus::Ok);
    let served = state_body(&net, &w);
    let r = vault_helper::sync::remote::parse(&served).unwrap();
    // Re-encoded from the verified fields in the annex A.3.2 order — the
    // same state, nothing the provider added beside it.
    let forwarded = vault_helper::sync::remote::canonical(&r);
    assert_eq!(b, vault_proto::peer::exchange::encode_state(&forwarded));
    assert!(forwarded.starts_with(br#"{"generation":"#), "fixed key order");
    assert_eq!(vault_helper::sync::remote::parse(&forwarded).unwrap().state_commit, r.state_commit);
    assert_eq!(ask(&w, &c, PeerOp::State, StateReq::State { have_generation: r.generation }.encode()).0, PeerStatus::NothingNewer, "not newer");

    // Objects mode: only for exactly that state; no blobs are held here.
    let (st, b) = ask(&w, &c, PeerOp::State, StateReq::Objects { state_commit: r.state_commit, wants: vec![([1; 32], 0)] }.encode());
    assert_eq!(st, PeerStatus::Ok);
    assert_eq!(b, Objects { complete: true, items: vec![ObjectItem::Unavailable { sha256: [1; 32], reason: 2 }] }.encode(), "all unavailable here");
    assert_eq!(ask(&w, &c, PeerOp::State, StateReq::Objects { state_commit: [9; 32], wants: vec![([1; 32], 0)] }.encode()).0, PeerStatus::NothingNewer);

    // A new own publication: the kept body no longer matches → none.
    vault_fx::add_login(&w.fx);
    assert_eq!(mac.backup_now().unwrap()["committed"], true);
    assert_eq!(ask(&w, &c, PeerOp::State, StateReq::State { have_generation: 0 }.encode()).0, PeerStatus::NothingNewer);
    w.fx.remove_dir();
}
