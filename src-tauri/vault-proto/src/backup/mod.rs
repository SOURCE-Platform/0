//! Backup formats (spec §3.7, §4.8, §11.2): record objects, the object
//! index, the signed manifest and the registry checkpoint.

pub mod checkpoint;
pub mod index;
pub mod manifest;
pub mod object;
pub mod stage;
