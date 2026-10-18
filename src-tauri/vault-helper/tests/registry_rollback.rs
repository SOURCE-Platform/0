//! RG-06 (final review): an older registry file put back next to a vault —
//! say from before a revocation — is refused, not believed: the header's
//! recorded head must be in the file. Synthetic data only.

mod device_fx;
mod join_fx;
mod sync_fx;
mod vault_fx;

use serde_json::json;
use sync_fx::*;
use vault_fx::*;

#[test]
fn rg06_an_older_registry_file_is_refused() {
    let _g = serial();
    let (mac, phone, _item) = paired();
    let entries = vault_helper::registry::log::read_entries(&mac.dir).unwrap();
    assert!(entries.len() >= 2, "genesis and the phone's enrollment");
    mac.core.lock().unwrap().lock(vault_helper::vault::LockReason::Explicit);
    // Put back the chain as it was before the phone was enrolled.
    let older = vault_helper::registry::file::encode(&entries[..entries.len() - 1]).unwrap();
    let path = mac.dir.join(vault_helper::VAULT_REGISTRY_NAME);
    let current = std::fs::read(&path).unwrap();
    std::fs::write(&path, older).unwrap();
    let r = mac.op(json!({"op": "unlock"}));
    assert_eq!(r["error"], "REGISTRY_TRUNCATED", "{r}");
    // The real file back: the vault opens again.
    std::fs::write(&path, current).unwrap();
    assert_eq!(mac.op(json!({"op": "unlock"}))["ok"], true);
    mac.remove_dir();
    phone.remove_dir();
}
