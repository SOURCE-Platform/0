//! Recovery (spec v0.4 §11.5, §11.7, §11.8, §12): the recovery-sheet
//! checkpoint and the total-loss engine over the provider protocol.
//! Trusted-device flows (MP set, RK rotation) live in `vault::recovery_ops`.

pub mod complete;
pub mod locate;
pub mod sheet;
pub mod total_loss;
