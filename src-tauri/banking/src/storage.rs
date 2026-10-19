use crate::{secrets::SecretStore, types::Ledger, Error, Result};
use chacha20poly1305::{
    aead::{Aead, KeyInit, Payload},
    XChaCha20Poly1305, XNonce,
};
use std::{fs, io::Write, path::Path};
use zeroize::Zeroizing;

const HEADER: &[u8] = b"SOURCE-BANKING\x01";
const MAX_BYTES: u64 = 128 * 1024 * 1024;

pub fn key(store: &dyn SecretStore, directory: &Path, create: bool) -> Result<Zeroizing<Vec<u8>>> {
    if let Some(key) = store.get("ledger-key")? {
        if key.len() != 32 {
            return Err(Error::Integrity);
        }
        return Ok(key);
    }
    if directory.join("ledger.enc").exists() {
        return Err(Error::Integrity);
    }
    if !create {
        return Err(Error::NotConfigured);
    }
    let mut key = Zeroizing::new(vec![0; 32]);
    getrandom::fill(&mut key).map_err(|_| Error::Keychain)?;
    store.put("ledger-key", &key)?;
    Ok(key)
}

pub fn load(directory: &Path, key: &[u8]) -> Result<Ledger> {
    let path = directory.join("ledger.enc");
    let metadata = match fs::symlink_metadata(&path) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Ledger::default()),
        Err(_) => return Err(Error::Storage),
    };
    if !metadata.is_file() || metadata.len() > MAX_BYTES {
        return Err(Error::Storage);
    }
    let data = fs::read(path).map_err(|_| Error::Storage)?;
    if !data.starts_with(HEADER) || data.len() < HEADER.len() + 24 + 16 {
        return Err(Error::Integrity);
    }
    let nonce: [u8; 24] = data[HEADER.len()..HEADER.len() + 24].try_into().unwrap();
    let cipher = XChaCha20Poly1305::new_from_slice(key).map_err(|_| Error::Integrity)?;
    let bytes = Zeroizing::new(
        cipher
            .decrypt(
                &XNonce::from(nonce),
                Payload {
                    msg: &data[HEADER.len() + 24..],
                    aad: HEADER,
                },
            )
            .map_err(|_| Error::Integrity)?,
    );
    serde_json::from_slice(&bytes).map_err(|_| Error::Integrity)
}

pub fn save(directory: &Path, key: &[u8], ledger: &Ledger) -> Result<()> {
    fs::create_dir_all(directory).map_err(|_| Error::Storage)?;
    let meta = fs::symlink_metadata(directory).map_err(|_| Error::Storage)?;
    if !meta.is_dir() || meta.file_type().is_symlink() {
        return Err(Error::Storage);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(directory, fs::Permissions::from_mode(0o700))
            .map_err(|_| Error::Storage)?;
    }
    let plaintext = Zeroizing::new(serde_json::to_vec(ledger).map_err(|_| Error::Storage)?);
    if plaintext.len() as u64 > MAX_BYTES - 128 {
        return Err(Error::Storage);
    }
    let mut nonce = [0; 24];
    getrandom::fill(&mut nonce).map_err(|_| Error::Storage)?;
    let cipher = XChaCha20Poly1305::new_from_slice(key).map_err(|_| Error::Integrity)?;
    let ciphertext = cipher
        .encrypt(
            &XNonce::from(nonce),
            Payload {
                msg: &plaintext,
                aad: HEADER,
            },
        )
        .map_err(|_| Error::Storage)?;
    let path = directory.join(format!(".{}.tmp", uuid::Uuid::new_v4()));
    let result = (|| {
        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&path).map_err(|_| Error::Storage)?;
        file.write_all(HEADER)
            .and_then(|_| file.write_all(&nonce))
            .and_then(|_| file.write_all(&ciphertext))
            .and_then(|_| file.sync_all())
            .map_err(|_| Error::Storage)?;
        fs::rename(&path, directory.join("ledger.enc")).map_err(|_| Error::Storage)?;
        fs::File::open(directory)
            .and_then(|f| f.sync_all())
            .map_err(|_| Error::Storage)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(path);
    }
    Ok(result?)
}
