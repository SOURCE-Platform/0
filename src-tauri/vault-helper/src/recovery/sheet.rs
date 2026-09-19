//! The printed recovery sheet's freshness checkpoint (spec §1.7, §11.7):
//! vault_id, the manifest generation, and an 8-hex prefix of the registry
//! head as of printing / last RK rotation. At recovery the user compares
//! it with what the provider served — the user is the freshness oracle on
//! a fresh device; nothing here *detects* rollback (§11.7, FR-02/FR-03).

use crate::crypto::hex;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SheetCheckpoint {
    pub vault_id: [u8; 16],
    pub generation: u64,
    pub registry_head_prefix: String,
}

impl SheetCheckpoint {
    pub fn new(vault_id: [u8; 16], generation: u64, registry_head: &[u8; 32]) -> SheetCheckpoint {
        SheetCheckpoint { vault_id, generation, registry_head_prefix: head_prefix(registry_head) }
    }
}

pub fn head_prefix(registry_head: &[u8; 32]) -> String {
    hex::encode(&registry_head[..4])
}

/// What the recovery UI tells the user after comparing the served state
/// with their sheet (§11.7 item 2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SheetComparison {
    /// Different vault: stop.
    DifferentVault,
    /// Served state is older than the sheet: stale or rolled-back backup.
    OlderThanSheet,
    /// Same generation but a different registry head: fork evidence.
    ForkEvidence,
    /// Same generation and head prefix as the sheet.
    MatchesSheet,
    /// Newer than the sheet — normal (backups advance after printing).
    NewerThanSheet,
}

pub fn compare(sheet: &SheetCheckpoint, served_vault: &[u8; 16], served_gen: u64, served_head: &[u8; 32]) -> SheetComparison {
    if &sheet.vault_id != served_vault {
        SheetComparison::DifferentVault
    } else if served_gen < sheet.generation {
        SheetComparison::OlderThanSheet
    } else if served_gen == sheet.generation && head_prefix(served_head) != sheet.registry_head_prefix {
        SheetComparison::ForkEvidence
    } else if served_gen == sheet.generation {
        SheetComparison::MatchesSheet
    } else {
        SheetComparison::NewerThanSheet
    }
}
