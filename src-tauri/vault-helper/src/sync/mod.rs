//! Backup, sync and publication over the provider protocol (spec v0.4
//! §11): staging the local vault as a state transition, verifying and
//! merging provider states, and signing provider requests. The helper
//! never touches the network; the main process moves the bytes.

pub mod apply;
pub mod change;
pub mod compare;
pub mod fetch;
pub mod join;
pub mod local;
pub mod pending;
pub mod publish;
pub mod remote;
pub mod seen;
pub mod sign;
