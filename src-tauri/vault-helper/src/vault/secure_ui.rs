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
}

impl SheetReason {
    /// One line under the title. Every case that supersedes an existing
    /// Recovery Key says so in as many words.
    pub fn line(self) -> &'static str {
        match self {
            SheetReason::VaultCreated => "This is the only way back into your vault if you forget your master password.",
            SheetReason::Replaced => "This replaces your previous Recovery Key, which no longer works.",
            SheetReason::DeviceRemoved => "Removing a device replaced your vault key, so your previous Recovery Key no longer works.",
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
    /// Show (and offer to print) a Recovery Key. `Acknowledged` only when
    /// the user confirmed they saved it; anything else is `Cancelled`.
    fn show_recovery_key(&self, _sheet: &RecoverySheet, _timeout: Duration) -> PanelOutcome {
        PanelOutcome::Cancelled
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every reason that supersedes an existing Recovery Key must say so
    /// in the window. Someone who is handed new words without being told
    /// the old ones are dead will keep the old paper.
    #[test]
    fn superseding_reasons_say_the_old_key_stopped_working() {
        for reason in [SheetReason::Replaced, SheetReason::DeviceRemoved] {
            let line = reason.line().to_lowercase();
            assert!(
                line.contains("no longer works"),
                "{reason:?} does not tell the user their previous key is dead: {line}"
            );
        }
        // The first key supersedes nothing, so it must not claim otherwise.
        assert!(!SheetReason::VaultCreated.line().contains("no longer works"));
    }

    /// The copy is shown in a fixed-width window; keep every line short
    /// enough to render on one line (§1.7).
    #[test]
    fn reason_lines_fit_the_window() {
        for reason in [
            SheetReason::VaultCreated,
            SheetReason::Replaced,
            SheetReason::DeviceRemoved,
        ] {
            assert!(reason.line().len() <= 92, "too long to render: {}", reason.line());
        }
    }
}
