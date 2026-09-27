//! Main-process side of the vault backup (spec v0.4 §11.1, §11.3.2): a
//! background worker that runs the shared `vault-coordinator` flows over
//! the helper connection and the URLSession transport — sync, publish,
//! the staged `create` after setup — with the §11.3.2 retry schedule and
//! the `BACKUP_ACCESS_LOST` / clock-skew / stale signals, reported to the
//! UI on the `vault:backup` event. This process never sees plaintext or
//! any key; the helper signs every request.

pub mod transport;
pub mod worker;

use serde_json::{json, Value};
use vault_coordinator::Helper;

/// The helper, through the app's single `app`-class connection.
pub struct AppHelper;

impl Helper for AppHelper {
    fn op(&self, frame: Value) -> Result<Value, String> {
        match crate::core::vault_client::request(frame) {
            Ok(v) => Ok(v),
            Err(e) if e.starts_with("HELPER_UNAVAILABLE") => Err(e),
            // A §15 error frame: hand it back as the frame it was.
            Err(code) => Ok(json!({ "ok": false, "error": code })),
        }
    }
}

/// The provider origin this build signs for (the helper's compiled-in
/// allowlist, §11.4); `None` when the build has none (release builds
/// until the production origin is decided).
pub fn origin() -> Option<&'static str> {
    vault_helper::storage::header::default_provider().ok()
}
