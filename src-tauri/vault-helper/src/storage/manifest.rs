//! Local `manifest.json` (§3.1) — the local mirror of the §11.3 signed
//! manifest, minus the signature: Phase C has no enrolled device key yet
//! (Phase E), so there is nobody to sign. The mismatch refusal of §3.5
//! still applies in full: the helper refuses to open a vault whose local
//! records don't match the manifest's object list.

use serde::{Deserialize, Serialize};

use super::header::{Hex16, Hex32};
use crate::errors::ErrorCode;

pub const MANIFEST_NAME: &str = "manifest.json";
pub const REGISTRY_NAME: &str = "registry.json";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManifestObject {
    /// uuid string (§3.3).
    pub record_id: String,
    /// hex, 32 bytes.
    pub revision_id: String,
}

/// Local manifest mirror. `version` governs this file's format;
/// `manifest_generation` flips on every mutation (§2.10/§11.3).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    pub version: u32,
    pub vault_id: Hex16,
    pub manifest_generation: u64,
    pub vk_generation: u32,
    pub registry_head: Hex32,
    /// Live item count (§3.4 accepted provider metadata leak).
    pub item_count: u64,
    /// One entry per revision row in `record_revs`.
    pub objects: Vec<ManifestObject>,
}

impl Manifest {
    pub fn fresh(vault_id: Hex16) -> Self {
        Manifest {
            version: 1,
            vault_id,
            manifest_generation: 1,
            vk_generation: 1,
            registry_head: Hex32([0u8; 32]),
            item_count: 0,
            objects: Vec::new(),
        }
    }
}

pub fn parse_manifest(bytes: &[u8]) -> Result<Manifest, ErrorCode> {
    let m: Manifest = serde_json::from_slice(bytes).map_err(|_| ErrorCode::ManifestMismatch)?;
    if m.version != 1 {
        return Err(ErrorCode::FormatTooNew);
    }
    Ok(m)
}

pub fn write_manifest(m: &Manifest) -> Result<Vec<u8>, ErrorCode> {
    serde_json::to_vec_pretty(m).map_err(|_| ErrorCode::Internal)
}

/// §3.5 cross-check against the header: generation, vault id, VK
/// generation and registry head must agree, or the directory contents
/// came from different points in time.
pub fn check_against_header(m: &Manifest, h: &super::header::Header) -> Result<(), ErrorCode> {
    if m.vault_id != h.vault_id
        || m.manifest_generation != h.manifest_generation
        || m.vk_generation != h.vk_generation
        || m.registry_head != h.registry_head
    {
        return Err(ErrorCode::ManifestMismatch);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::header::Header;

    #[test]
    fn round_trip_manifest() {
        let vault_id = Hex16::random();
        let mut m = Manifest::fresh(vault_id);
        m.objects.push(ManifestObject {
            record_id: "7c9e6679-7425-40de-944b-e07fc1f90ae7".to_string(),
            revision_id: "ab".repeat(32),
        });
        let parsed = parse_manifest(&write_manifest(&m).unwrap()).unwrap();
        assert_eq!(parsed.objects, m.objects);
        assert_eq!(parsed.item_count, 0);
    }

    #[test]
    fn header_disagreement_is_manifest_mismatch() {
        let vault_id = Hex16::random();
        let h = Header::fresh(vault_id);
        let mut m = Manifest::fresh(vault_id);
        check_against_header(&m, &h).unwrap();
        m.manifest_generation = 2;
        assert_eq!(
            check_against_header(&m, &h),
            Err(ErrorCode::ManifestMismatch)
        );
        let mut m = Manifest::fresh(vault_id);
        m.registry_head = Hex32([1u8; 32]);
        assert_eq!(
            check_against_header(&m, &h),
            Err(ErrorCode::ManifestMismatch)
        );
    }

    #[test]
    fn corrupt_or_future_manifest_refused() {
        assert_eq!(parse_manifest(b"xx").unwrap_err(), ErrorCode::ManifestMismatch);
        let mut m = Manifest::fresh(Hex16::random());
        m.version = 9;
        assert_eq!(
            parse_manifest(&write_manifest(&m).unwrap()).unwrap_err(),
            ErrorCode::FormatTooNew
        );
    }
}
