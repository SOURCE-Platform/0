//! The `get_state` answer (§1.5): one shape for the Mac helper and the
//! iPhone engine.

use serde_json::Value;

use super::VaultCore;

impl VaultCore {
    /// The `get_state` answer (§1.5), one shape for every shell: the
    /// reported state, whether the vault itself is open (BACKING_UP can
    /// overlay a LOCKED vault, §13.2), and §22.14's `behind`.
    pub fn state_answer(&self) -> Value {
        let mut v = serde_json::json!({ "ok": true, "error": null, "state": self.reported_state().as_str(), "vault_open": self.vk.is_some() });
        if self.behind {
            v["behind"] = serde_json::json!(true);
        }
        // §22.9: this phone was removed by its Mac (locked; reads only).
        if let Some(r) = crate::peer::client::removal::active(self.phone, &self.vault_dir) {
            v["removed"] = serde_json::json!({ "published": r.published });
        }
        // §11.3.2 corroborated (the phone's provider path keeps the count).
        let lost = self.phone && self.store.as_ref().is_some_and(|s| crate::storage::kv::get::<bool>(&s.conn, "phone_access_lost").ok().flatten() == Some(true));
        if lost {
            v["access_lost"] = serde_json::json!(true);
        }
        // §22.11: a fully staged publication waits (it may finish LOCKED).
        if self.provider.publish.is_some() {
            v["upload_waiting"] = serde_json::json!(true);
        }
        // §22.10: a Recovery Key sheet that was shown for a rotation that
        // never committed (not while that rotation is still running).
        let gen = self.header.as_ref().map(|h| h.vk_generation);
        if self.state != crate::state::VaultState::Authorizing && gen.is_some_and(|g| super::staged_sheet::void(&self.vault_dir, g)) {
            v["rk_sheet_void"] = serde_json::json!(true);
        }
        v
    }
}
