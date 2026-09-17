//! IPC transport (spec §1.4): Unix-domain socket, length-prefixed JSON
//! frames, SecCode peer authentication.

pub mod client;
pub mod framing;
pub mod peer_auth;
pub mod server;
