//! Re-sealing device envelopes as part of a VK rotation (§2.10, §11.4).
//!
//! A rotation that changed the VK without rewriting the envelopes would
//! leave every enrolled device holding a wrap of a dead key, so the
//! envelopes are staged through the rotation journal alongside the wraps:
//! the new VK and the new envelopes commit in the same pass, or neither
//! does. Each envelope keeps the enrollment nonce it was first bound to,
//! so re-sealing changes only the VK inside (§2.10).

use std::path::Path;

use super::envelope::{self, DEVICES_DIR};
use crate::crypto::hex;
use crate::crypto::secret::SecretBytes;
use crate::crypto::wrap::DeviceEnvelopePayload;
use crate::errors::ErrorCode;
use crate::storage::rotation::ExtraStaging;
use crate::storage::rotation_journal::next_path;
use crate::storage::store::{now_epoch, WRAPS_DIR};

/// Every device that must still be able to open its envelope after the
/// rotation, with the agreement key from the verified registry.
pub struct EnvelopePlan {
    pub vault_id: [u8; 16],
    pub devices: Vec<([u8; 16], [u8; 65])>,
    /// Devices getting their first envelope here (a recovery's new
    /// device), with the enrollment nonce to bind it to.
    pub fresh: Vec<([u8; 16], [u8; 65], [u8; 16])>,
}

fn staged_name(device_id: &[u8; 16]) -> String {
    format!("{WRAPS_DIR}/{DEVICES_DIR}/{}.wrap", hex::encode(device_id))
}

impl ExtraStaging for EnvelopePlan {
    fn stage(
        &self,
        dir: &Path,
        new_vk: &SecretBytes<32>,
        new_vk_generation: u32,
    ) -> Result<Vec<String>, ErrorCode> {
        let mut names = Vec::new();
        let all = self
            .devices
            .iter()
            .map(|(id, agree)| (id, agree, None))
            .chain(self.fresh.iter().map(|(id, agree, n)| (id, agree, Some(*n))));
        for (device_id, agree_pub, fresh_nonce) in all {
            // Keep the envelope bound to the same enrollment instance.
            let nonce = match (fresh_nonce, envelope::read_envelope(dir, device_id)) {
                (Some(n), _) => n,
                (None, Ok(existing)) => {
                    hex::decode_array::<16>(&existing.enrollment_nonce).ok_or(ErrorCode::WrapCorrupt)?
                }
                (None, Err(_)) => return Err(ErrorCode::RotationFailed),
            };
            let payload = DeviceEnvelopePayload {
                vk: SecretBytes::new(*new_vk.expose()),
                wrapped_at: now_epoch(),
                vk_generation: new_vk_generation,
            };
            let file =
                envelope::seal_envelope(agree_pub, &self.vault_id, device_id, &nonce, &payload)?;
            let name = staged_name(device_id);
            let path = next_path(dir, &name);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).map_err(|_| ErrorCode::Internal)?;
            }
            crate::storage::store::write_atomic(
                &path,
                &serde_json::to_vec_pretty(&file).map_err(|_| ErrorCode::Internal)?,
            )?;
            names.push(name);
        }
        Ok(names)
    }
}
