//! `FsStores`: the three storage traits over a directory tree with the
//! §11.2 S3 key layout — the backend for tests and rehearsals, running
//! the production provider logic (§1.1). One process-wide lock makes the
//! conditional writes atomic across threads; the ETag is a random token
//! rewritten on every write (a concurrency token, not a content hash).

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::UNIX_EPOCH;

use vault_proto::crypto::hex;

use crate::stores::{BlobStore, Etag, OpsStore, StateStore, StoreError, StoreResult};

pub struct FsStores {
    root: PathBuf,
    lock: Mutex<()>,
}

impl FsStores {
    pub fn new(root: &Path) -> FsStores {
        FsStores { root: root.to_path_buf(), lock: Mutex::new(()) }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    fn path(&self, key: &str) -> PathBuf {
        self.root.join(key)
    }

    fn guard(&self) -> std::sync::MutexGuard<'_, ()> {
        self.lock.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn read(&self, key: &str) -> StoreResult<Option<(Vec<u8>, Etag)>> {
        let p = self.path(key);
        let bytes = match std::fs::read(&p) {
            Ok(b) => b,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(_) => return Err(StoreError),
        };
        let tag = std::fs::read_to_string(etag_path(&p)).map_err(|_| StoreError)?;
        Ok(Some((bytes, Etag(tag))))
    }

    fn write(&self, key: &str, bytes: &[u8]) -> StoreResult<Etag> {
        let p = self.path(key);
        std::fs::create_dir_all(p.parent().ok_or(StoreError)?).map_err(|_| StoreError)?;
        let tag = new_etag();
        atomic_write(&etag_path(&p), tag.as_bytes())?;
        atomic_write(&p, bytes)?;
        Ok(Etag(tag))
    }

    fn cas(&self, key: &str, bytes: &[u8], expect: Option<&Etag>) -> StoreResult<Option<Etag>> {
        let _g = self.guard();
        let current = self.read(key)?;
        match (current, expect) {
            (None, None) => self.write(key, bytes).map(Some),
            (Some((_, cur)), Some(want)) if &cur == want => self.write(key, bytes).map(Some),
            _ => Ok(None),
        }
    }

    fn remove(&self, key: &str) -> StoreResult<()> {
        let p = self.path(key);
        for f in [etag_path(&p), p] {
            match std::fs::remove_file(&f) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(_) => return Err(StoreError),
            }
        }
        Ok(())
    }
}

fn state_key(vid: &[u8; 16]) -> String {
    format!("v2/vaults/{}/state", hex::encode(vid))
}

fn blob_key(vid: &[u8; 16], sha: &[u8; 32]) -> String {
    format!("v2/vaults/{}/blobs/{}", hex::encode(vid), hex::encode(sha))
}

fn etag_path(p: &Path) -> PathBuf {
    let mut s = p.as_os_str().to_owned();
    s.push(".etag");
    PathBuf::from(s)
}

fn new_etag() -> String {
    let mut b = [0u8; 16];
    getrandom::fill(&mut b).expect("OS RNG");
    hex::encode(b)
}

fn atomic_write(path: &Path, bytes: &[u8]) -> StoreResult<()> {
    let tmp = path.with_extension(format!("tmp-{}", new_etag()));
    std::fs::write(&tmp, bytes).map_err(|_| StoreError)?;
    std::fs::rename(&tmp, path).map_err(|_| StoreError)
}

impl StateStore for FsStores {
    fn load(&self, vid: &[u8; 16]) -> StoreResult<Option<(Vec<u8>, Etag)>> {
        let _g = self.guard();
        self.read(&state_key(vid))
    }

    fn create(&self, vid: &[u8; 16], bytes: &[u8]) -> StoreResult<Option<Etag>> {
        self.cas(&state_key(vid), bytes, None)
    }

    fn replace(&self, vid: &[u8; 16], bytes: &[u8], etag: &Etag) -> StoreResult<Option<Etag>> {
        self.cas(&state_key(vid), bytes, Some(etag))
    }

    fn delete(&self, vid: &[u8; 16], etag: &Etag) -> StoreResult<bool> {
        let _g = self.guard();
        match self.read(&state_key(vid))? {
            Some((_, cur)) if &cur == etag => self.remove(&state_key(vid)).map(|_| true),
            _ => Ok(false),
        }
    }
}

impl BlobStore for FsStores {
    fn put_if_absent(&self, vid: &[u8; 16], sha: &[u8; 32], bytes: &[u8]) -> StoreResult<bool> {
        Ok(self.cas(&blob_key(vid, sha), bytes, None)?.is_some())
    }

    fn get(&self, vid: &[u8; 16], sha: &[u8; 32]) -> StoreResult<Option<Vec<u8>>> {
        let _g = self.guard();
        Ok(self.read(&blob_key(vid, sha))?.map(|(b, _)| b))
    }

    fn size(&self, vid: &[u8; 16], sha: &[u8; 32]) -> StoreResult<Option<u64>> {
        match std::fs::metadata(self.path(&blob_key(vid, sha))) {
            Ok(m) => Ok(Some(m.len())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(_) => Err(StoreError),
        }
    }

    fn list(&self, vid: &[u8; 16]) -> StoreResult<Vec<([u8; 32], u64)>> {
        let dir = self.path(&format!("v2/vaults/{}/blobs", hex::encode(vid)));
        let rd = match std::fs::read_dir(&dir) {
            Ok(rd) => rd,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(_) => return Err(StoreError),
        };
        let mut out = Vec::new();
        for e in rd {
            let e = e.map_err(|_| StoreError)?;
            let name = e.file_name().to_string_lossy().to_string();
            if let Some(sha) = hex::decode_array::<32>(&name) {
                let t = e.metadata().and_then(|m| m.modified()).map_err(|_| StoreError)?;
                out.push((sha, t.duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)));
            }
        }
        Ok(out)
    }

    fn delete(&self, vid: &[u8; 16], sha: &[u8; 32]) -> StoreResult<()> {
        let _g = self.guard();
        self.remove(&blob_key(vid, sha))
    }
}

impl OpsStore for FsStores {
    fn get(&self, key: &str) -> StoreResult<Option<(Vec<u8>, Etag)>> {
        let _g = self.guard();
        self.read(key)
    }

    fn create(&self, key: &str, bytes: &[u8]) -> StoreResult<Option<Etag>> {
        self.cas(key, bytes, None)
    }

    fn replace(&self, key: &str, bytes: &[u8], etag: &Etag) -> StoreResult<Option<Etag>> {
        self.cas(key, bytes, Some(etag))
    }

    fn delete(&self, key: &str) -> StoreResult<()> {
        let _g = self.guard();
        self.remove(key)
    }

    fn list_prefix(&self, prefix: &str) -> StoreResult<Vec<String>> {
        let _g = self.guard();
        let (dir, _) = prefix.rsplit_once('/').unwrap_or(("", prefix));
        let rd = match std::fs::read_dir(self.path(dir)) {
            Ok(rd) => rd,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(_) => return Err(StoreError),
        };
        let mut out = Vec::new();
        for e in rd {
            let name = e.map_err(|_| StoreError)?.file_name().to_string_lossy().to_string();
            let key = if dir.is_empty() { name } else { format!("{dir}/{name}") };
            if key.starts_with(prefix) && !key.ends_with(".etag") && !key.contains(".tmp-") {
                out.push(key);
            }
        }
        Ok(out)
    }
}
