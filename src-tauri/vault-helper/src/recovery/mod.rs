//! Phase D recovery layer (spec §11.7, §11.8, §12): recovery-class
//! credentials, the recovery-sheet checkpoint, and the total-loss
//! recovery engine. Trusted-device flows (MP set, RK rotation) live in
//! `vault::recovery_ops`.

pub mod creds;
pub mod sheet;
pub mod total_loss;
