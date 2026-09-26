//! The local vault as a publishable state (spec v0.4 §11.2): every blob a
//! state references — header, registry, wraps, the envelopes of the
//! active devices, and every admitted revision — gathered from the vault
//! directory and staged with the shared `vault-proto` builder.

use crate::backup::stage::{stage, StateInputs, Staged};
use crate::crypto::secret::SecretBytes;
use crate::device::envelope;
use crate::errors::ErrorCode;
use crate::registry::chain::RegistryState;
use crate::registry::device::DeviceIdentity;
use crate::registry::file as registry_file;
use crate::storage::header::write_header;
use crate::storage::revision_rows;
use crate::storage::store::{now_epoch, VaultStore, PASSWORD_WRAP_NAME, RECOVERY_WRAP_NAME};

/// Stage the local vault as generation `generation`, chained to
/// `prev_manifest_hash`, signed by `signer`, checkpointed under `vk`.
/// `pending_envelopes` supplies envelopes not yet on disk (an enrollment
/// whose entry is in `registry` but not yet committed).
pub fn stage_local(
    store: &VaultStore,
    registry: &RegistryState,
    generation: u64,
    prev_manifest_hash: [u8; 32],
    signer: &dyn DeviceIdentity,
    vk: &SecretBytes<32>,
    pending_envelopes: &[([u8; 16], Vec<u8>)],
) -> Result<Staged, ErrorCode> {
    let dir = &store.dir;
    let read = |name: &str| std::fs::read(dir.join(name)).map_err(|_| ErrorCode::WrapCorrupt);
    let mut envelopes = Vec::new();
    for d in registry.devices.iter().filter(|d| !d.revoked) {
        // Every active device's envelope is part of every state (§2.10);
        // a missing one is a local inconsistency, never silently skipped.
        let bytes = match pending_envelopes.iter().find(|(id, _)| *id == d.device_id) {
            Some((_, b)) => b.clone(),
            None => std::fs::read(envelope::envelope_path(dir, &d.device_id)).map_err(|_| ErrorCode::WrapCorrupt)?,
        };
        envelopes.push((d.device_id, bytes));
    }
    let rows = revision_rows::all_rows(&store.conn)?;
    stage(
        StateInputs {
            vault_id: store.header.vault_id.0,
            generation,
            prev_manifest_hash,
            created_at: now_epoch(),
            header: write_header(&store.header)?,
            registry: registry_file::encode(&registry.entries)?,
            registry_head: registry.head,
            epoch: registry.epoch,
            vk_generation: store.header.vk_generation,
            wrap_mp: read(PASSWORD_WRAP_NAME)?,
            wrap_rk: std::fs::read(dir.join(RECOVERY_WRAP_NAME)).ok(),
            envelopes,
            revisions: &rows,
        },
        signer,
        vk,
    )
}
