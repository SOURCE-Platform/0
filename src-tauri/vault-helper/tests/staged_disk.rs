//! §22.11 on-disk staging of a fully staged publication (SG-01…SG-03).
//! Stub panels; synthetic data only.

mod vault_fx;

use serde_json::json;
use vault_fx::*;
use vault_helper::sync::staged_disk;

fn staged(fx: &Fx) -> Option<(String, usize)> {
    let c = fx.core.lock().unwrap();
    c.provider.publish.as_ref().map(|p| (vault_helper::crypto::hex::encode(p.staging.body_sha256), p.staging.blobs.len()))
}

fn files(fx: &Fx) -> Vec<std::path::PathBuf> {
    std::fs::read_dir(staged_disk::dir(&fx.dir)).map(|d| d.filter_map(|e| e.ok().map(|e| e.path())).collect()).unwrap_or_default()
}

/// SG-01: the staged `create` survives a helper restart and is served
/// again while LOCKED, byte for byte.
#[test]
fn staged_publication_survives_a_restart() {
    let _g = serial();
    let mut fx = fx();
    let setup = setup_vault(&fx, MP);
    let before = staged(&fx).expect("setup stages the create");
    assert_eq!(setup["publication"]["body_sha256"], before.0.as_str());
    fx.reboot();
    assert_eq!(fx.state(), VaultState::Locked);
    assert_eq!(staged(&fx), Some(before.clone()), "resumed without the VK");
    assert_eq!(fx.core.lock().unwrap().reported_state(), VaultState::BackingUp);
    let resumed = fx.op(json!({"op": "backup_prepare"}));
    assert_eq!(resumed["body_sha256"], before.0.as_str(), "{resumed}");
    let list = fx.op(json!({"op": "backup_blob_list", "session": resumed["session"]}));
    assert_eq!(list["blobs"].as_array().unwrap().len(), before.1, "{list}");
    fx.remove_dir();
}

/// SG-02: tampered, partial or stale staging is discarded, never resumed.
#[test]
fn damaged_or_stale_staging_is_discarded() {
    let _g = serial();
    // Tampered blob.
    let mut fx = fx();
    setup_vault(&fx, MP);
    let victim = files(&fx).into_iter().find(|p| p.file_name().unwrap().len() == 64).unwrap();
    let mut bytes = std::fs::read(&victim).unwrap();
    bytes[0] ^= 1;
    std::fs::write(&victim, bytes).unwrap();
    fx.reboot();
    assert_eq!(staged(&fx), None);
    assert!(files(&fx).is_empty(), "directory removed");
    // Re-staged at the next unlock.
    assert_eq!(unlock(&fx, MP)["ok"], true);
    assert_eq!(fx.op(json!({"op": "backup_prepare"}))["kind"], "create");
    // A descriptor edited to list one blob fewer (review SEC-O1).
    let ready_path = staged_disk::dir(&fx.dir).join("ready.json");
    let mut ready: serde_json::Value = serde_json::from_slice(&std::fs::read(&ready_path).unwrap()).unwrap();
    ready["blobs"].as_array_mut().unwrap().pop();
    std::fs::write(&ready_path, serde_json::to_vec(&ready).unwrap()).unwrap();
    fx.reboot();
    assert_eq!(staged(&fx), None, "an edited blob list is not resumed");
    assert_eq!(unlock(&fx, MP)["ok"], true);
    assert_eq!(fx.op(json!({"op": "backup_prepare"}))["kind"], "create");
    // Partial: the descriptor is missing.
    std::fs::remove_file(staged_disk::dir(&fx.dir).join("ready.json")).unwrap();
    fx.reboot();
    assert_eq!(staged(&fx), None);
    // Stale: an authority change after the staging supersedes it.
    assert_eq!(unlock(&fx, MP)["ok"], true);
    assert_eq!(fx.op(json!({"op": "backup_prepare"}))["kind"], "create");
    fx.push_panel(PanelOutcome::SubmittedChange(SecretVec::new(MP.to_vec()), SecretVec::new(MP_NEW.to_vec())));
    let changed = fx.op(json!({"op": "change_master_password"}));
    assert_eq!(changed["ok"], true, "{changed}");
    assert!(!files(&fx).is_empty(), "files remain, but the record is gone");
    {
        let dir = fx.dir.clone();
        let store = vault_helper::storage::VaultStore::open(&dir).unwrap();
        let record: Option<serde_json::Value> = vault_helper::storage::kv::get(&store.conn, staged_disk::KEY).unwrap();
        assert!(record.is_none(), "the authority change deleted the record in its own commit");
    }
    fx.reboot();
    assert_eq!(staged(&fx), None, "stale staging is not resumed");
    assert!(files(&fx).is_empty());
    fx.remove_dir();
}

/// SG-03: the staging directory holds ciphertext and public data only.
#[test]
fn staging_holds_no_secrets() {
    let _g = serial();
    let fx = fx();
    setup_and_unlock(&fx);
    add_login(&fx);
    assert_eq!(fx.op(json!({"op": "backup_prepare"}))["kind"], "create");
    let rk = fx.panel.shown_rk.lock().unwrap().clone().unwrap();
    let first_words: Vec<&str> = rk.split_whitespace().take(4).collect();
    let vk = fx.core.lock().unwrap().vk.as_ref().map(|v| v.expose().to_vec()).unwrap();
    let hex_vk = vault_helper::crypto::hex::encode(&vk).into_bytes();
    let needles: Vec<Vec<u8>> = vec![MP.to_vec(), PASSWORD.as_bytes().to_vec(), first_words.join(" ").into_bytes(), b"fixture@example.test".to_vec(), vk, hex_vk];
    let all = files(&fx);
    assert!(all.len() > 3, "blobs, body and descriptor");
    for f in all {
        let bytes = std::fs::read(&f).unwrap();
        for n in &needles {
            assert!(!bytes.windows(n.len()).any(|w| w == n.as_slice()), "secret bytes in {}", f.display());
        }
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(std::fs::metadata(&f).unwrap().permissions().mode() & 0o777, 0o600);
    }
    fx.remove_dir();
}
