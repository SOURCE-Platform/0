//! Content comparison for same-generation duplicates (spec v0.4 §3.2
//! "Duplicates"): only a VK holder can tell whether two representations
//! of one `revision_id` carry the same plaintext. Any decryption failure
//! counts as different content (fail toward freezing).

use crate::crypto::secret::SecretBytes;
use crate::errors::ErrorCode;
use crate::storage::merge::ContentCompare;
use crate::storage::revisions::RevisionRow;
use crate::storage::VaultStore;

pub struct VkCompare<'a> {
    pub store: &'a VaultStore,
    pub vk: &'a SecretBytes<32>,
}

impl ContentCompare for VkCompare<'_> {
    fn same_content(&self, a: &RevisionRow, b: &RevisionRow) -> Result<bool, ErrorCode> {
        let open = |r: &RevisionRow| -> Option<(crate::crypto::secret::SecretVec, crate::crypto::secret::SecretVec)> {
            Some((self.store.open_row(self.vk, r).ok()?, self.store.open_row_meta(self.vk, r).ok()?))
        };
        Ok(match (open(a), open(b)) {
            (Some((pa, ma)), Some((pb, mb))) => {
                use subtle::ConstantTimeEq;
                bool::from(pa.as_slice().ct_eq(pb.as_slice())) && ma.as_slice() == mb.as_slice()
            }
            _ => false,
        })
    }
}
