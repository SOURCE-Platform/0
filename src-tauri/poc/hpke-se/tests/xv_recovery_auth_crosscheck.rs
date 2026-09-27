//! XV-RECOVERY-AUTH independent cross-check (spec v0.4 §16.8, CR-13 (b)):
//! the `ikm_c` of every committed composition row, run through the `hpke`
//! crate's own DHKEM(P-256, HKDF-SHA256) DeriveKeyPair, must give the
//! committed public key — so the helper's composition vectors are not
//! checked only against the implementation that produced them. Dev/test
//! only: this PoC crate is outside the shipping workspace.

use hpke::kem::{DhP256HkdfSha256, Kem};
use hpke::Serializable;

#[test]
fn composition_ikm_matches_hpke_crate() {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../vault-helper/tests/vectors/xv_recovery_auth.json");
    let doc: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let rows = doc["composition"].as_array().unwrap();
    assert_eq!(rows.len(), 2, "both classes");
    for row in rows.iter().chain(doc["rfc9180_a3"].as_array().unwrap()) {
        let ikm = hex::decode(row["ikm"].as_str().unwrap()).unwrap();
        let (_, pk) = <DhP256HkdfSha256 as Kem>::derive_keypair(&ikm);
        let want = row.get("pub").or(row.get("pk")).unwrap().as_str().unwrap();
        assert_eq!(hex::encode(pk.to_bytes()), want);
    }
}
