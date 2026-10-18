//! PA-06 (§22.8, §2.9): a signature made for one purpose never verifies
//! for another. A peer request's signature does not verify as a provider
//! request, a peer response or an approval over the same bytes; a provider
//! request's signature does not verify as a peer message. The committed
//! vectors' real signatures are used. Synthetic keys only.

use serde_json::Value;
use sha2::{Digest, Sha256};
use vault_helper::crypto::{ecdsa, hex};

fn file(stem: &str) -> Value {
    let raw = std::fs::read_to_string(format!("{}/tests/vectors/{stem}.json", env!("CARGO_MANIFEST_DIR"))).unwrap();
    serde_json::from_str(&raw).unwrap()
}

fn b(v: &Value) -> Vec<u8> {
    hex::decode(v.as_str().unwrap()).unwrap()
}

fn domain(prefix: &[u8], tlv: &[u8]) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(prefix);
    h.update(tlv);
    h.finalize().into()
}

const PEER_REQ: &[u8] = b"ov0/peer/request/v1";
const PEER_RESP: &[u8] = b"ov0/peer/response/v1";
const PROVIDER: &[u8] = b"ov0/provider/request/v2";
const APPROVAL: &[u8] = b"ov0/approval/sign/v1";

#[test]
fn pa06_signatures_never_cross_purposes() {
    let peer = file("xv_peer");
    let phone_pub = b(&peer["keys"]["phone_pub"]);
    let req = &peer["exchanges"][0]["request"];
    let (tlv, sig) = (b(&req["tlv"]), b(&req["signature"]));
    assert_eq!(domain(PEER_REQ, &tlv).to_vec(), b(&req["prehash"]), "the vector's own domain");
    ecdsa::verify_prehash(&phone_pub, &domain(PEER_REQ, &tlv), &sig).unwrap();
    for other in [PEER_RESP, PROVIDER, APPROVAL] {
        assert!(ecdsa::verify_prehash(&phone_pub, &domain(other, &tlv), &sig).is_err(), "{}", String::from_utf8_lossy(other));
    }
    let reqsig = file("xv_reqsig");
    let d = &reqsig["device"];
    let (ptlv, psig, ppub) = (b(&d["tlv"]), b(&d["signature_verify_only"]), b(&d["sign_pub"]));
    ecdsa::verify_prehash(&ppub, &domain(PROVIDER, &ptlv), &psig).unwrap();
    for other in [PEER_REQ, PEER_RESP, APPROVAL] {
        assert!(ecdsa::verify_prehash(&ppub, &domain(other, &ptlv), &psig).is_err(), "{}", String::from_utf8_lossy(other));
    }
}
