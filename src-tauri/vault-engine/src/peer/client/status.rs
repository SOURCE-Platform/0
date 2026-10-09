//! `peer_status` on the phone (spec v0.5 §22.9). The Mac's local chain
//! must extend the phone's committed chain; the extension is verified
//! entry by entry (a new `recovery_epoch` only under the VK the phone holds
//! for its last accepted manifest). If it revokes this phone — published
//! (`seq ≤ committed_seq`) or still among the Mac's own entries — the vault
//! locks either way; only a provider-confirmed state can lift that.
//! Nothing here moves the committed tier (§22.5: provisional).

use serde::{Deserialize, Serialize};
use vault_proto::crypto::registry::{entry_hash, EntryKind};
use vault_proto::peer::body::Status;

use crate::crypto::secret::SecretBytes;
use crate::errors::ErrorCode;
use crate::registry::chain::{apply_with, EpochContext, EpochPolicy, RegistryState};
use crate::registry::file;

/// How the Mac's chain stands for this phone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Standing {
    Active,
    /// Revoked in the Mac's committed entries or its own unpublished ones;
    /// `published` says which (§22.9 copy: "removal pending" until
    /// `BACKUP_ACCESS_LOST` corroborates it).
    Removed { published: bool },
}

/// The phone's own key for its last accepted manifest, and nothing else.
struct Held<'a> {
    manifest_hash: Option<[u8; 32]>,
    vk: &'a SecretBytes<32>,
}

impl EpochContext for Held<'_> {
    fn vk_for_manifest(&self, h: &[u8; 32]) -> Option<SecretBytes<32>> {
        (self.manifest_hash == Some(*h)).then(|| SecretBytes::new(*self.vk.expose()))
    }
    fn manifest_acceptable(&self, h: &[u8; 32]) -> bool {
        self.manifest_hash == Some(*h)
    }
}

pub fn check(committed: &RegistryState, vault_id: &[u8; 16], me: &[u8; 16], manifest_hash: Option<[u8; 32]>, vk: &SecretBytes<32>, body: &[u8]) -> Result<Standing, ErrorCode> {
    let bad = ErrorCode::PeerAuthInvalid;
    let s = Status::decode(body).map_err(|_| bad)?;
    if &s.vault_id != vault_id {
        return Err(bad);
    }
    let entries = file::decode(&s.registry).map_err(|_| bad)?;
    let n = committed.entries.len();
    if entries.len() < n || s.committed_seq >= entries.len() as u64 {
        return Err(bad);
    }
    // The Mac's chain extends the committed one, entry for entry.
    for (mine, theirs) in committed.entries.iter().zip(&entries) {
        if entry_hash(mine).map_err(|_| bad)? != entry_hash(theirs).map_err(|_| bad)? {
            return Err(bad);
        }
    }
    let mut st = committed.clone();
    let held = Held { manifest_hash, vk };
    for e in &entries[n..] {
        apply_with(&mut st, e, vault_id, &EpochPolicy::RequireProof(&held)).map_err(|_| bad)?;
    }
    let revocation = entries.iter().find(|e| e.kind == EntryKind::Revoke && &e.device_id == me);
    Ok(match revocation {
        Some(e) => Standing::Removed { published: e.seq <= s.committed_seq },
        None => Standing::Active,
    })
}
