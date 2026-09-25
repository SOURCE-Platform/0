//! Synthetic revision-graph fixture for the §3.2 merge tests: a fresh
//! `vault.db` per test and a builder for revisions with synthetic
//! ciphertext bytes (the merge never decrypts). Synthetic data only.
#![allow(dead_code)]

use rusqlite::Connection;
use vault_helper::backup::object;
use vault_helper::storage::db;
use vault_helper::storage::revisions::{new_revision_id, RevisionRow};

pub const REC: &str = "5e7e0000-0000-4000-8000-000000000001";
pub const A: &str = "a0000000-0000-4000-8000-00000000000a";
pub const B: &str = "b0000000-0000-4000-8000-00000000000b";
pub const C: &str = "c0000000-0000-4000-8000-00000000000c";
pub const ZERO: &str = "00000000-0000-0000-0000-000000000000";

pub struct Graph {
    pub conn: Connection,
    pub dir: std::path::PathBuf,
}

impl Drop for Graph {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

#[derive(Clone, Debug)]
pub struct Rev {
    pub row: RevisionRow,
    pub obj: Vec<u8>,
}

impl Rev {
    pub fn set_times(&mut self, created: u64, updated: u64) {
        self.row.created_at = created;
        self.row.updated_at = updated;
        self.refresh();
    }

    /// Another representation of the same revision (a rotation re-seal).
    pub fn reseal(&mut self, vk_generation: u32) {
        self.row.vk_generation = vk_generation;
        self.row.ct = format!("resealed-{vk_generation}").into_bytes();
        self.row.nonce = [vk_generation as u8; 24];
        self.refresh();
    }

    pub fn refresh(&mut self) {
        if let Ok(o) = object::encode(&self.row) {
            self.obj = o;
        }
    }
}

impl Graph {
    pub fn new(tag: &str) -> Graph {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let dir = std::env::temp_dir().join(format!("vh-graph-{tag}-{}-{nanos}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let conn = db::open_db(&dir.join(db::DB_NAME), true).unwrap();
        Graph { conn, dir }
    }

    pub fn rev(&self, author: &str, counter: u64, parents: &[&Rev], deleted: bool) -> Rev {
        let mut parent_ids: Vec<[u8; 32]> = parents.iter().map(|p| p.row.revision_id).collect();
        parent_ids.sort();
        let row = RevisionRow {
            revision_id: new_revision_id(),
            record_id: REC.to_string(),
            parent_ids,
            author_device: author.to_string(),
            counter,
            deleted,
            kind_tag: 1,
            vk_generation: 0,
            schema_version: 1,
            nonce: [7; 24],
            ct: format!("synthetic-ct-{author}-{counter}").into_bytes(),
            meta_nonce: [8; 24],
            meta_ct: b"synthetic-meta".to_vec(),
            created_at: 1,
            updated_at: 1,
        };
        let obj = object::encode(&row).unwrap();
        Rev { row, obj }
    }
}

pub fn sorted(revs: &[&Rev]) -> Vec<[u8; 32]> {
    let mut v: Vec<[u8; 32]> = revs.iter().map(|r| r.row.revision_id).collect();
    v.sort();
    v
}
