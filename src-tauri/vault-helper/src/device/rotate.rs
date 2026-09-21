//! Re-sealing device envelopes as part of a VK rotation (§2.10, §11.4).
//!
//! A rotation that changed the VK without rewriting the envelopes would
//! leave every enrolled device holding a wrap of a dead key, so the
//! envelopes and the credential record are staged through the rotation
//! journal alongside the wraps: the new VK and the new envelopes commit
//! in the same pass, or neither does.
//!
//! Each device keeps the credential it was issued (§11.4: credentials
//! rotate only by re-enrollment) and the enrollment nonce its envelope
//! was first bound to, so re-sealing changes only the VK inside.

use std::path::Path;

use super::creds::{DeviceCreds, CREDS_NAME};
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
pub struct EnvelopePlan<'a> {
    pub vault_id: [u8; 16],
    pub devices: Vec<([u8; 16], [u8; 65])>,
    pub creds: &'a DeviceCreds,
}

fn staged_name(device_id: &[u8; 16]) -> String {
    format!("{WRAPS_DIR}/{DEVICES_DIR}/{}.wrap", hex::encode(device_id))
}

impl ExtraStaging for EnvelopePlan<'_> {
    fn stage(
        &self,
        dir: &Path,
        new_vk: &SecretBytes<32>,
        new_vk_generation: u32,
    ) -> Result<Vec<String>, ErrorCode> {
        let mut names = Vec::new();
        for (device_id, agree_pub) in &self.devices {
            // The credential is the one this device already holds; a
            // device we cannot re-envelope must not be silently dropped.
            let cred = self
                .creds
                .get(device_id)
                .ok_or(ErrorCode::RotationFailed)?;
            // Keep the envelope bound to the same enrollment instance.
            let nonce = match envelope::read_envelope(dir, device_id) {
                Ok(existing) => {
                    hex::decode_array::<16>(&existing.enrollment_nonce).ok_or(ErrorCode::WrapCorrupt)?
                }
                Err(_) => return Err(ErrorCode::RotationFailed),
            };
            let payload = DeviceEnvelopePayload {
                vk: SecretBytes::new(*new_vk.expose()),
                device_backup_cred: cred,
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
        // The credential record is itself VK-sealed.
        let creds_name = format!("{WRAPS_DIR}/{DEVICES_DIR}/{CREDS_NAME}");
        self.creds
            .seal_to(&next_path(dir, &creds_name), new_vk, &self.vault_id)?;
        names.push(creds_name);
        Ok(names)
    }
}
