//! §4.8 registry checkpoint binding (CP-04, CP-05, CP-06): a checkpoint
//! verifies only under the current VK and only for exactly the manifest,
//! registry head, generation and epoch it was made for. Synthetic keys.

use vault_proto::backup::checkpoint::RegistryCheckpoint;
use vault_proto::backup::manifest::SignedManifest;
use vault_proto::crypto::secret::SecretBytes;
use vault_proto::errors::ErrorCode;
use vault_proto::registry::device::{SoftwareDevice, PLATFORM_MACOS};

fn manifest(generation: u64, head: [u8; 32]) -> SignedManifest {
    let dev = SoftwareDevice::generate("Synthetic Mac", PLATFORM_MACOS);
    SignedManifest {
        vault_id: [0xa0; 16],
        generation,
        created_at: 1_900_000_000,
        registry_head: head,
        vk_generation: 2,
        object_index_hash: [0x33; 32],
        prev_manifest_hash: [0x44; 32],
        signer_device_id: [0; 16],
        signature: [0; 64],
    }
    .sign(&dev)
    .unwrap()
}

#[test]
fn checkpoint_binds_exactly_one_state() {
    let vk = SecretBytes::new([0x55; 32]);
    let head = [0x66; 32];
    let m = manifest(7, head);
    let cp = RegistryCheckpoint::create(&vk, &m, 1).unwrap();
    assert!(cp.verify_binding(&vk, &m, &head, 1).is_ok());
    let mismatch = Err(ErrorCode::ManifestMismatch);
    // CP-04: the wrong registry head.
    assert_eq!(cp.verify_binding(&vk, &m, &[0x67; 32], 1), mismatch);
    // CP-05: another manifest generation, or another epoch.
    assert_eq!(cp.verify_binding(&vk, &manifest(8, head), &head, 1), mismatch);
    assert_eq!(cp.verify_binding(&vk, &m, &head, 2), mismatch);
    // CP-06: MAC'd under an old VK (after a rotation) — refused.
    let old = SecretBytes::new([0x54; 32]);
    let stale = RegistryCheckpoint::create(&old, &m, 1).unwrap();
    assert!(stale.verify_binding(&vk, &m, &head, 1).is_err());
    // Round trip is canonical.
    assert_eq!(RegistryCheckpoint::decode(&cp.encode()).unwrap(), cp);
}
