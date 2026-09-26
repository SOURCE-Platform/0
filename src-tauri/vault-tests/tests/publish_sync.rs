//! BK-01 (publish → provider → another path reads it back), BK-09/BK-17
//! merge-then-republish, and the create flow end to end: the helper engine
//! staging and signing, the provider core validating and committing.
//! Synthetic data only.

mod mfx;

use mfx::*;

#[test]
fn setup_publish_and_resync() {
    let cloud = Cloud::new("e2e1");
    let mut mac = Mac::new("e2e1-mac");
    mac.setup(&cloud, "synthetic-e2e1@example.test").expect("create");
    mac.add("alpha");
    mac.add("beta");
    mac.publish(&cloud).expect("publish");
    // The provider now serves exactly what this Mac published.
    assert!(mac.sync(&cloud).unwrap().is_none(), "up to date");
    let g = mac.read(&cloud, vault_proto::request::Operation::StateGet, None);
    let v: serde_json::Value = serde_json::from_slice(&g.body).unwrap();
    assert_eq!(v["generation"], 2);
}
