use crate::{secrets::SecretStore, storage, tests::ledger, Error, Result};
use std::{collections::HashMap, sync::Mutex};
use zeroize::Zeroizing;

#[derive(Default)]
struct MemorySecrets(Mutex<HashMap<String, Vec<u8>>>);

impl SecretStore for MemorySecrets {
    fn get(&self, name: &str) -> Result<Option<Zeroizing<Vec<u8>>>> {
        Ok(self
            .0
            .lock()
            .unwrap()
            .get(name)
            .cloned()
            .map(Zeroizing::new))
    }
    fn put(&self, name: &str, value: &[u8]) -> Result<()> {
        self.0.lock().unwrap().insert(name.into(), value.to_vec());
        Ok(())
    }
}

#[test]
fn a_missing_key_never_generates_a_replacement_for_existing_records() {
    let dir = tempfile::tempdir().unwrap();
    let secrets = MemorySecrets::default();
    storage::save(dir.path(), &[1; 32], &ledger()).unwrap();
    assert!(matches!(
        storage::key(&secrets, dir.path(), true),
        Err(Error::Integrity)
    ));
    assert!(secrets.0.lock().unwrap().is_empty());
}

#[test]
fn new_keys_are_random_and_writes_use_fresh_nonces() {
    let dir = tempfile::tempdir().unwrap();
    let secrets = MemorySecrets::default();
    let key = storage::key(&secrets, dir.path(), true).unwrap();
    assert_eq!(key.len(), 32);
    assert!(key.iter().any(|b| *b != 0));
    storage::save(dir.path(), &key, &ledger()).unwrap();
    let first = std::fs::read(dir.path().join("ledger.enc")).unwrap();
    storage::save(dir.path(), &key, &ledger()).unwrap();
    assert_ne!(first, std::fs::read(dir.path().join("ledger.enc")).unwrap());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(dir.path()).unwrap().permissions().mode() & 0o777,
            0o700
        );
        assert_eq!(
            std::fs::metadata(dir.path().join("ledger.enc"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }
}

#[cfg(unix)]
#[test]
fn symlinked_ledger_is_not_read_or_followed() {
    use std::os::unix::fs::symlink;
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("unrelated");
    std::fs::write(&target, b"untouched").unwrap();
    symlink(&target, dir.path().join("ledger.enc")).unwrap();
    assert!(matches!(
        storage::load(dir.path(), &[1; 32]),
        Err(Error::Storage)
    ));
    assert_eq!(std::fs::read(target).unwrap(), b"untouched");
}
