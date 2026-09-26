//! The three storage traits every provider backend implements (spec v0.4
//! §1.1). Keys follow the §11.2 layout; `FsStores` and the S3 adapter use
//! the same paths. ETags are opaque concurrency tokens, never content
//! hashes a client could rely on.
//!
//! Every mutation is conditional: create-if-absent, replace-if-match or
//! delete-if-match. A failed condition is `Ok(None)` / `Ok(false)`, never
//! an error; `StoreError` means the backend itself is unavailable (503).

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Etag(pub String);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StoreError;

pub type StoreResult<T> = Result<T, StoreError>;

/// `v2/vaults/{vid}/state` — the only mutable vault object.
pub trait StateStore: Send + Sync {
    fn load(&self, vid: &[u8; 16]) -> StoreResult<Option<(Vec<u8>, Etag)>>;
    /// `If-None-Match: *`; `None` if a state already exists.
    fn create(&self, vid: &[u8; 16], bytes: &[u8]) -> StoreResult<Option<Etag>>;
    /// `If-Match`; `None` on an ETag mismatch.
    fn replace(&self, vid: &[u8; 16], bytes: &[u8], etag: &Etag) -> StoreResult<Option<Etag>>;
    /// §11.3.1 C4 rollback only; `false` on an ETag mismatch.
    fn delete(&self, vid: &[u8; 16], etag: &Etag) -> StoreResult<bool>;
}

/// `v2/vaults/{vid}/blobs/{sha256}` — immutable, content-addressed.
pub trait BlobStore: Send + Sync {
    /// `If-None-Match: *`. `true` if written, `false` if the name exists
    /// (by construction the existing content is identical).
    fn put_if_absent(&self, vid: &[u8; 16], sha: &[u8; 32], bytes: &[u8]) -> StoreResult<bool>;
    fn get(&self, vid: &[u8; 16], sha: &[u8; 32]) -> StoreResult<Option<Vec<u8>>>;
    /// Size of an existing blob.
    fn size(&self, vid: &[u8; 16], sha: &[u8; 32]) -> StoreResult<Option<u64>>;
    /// Every blob with its creation time (Unix seconds), for GC.
    fn list(&self, vid: &[u8; 16]) -> StoreResult<Vec<([u8; 32], u64)>>;
    /// Provider-controlled GC only; devices have no delete API.
    fn delete(&self, vid: &[u8; 16], sha: &[u8; 32]) -> StoreResult<()>;
}

/// Provider operational state: handle claims, nonces, rate-limit slots.
pub trait OpsStore: Send + Sync {
    fn get(&self, key: &str) -> StoreResult<Option<(Vec<u8>, Etag)>>;
    fn create(&self, key: &str, bytes: &[u8]) -> StoreResult<Option<Etag>>;
    fn replace(&self, key: &str, bytes: &[u8], etag: &Etag) -> StoreResult<Option<Etag>>;
    fn delete(&self, key: &str) -> StoreResult<()>;
    /// Keys under `prefix` (full keys, any order).
    fn list_prefix(&self, prefix: &str) -> StoreResult<Vec<String>>;
}
