//! The Chrome native-messaging broker (spec §9.2, §9.3; Phase H), its own
//! crate so it links nothing of the vault (§17.2; owner decision
//! 2026-10-17, final review VER-I12): `broker` checks and translates the
//! extension's frames, `helper` is the one connection to the vault helper,
//! `codesign` verifies that helper's code identity before any byte is sent.

pub mod broker;
pub mod codesign;
pub mod helper;
