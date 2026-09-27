//! U-1 (phase-f-design-closure.md): Secure Enclave signing latency per
//! provider request on this Mac. Every provider request is signed by the
//! device's SE key; above ~20 ms/request the design adds batch blob
//! uploads. Run explicitly (it creates and destroys a synthetic SE key):
//!
//!   cargo test -p source-vault-helper --test se_latency -- --ignored --nocapture

use std::collections::BTreeSet;
use std::time::Instant;

use vault_helper::device::{se, SeDevice};
use vault_helper::registry::device::PLATFORM_MACOS;
use vault_helper::sync::sign::{sign, Key, SignRequest, SignScope};
use vault_proto::request::{body_hash, Operation};

#[test]
#[ignore = "hardware measurement; run explicitly"]
fn u1_se_signing_latency() {
    vault_helper::test_support::init_test_namespace();
    let dir = std::env::temp_dir().join(format!("vh-u1-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let dev = SeDevice::create(&dir, "Synthetic U-1", PLATFORM_MACOS).unwrap();
    let put: BTreeSet<[u8; 32]> = (0..200u8).map(|i| [i; 32]).collect();
    let scope = SignScope { put_blobs: &put, staged: None };
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
    let mut samples = Vec::new();
    for i in 0..200u8 {
        let req = SignRequest { operation: Operation::BlobPut, blob: Some([i; 32]), body_sha256: body_hash(&[i]), expected_state: None };
        let t = Instant::now();
        sign("https://provider.test", [0xa0; 16], Key::Device(&dev), &req, &scope, 0, now).unwrap();
        samples.push(t.elapsed().as_secs_f64() * 1000.0);
    }
    se::delete_keys(dev.key_tag());
    std::fs::remove_dir_all(&dir).ok();
    samples.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let mean = samples.iter().sum::<f64>() / samples.len() as f64;
    let p = |q: f64| samples[((samples.len() - 1) as f64 * q) as usize];
    println!("U-1 SE signing: n={} mean={mean:.2} ms p50={:.2} p95={:.2} max={:.2}", samples.len(), p(0.5), p(0.95), p(1.0));
}
