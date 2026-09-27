//! Semantic checks of the spec v0.4 vector families (§16.8, CR-13):
//! RFC 9180 A.3 DeriveKeyPair and RFC 6979 A.2.5 conformance, request
//! signatures that verify, object rejections with their stated codes, and
//! independently recomputed recovery-auth compositions.

use serde_json::Value;
use sha2::{Digest, Sha256};
use vault_helper::backup::object;
use vault_helper::crypto::recovery_auth::{self, RecoveryClass};
use vault_helper::crypto::secret::SecretBytes;
use vault_helper::crypto::{ecdsa, hex};

fn committed(stem: &str) -> Value {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/vectors/");
    serde_json::from_str(&std::fs::read_to_string(format!("{path}{stem}.json")).expect("vector file")).unwrap()
}

fn h(v: &Value) -> Vec<u8> {
    hex::decode(v.as_str().unwrap()).unwrap()
}

#[test]
fn rfc9180_and_rfc6979_conformance() {
    let doc = committed("xv_recovery_auth");
    for row in doc["rfc9180_a3"].as_array().unwrap() {
        let k = recovery_auth::derive_key_pair(&h(&row["ikm"])).unwrap();
        assert_eq!(hex::encode(k.public), row["pk"].as_str().unwrap());
    }
    // A.2.5: r exact; s as published or its low-S form n − s (the §2.7
    // producer rule normalizes), and the published form must verify.
    let rfc = &doc["rfc6979_a2_5"];
    let x: [u8; 32] = h(&rfc["x"]).try_into().unwrap();
    let (sk, pk) = ecdsa::dev_keypair_from_scalar(x);
    for msg in ["sample", "test"] {
        let digest: [u8; 32] = Sha256::digest(msg.as_bytes()).into();
        let ours = ecdsa::dev_sign_prehash(&sk, &digest);
        let published: [u8; 64] = h(&rfc[msg]).try_into().unwrap();
        assert_eq!(ours[..32], published[..32], "{msg}: r");
        let normalized = ecdsa::normalize_low_s(&published).unwrap();
        assert_eq!(ours, normalized, "{msg}: s (low-S)");
        ecdsa::verify_prehash(&pk, &digest, &ours).unwrap();
    }
}

#[test]
fn recovery_auth_compositions_recompute() {
    for row in committed("xv_recovery_auth")["composition"].as_array().unwrap() {
        let class = RecoveryClass::from_code(row["class"].as_u64().unwrap() as u8).unwrap();
        let secret = SecretBytes::new(h(&row["secret"]).try_into().unwrap());
        let salt: [u8; 16] = h(&row["auth_salt"]).try_into().unwrap();
        let vid: [u8; 16] = h(&row["vault_id"]).try_into().unwrap();
        let k = recovery_auth::derive(class, &secret, &salt, &vid).unwrap();
        assert_eq!(hex::encode(k.public), row["pub"].as_str().unwrap());
        let digest: [u8; 32] = h(&row["prehash"]).try_into().unwrap();
        assert_eq!(hex::encode(k.sign_prehash(&digest)), row["signature"].as_str().unwrap(), "RFC 6979 determinism");
        ecdsa::verify_prehash(&k.public, &digest, &h(&row["signature"])).unwrap();
    }
}

#[test]
fn request_signatures_verify() {
    let doc = committed("xv_reqsig");
    for (who, sig) in [("device", "signature_verify_only"), ("recovery", "signature")] {
        let d = &doc[who];
        let key = d.get("sign_pub").or(d.get("pub")).unwrap();
        let prehash: [u8; 32] = h(&d["prehash"]).try_into().unwrap();
        assert_eq!(vault_proto::request::prehash_tlv(&h(&d["tlv"])), prehash, "{who}");
        vault_proto::request::ProviderRequest::decode(&h(&d["tlv"])).unwrap();
        ecdsa::verify_prehash(&h(key), &prehash, &h(&d[sig])).unwrap();
    }
}

#[test]
fn object_rejections_match() {
    let doc = committed("xv_obj");
    let bytes = h(&doc["object"]);
    assert_eq!(hex::encode(object::blob_hash(&bytes)), doc["blob_hash"].as_str().unwrap());
    assert_eq!(object::encode(&object::decode(&bytes).unwrap()).unwrap(), bytes);
    for r in doc["rejections"].as_array().unwrap() {
        let got = object::decode(&h(&r["object"])).err().map(|e| e.as_str());
        assert_eq!(got, r["error"].as_str(), "{}", r["name"]);
    }
}
