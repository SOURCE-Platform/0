//! The types the helper-owned secure UI exchanges with the op layer
//! (§1.7): what a panel is asking for, what the user answered, and what a
//! Recovery Key window shows.
//!
//! None of this crosses IPC. Panel outcomes carry secrets in zeroizing
//! buffers that live and die inside the helper; the main app only ever
//! learns that a panel opened or closed (§1.5 `secure_panel_visible`).

use std::time::Duration;

use crate::crypto::secret::SecretVec;

/// What the helper-owned panel is asking for (§1.7). Secrets cross back
/// exactly once, inside zeroizing buffers, helper-internal only.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PanelRequest {
    /// Initial MP creation with confirmation field (§5.4).
    MpCreate,
    /// MP entry for the panel-based unlock path (§6 fallback).
    MpEntry,
    /// MP change: old + new + confirmation (§1.5 change_master_password).
    MpChange,
    /// Recovery Key entry: one secure field, 24 words (§1.7, §2.4).
    RkEntry,
    /// Adopting another device's key change with the master password the
    /// backup currently uses (§2.7 master-password adoption, F.2d) — its
    /// own title, never "Unlock" (review SEC-I3).
    MpAdopt,
    /// Adding a device (§5.1, §22.4): the code the new device must show,
    /// in this panel — never in the main app (owner decision 2026-10-03,
    /// review SEC-B3) — and the current MP.
    EnrollConfirm,
}

impl PanelRequest {
    /// Window title naming the requesting flow (§1.7 focus-theft rule).
    /// Carried in `secure_panel_visible` so the main app registers the
    /// exact title in the capture-exclusion registry (§14.2).
    pub fn title(self) -> &'static str {
        match self {
            PanelRequest::MpCreate => "Source Vault — Create Master Password",
            PanelRequest::MpEntry => "Source Vault — Unlock",
            PanelRequest::MpChange => "Source Vault — Change Master Password",
            PanelRequest::RkEntry => "Source Vault — Enter Recovery Key",
            PanelRequest::EnrollConfirm => "Source Vault — Add Device",
            PanelRequest::MpAdopt => "Source Vault — Apply a Security Change",
        }
    }
}

/// Title of the Recovery Key display/print window (§1.7).
pub const RK_SHEET_TITLE: &str = "Source Vault — Recovery Key";

/// What the helper's Recovery Key window shows and prints. Built and
/// consumed inside the helper only; never crosses IPC (§1.5 never-list).
pub struct RecoverySheet {
    /// The 24 words, space separated.
    pub words: zeroize::Zeroizing<String>,
    /// Non-secret freshness checkpoint line (§11.7): vault id, manifest
    /// generation, registry head prefix.
    pub checkpoint: String,
    /// FR-02: the normalized recovery handle and the provider origin, so
    /// a later recovery can be checked against the printed page.
    pub recovery: String,
    /// Why this window is on screen. A Recovery Key that appears without
    /// explanation invites the worst outcome available: keeping the old
    /// printout and discarding the new one.
    pub reason: SheetReason,
}

/// What caused a Recovery Key to be issued (§1.7 window copy).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SheetReason {
    /// First one, at vault creation (§5.4).
    VaultCreated,
    /// The user asked for a new one (§1.5 rotate_recovery_key).
    Replaced,
    /// A device was removed, which rotates the vault key (§11.4).
    DeviceRemoved,
    /// Total-loss recovery issued a new one (§11.8).
    Recovered,
    /// The first handle was taken; `setup_retry_handle` voids the first
    /// sheet (it opens only a retired key that protects nothing, §1.5).
    HandleRetried,
}

impl SheetReason {
    /// One line under the title. Every case that supersedes an existing
    /// Recovery Key says so — and, per the §1.7/§11.3.2 remote-pending
    /// rule, never claims the old key is dead before the backup has
    /// accepted the change.
    pub fn line(self) -> &'static str {
        match self {
            SheetReason::VaultCreated => "This is the only way back into your vault if you forget your master password.",
            SheetReason::Replaced => "Replaces your previous key. Your backup accepts the old one until this Mac reaches it.",
            SheetReason::DeviceRemoved => "A removed device means this replaces your key. The old one works until this Mac reaches it.",
            SheetReason::Recovered => "Your vault was recovered under a new vault key. This is your new Recovery Key.",
            SheetReason::HandleRetried => "Your first recovery name was taken. Discard the earlier sheet; it opens nothing.",
        }
    }
}

pub enum PanelOutcome {
    Cancelled,
    /// Recovery Key window closed through "I've saved it".
    Acknowledged,
    /// MpCreate / MpEntry submission.
    Submitted(SecretVec),
    /// MpChange submission: (old, new).
    SubmittedChange(SecretVec, SecretVec),
}

/// Runs a secure panel to completion (blocks the calling executor
/// thread; production impl marshals to the AppKit main thread).
pub trait PanelRunner: Send + Sync {
    fn run(&self, req: PanelRequest, timeout: Duration) -> PanelOutcome;
    /// `run` with a non-secret code the panel shows above its fields (the
    /// enrollment SAS). A runner that cannot show one must refuse.
    fn run_with_code(&self, req: PanelRequest, code: &str, timeout: Duration) -> PanelOutcome {
        let _ = (req, code, timeout);
        PanelOutcome::Cancelled
    }
    /// Show (and offer to print) a Recovery Key. `Acknowledged` only when
    /// the user confirmed they saved it; anything else is `Cancelled`.
    fn show_recovery_key(&self, _sheet: &RecoverySheet, _timeout: Duration) -> PanelOutcome {
        PanelOutcome::Cancelled
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every reason that supersedes an existing Recovery Key must say so,
    /// and — the §1.7 remote-pending rule — none may claim the old key is
    /// dead while the backup can still accept it.
    #[test]
    fn superseding_reasons_say_so_without_claiming_a_cutoff() {
        for reason in [SheetReason::Replaced, SheetReason::DeviceRemoved] {
            let line = reason.line().to_lowercase();
            assert!(line.contains("replace"), "{reason:?}: {line}");
            assert!(line.contains("until this mac reaches it"), "{reason:?} must state the pending window: {line}");
        }
        for reason in [SheetReason::VaultCreated, SheetReason::Replaced, SheetReason::DeviceRemoved, SheetReason::Recovered, SheetReason::HandleRetried] {
            assert!(!reason.line().contains("no longer works"), "{reason:?} claims a cutoff");
        }
    }

    /// The copy is shown in a fixed-width window; keep every line short
    /// enough to render on one line (§1.7).
    #[test]
    fn reason_lines_fit_the_window() {
        for reason in [
            SheetReason::VaultCreated,
            SheetReason::Replaced,
            SheetReason::DeviceRemoved,
            SheetReason::Recovered,
            SheetReason::HandleRetried,
        ] {
            assert!(reason.line().len() <= 92, "too long to render: {}", reason.line());
        }
    }

    /// FR-02: the printed sheet carries the checkpoint facts plus the
    /// normalized handle and the provider origin.
    #[test]
    fn sheet_carries_handle_and_origin() {
        let rk = crate::crypto::secret::SecretBytes::new([0x42; 32]);
        let s = crate::vault::rk_ops::make_sheet(&rk, &[0xa0; 16], 7, &[0x66; 32], Some("synthetic@example.test"), SheetReason::VaultCreated);
        assert!(s.checkpoint.contains(&crate::crypto::hex::encode([0xa0u8; 16])) && s.checkpoint.contains("generation 7"));
        assert!(s.recovery.contains("synthetic@example.test"), "{}", s.recovery);
        assert!(s.recovery.contains(crate::storage::header::default_provider().unwrap()), "{}", s.recovery);
        assert!(s.recovery.len() <= 92, "fits the window: {}", s.recovery);
    }
}
