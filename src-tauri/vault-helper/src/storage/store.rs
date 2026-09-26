//! `VaultStore`: the open vault database plus header/manifest state.
//! Creation and the §3.6 open sequence live here; record CRUD lives in
//! `store_records.rs` (same `impl`).

use std::path::{Path, PathBuf};

use rusqlite::Connection;

use super::db::{self, DB_NAME};
use super::header::{self, Header};
use super::manifest::{self, Manifest};
use crate::errors::ErrorCode;
use crate::{VAULT_HEADER_NAME, VAULT_REGISTRY_NAME};

pub use super::manifest::MANIFEST_NAME;

pub const WRAPS_DIR: &str = "wraps";
pub const IMPORT_DIR: &str = "import";
pub const PASSWORD_WRAP_NAME: &str = "wraps/password.wrap";
pub const RECOVERY_WRAP_NAME: &str = "wraps/recovery.wrap";

/// Field tag for the single per-revision list-metadata blob sealed into
/// `meta_ct` (§2.6 `meta_aad` field_tag). Phase C seals one JSON blob
/// per revision rather than one blob per field; the tag namespace leaves
/// room for per-field blobs later.
pub const META_FIELD_TAG: &[u8] = b"list";

pub struct VaultStore {
    pub conn: Connection,
    pub header: Header,
    pub manifest: Manifest,
    pub dir: PathBuf,
}

/// Write `bytes` to `path` atomically (tmp + fsync + rename), 0600.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), ErrorCode> {
    use std::os::unix::fs::PermissionsExt;
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, bytes).map_err(|_| ErrorCode::Internal)?;
    let f = std::fs::File::open(&tmp).map_err(|_| ErrorCode::Internal)?;
    f.sync_all().map_err(|_| ErrorCode::Internal)?;
    drop(f);
    std::fs::rename(&tmp, path).map_err(|_| ErrorCode::Internal)?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
        .map_err(|_| ErrorCode::Internal)?;
    Ok(())
}

impl VaultStore {
    /// Create a brand-new vault directory's storage (§5.4 Phase C subset):
    /// header.json, vault.db (schema v2), manifest.json, registry.json
    /// (empty log — no enrolled device yet), wraps/ and import/ dirs.
    /// Caller writes the wrap files afterwards so a failed wrap leaves no
    /// half-usable vault: this function runs before any wrap exists and
    /// the caller removes the directory on any later failure.
    pub fn create(dir: &Path, header: Header) -> Result<VaultStore, ErrorCode> {
        let conn = db::open_db(&dir.join(DB_NAME), true)?;
        let mut manifest = Manifest::fresh(header.vault_id);
        // The genesis registry entry exists before the first manifest, so
        // the two heads agree from the start (§3.5 consistency check).
        manifest.registry_head = header.registry_head.clone();
        write_atomic(&dir.join(VAULT_HEADER_NAME), &header::write_header(&header)?)?;
        write_atomic(&dir.join(MANIFEST_NAME), &manifest::write_manifest(&manifest)?)?;
        write_atomic(&dir.join(VAULT_REGISTRY_NAME), b"[]")?;
        for sub in [WRAPS_DIR, IMPORT_DIR] {
            let sub = dir.join(sub);
            std::fs::create_dir_all(&sub).map_err(|_| ErrorCode::Internal)?;
            std::fs::set_permissions(&sub, std::os::unix::fs::PermissionsExt::from_mode(0o700))
                .map_err(|_| ErrorCode::Internal)?;
        }
        Ok(VaultStore {
            conn,
            header,
            manifest,
            dir: dir.to_path_buf(),
        })
    }

    /// §3.6 open sequence: parse header → integrity_check → load manifest
    /// → §3.5 manifest/header consistency → manifest objects == DB revs.
    /// Pure ciphertext operations; VK is not involved here.
    pub fn open(dir: &Path) -> Result<VaultStore, ErrorCode> {
        // Finish or discard an interrupted VK rotation first (§2.10):
        // an opened vault is never half-rotated.
        super::rotation_journal::recover_pending(dir)?;
        let header_bytes =
            std::fs::read(dir.join(VAULT_HEADER_NAME)).map_err(|_| ErrorCode::DbCorrupt)?;
        let header = header::parse_header(&header_bytes)?;
        let conn = db::open_db(&dir.join(DB_NAME), false)?;
        db::integrity_check(&conn)?;
        let manifest_bytes =
            std::fs::read(dir.join(MANIFEST_NAME)).map_err(|_| ErrorCode::ManifestMismatch)?;
        let manifest = manifest::parse_manifest(&manifest_bytes)?;
        let mut store = VaultStore {
            conn,
            header,
            manifest,
            dir: dir.to_path_buf(),
        };
        let consistent = manifest::check_against_header(&store.manifest, &store.header).and_then(|_| store.verify_objects());
        if consistent.is_err() && store.can_roll_forward() {
            // A crash between the DB commit and the manifest flip: the DB
            // records exactly this one pending step (SEC-I7).
            store.persist_head()?; // manifest generation → the pending step
            manifest::check_against_header(&store.manifest, &store.header)?;
            store.verify_objects()?;
        } else {
            consistent?;
        }
        Ok(store)
    }

/// Roll forward only the single pending flip: same vault, VK
    /// generation and registry head, the DB marker exactly one step past
    /// the manifest, and the header at either end of that step.
    fn can_roll_forward(&self) -> bool {
        let (m, h) = (&self.manifest, &self.header);
        let same = m.vault_id == h.vault_id && m.vk_generation == h.vk_generation && m.registry_head == h.registry_head;
        let step = m.manifest_generation + 1;
        same && pending_flip(&self.conn) == Some(step) && (h.manifest_generation == m.manifest_generation || h.manifest_generation == step)
    }

    /// Parse just the header (boot path; no DB touch).
    pub fn read_header(dir: &Path) -> Result<Header, ErrorCode> {
        super::rotation_journal::recover_pending(dir)?;
        let bytes = std::fs::read(dir.join(VAULT_HEADER_NAME)).map_err(|_| ErrorCode::DbCorrupt)?;
        header::parse_header(&bytes)
    }

    /// §3.5: the manifest's object list must equal the set of admitted
    /// revision ids in `record_revs`, and every revision must be sealed
    /// under the header's `vk_generation`; anything else is a
    /// swapped/tampered directory → MANIFEST_MISMATCH.
    pub fn verify_objects(&self) -> Result<(), ErrorCode> {
        let stale: i64 = self
            .conn
            .query_row(
                "SELECT count(*) FROM record_revs WHERE vk_generation != ?1",
                [i64::from(self.header.vk_generation)],
                |r| r.get(0),
            )
            .map_err(|_| ErrorCode::DbCorrupt)?;
        if stale != 0 {
            return Err(ErrorCode::ManifestMismatch);
        }
        let mut stmt = self
            .conn
            .prepare("SELECT record_id, revision_id FROM record_revs")
            .map_err(|_| ErrorCode::DbCorrupt)?;
        let rows = stmt
            .query_map([], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, Vec<u8>>(1)?))
            })
            .map_err(|_| ErrorCode::DbCorrupt)?;
        let mut db_set: Vec<(String, String)> = Vec::new();
        for row in rows {
            let (rid, hash) = row.map_err(|_| ErrorCode::DbCorrupt)?;
            db_set.push((rid, crate::crypto::hex::encode(hash)));
        }
        let mut manifest_set: Vec<(String, String)> = self
            .manifest
            .objects
            .iter()
            .map(|o| (o.record_id.clone(), o.revision_id.clone()))
            .collect();
        db_set.sort();
        manifest_set.sort();
        if db_set != manifest_set {
            return Err(ErrorCode::ManifestMismatch);
        }
        Ok(())
    }

    /// Flip the manifest generation after a committed mutation and persist
    /// header + manifest atomically (generation kept in lockstep so §3.5
    /// catches any file swapped in from a different point in time).
    pub fn persist_head(&mut self) -> Result<(), ErrorCode> {
        self.manifest.manifest_generation += 1;
        self.header.manifest_generation = self.manifest.manifest_generation;
        let mut stmt = self
            .conn
            .prepare("SELECT record_id, revision_id FROM record_revs")
            .map_err(|_| ErrorCode::DbCorrupt)?;
        let objects = stmt
            .query_map([], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, Vec<u8>>(1)?))
            })
            .map_err(|_| ErrorCode::DbCorrupt)?
            .filter_map(|r| r.ok())
            .map(|(record_id, id)| manifest::ManifestObject {
                record_id,
                revision_id: crate::crypto::hex::encode(id),
            })
            .collect();
        self.manifest.objects = objects;
        self.manifest.item_count = self.live_item_count()?;
        write_atomic(
            &self.dir.join(VAULT_HEADER_NAME),
            &header::write_header(&self.header)?,
        )?;
        write_atomic(
            &self.dir.join(MANIFEST_NAME),
            &manifest::write_manifest(&self.manifest)?,
        )?;
        Ok(())
    }

    /// Move both heads to a new registry head (an appended enroll/revoke
    /// entry, §4). Header and manifest stay in lockstep, so a directory
    /// carrying a registry from a different point in time is caught by
    /// §3.5 rather than silently accepted.
    pub fn set_registry_head(&mut self, head: [u8; 32]) -> Result<(), ErrorCode> {
        self.header.registry_head = crate::storage::header::Hex32(head);
        self.manifest.registry_head = crate::storage::header::Hex32(head);
        self.persist_head()
    }

    /// The registry `device_id` this vault authors revisions as (§3.2),
    /// recorded in `kv` at creation or recovery.
    pub fn author_device(&self) -> Result<String, ErrorCode> {
        let v: Vec<u8> = self
            .conn
            .query_row("SELECT value FROM kv WHERE key='author_device'", [], |r| r.get(0))
            .map_err(|_| ErrorCode::DeviceNotAuthorized)?;
        String::from_utf8(v).map_err(|_| ErrorCode::DbCorrupt)
    }

    pub fn set_author_device(&self, device_id: &[u8; 16]) -> Result<(), ErrorCode> {
        let id = super::revisions::uuid_string(device_id);
        self.conn
            .execute(
                "INSERT INTO kv (key, value) VALUES ('author_device', ?1)
                 ON CONFLICT(key) DO UPDATE SET value=excluded.value",
                [id.into_bytes()],
            )
            .map(|_| ())
            .map_err(|_| ErrorCode::DbCorrupt)
    }

    fn live_item_count(&self) -> Result<u64, ErrorCode> {
        super::revision_rows::live_count(&self.conn)
    }
}

/// SEC-I7 roll-forward marker, written inside the transaction that
/// changes the revision set: "the manifest flip to `next_gen` is due".
pub fn stamp_flip(tx: &rusqlite::Connection, next_gen: u64) -> Result<(), ErrorCode> {
    tx.execute(
        "INSERT INTO kv (key, value) VALUES ('pending_flip', ?1)
         ON CONFLICT(key) DO UPDATE SET value=excluded.value",
        [next_gen.to_be_bytes().to_vec()],
    )
    .map(|_| ())
    .map_err(|_| ErrorCode::DbCorrupt)
}

fn pending_flip(conn: &rusqlite::Connection) -> Option<u64> {
    let v: Vec<u8> = conn.query_row("SELECT value FROM kv WHERE key='pending_flip'", [], |r| r.get(0)).ok()?;
    Some(u64::from_be_bytes(v.try_into().ok()?))
}

pub fn now_epoch() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}
