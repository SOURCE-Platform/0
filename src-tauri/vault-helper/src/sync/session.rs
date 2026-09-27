//! Helper sessions and ciphertext streams (spec v0.4 §1.3). Data larger
//! than one IPC frame — backup blobs, a `create` body, the enrollment
//! bundle — moves in chunks of at most 24 KiB. Only ciphertext and public
//! data ever travel this way.
//!
//! - Outbound (helper → main), pull-based: only blobs listed in the
//!   session are readable (`read`).
//! - Inbound (main → helper), acknowledged: a stream opens only for a hash
//!   on the session's need list, within the role cap and the session
//!   budget; writes are strictly contiguous; `end` checks the length and
//!   the SHA-256 and only then accepts the blob. Any violation →
//!   `TRANSFER_INVALID` and the partial data is dropped.
//! - Session and stream ids are 128-bit OsRng values. Caps: blob 1 MiB
//!   (registry 4 MiB, index 8 MiB), session 512 MiB, ≤ 4 open streams,
//!   idle stream 60 s, idle session 10 min. Staging lives in memory and is
//!   dropped on close, lock or expiry (never written to disk here).

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::time::{Duration, Instant};

use sha2::{Digest, Sha256};

use crate::errors::ErrorCode;

pub const CHUNK: usize = 24 * 1024;
pub const SESSION_BUDGET: u64 = 512 << 20;
pub const MAX_STREAMS: usize = 4;
pub const IDLE_STREAM: Duration = Duration::from_secs(60);
pub const IDLE_SESSION: Duration = Duration::from_secs(600);

pub type Id = [u8; 16];

pub fn new_id() -> Id {
    let mut b = [0u8; 16];
    getrandom::fill(&mut b).expect("OS RNG");
    b
}

struct Inbound {
    sha: [u8; 32],
    size: u64,
    data: Vec<u8>,
    hasher: Sha256,
    seq: u64,
    touched: Instant,
}

/// One session's transfer state. What the session is *for* (publish,
/// sync, recovery, enrollment) lives with its owner; this is only the
/// §1.3 transport discipline.
pub struct Transfer {
    pub id: Id,
    /// Outbound: blobs main may read.
    pub outbound: BTreeMap<[u8; 32], Vec<u8>>,
    /// Inbound: hashes main may send, with their size cap.
    pub need: BTreeMap<[u8; 32], u64>,
    /// Inbound blobs received and verified.
    pub received: HashMap<[u8; 32], Vec<u8>>,
    streams: HashMap<Id, Inbound>,
    received_bytes: u64,
    touched: Instant,
}

impl Transfer {
    pub fn new(outbound: BTreeMap<[u8; 32], Vec<u8>>) -> Transfer {
        Transfer {
            id: new_id(),
            outbound,
            need: BTreeMap::new(),
            received: HashMap::new(),
            streams: HashMap::new(),
            received_bytes: 0,
            touched: Instant::now(),
        }
    }

    pub fn expired(&self) -> bool {
        self.touched.elapsed() > IDLE_SESSION
    }

    fn touch(&mut self) {
        self.touched = Instant::now();
    }

    /// Outbound chunk: `(data, total_len, eof)`.
    pub fn read(&mut self, sha: &[u8; 32], offset: u64) -> Result<(Vec<u8>, u64, bool), ErrorCode> {
        self.touch();
        let blob = self.outbound.get(sha).ok_or(ErrorCode::TransferInvalid)?;
        let total = blob.len() as u64;
        if offset > total {
            return Err(ErrorCode::TransferInvalid);
        }
        let end = (offset as usize + CHUNK).min(blob.len());
        Ok((blob[offset as usize..end].to_vec(), total, end == blob.len()))
    }

    /// Add hashes main may send (with their caps).
    pub fn expect(&mut self, items: impl IntoIterator<Item = ([u8; 32], u64)>) {
        for (sha, cap) in items {
            if !self.received.contains_key(&sha) {
                self.need.insert(sha, cap);
            }
        }
    }

    pub fn still_needed(&self) -> BTreeSet<[u8; 32]> {
        self.need.keys().filter(|h| !self.received.contains_key(*h)).copied().collect()
    }

    pub fn begin(&mut self, sha: [u8; 32], size: u64) -> Result<Id, ErrorCode> {
        self.touch();
        self.streams.retain(|_, s| s.touched.elapsed() <= IDLE_STREAM);
        let cap = *self.need.get(&sha).ok_or(ErrorCode::TransferInvalid)?;
        let open: u64 = self.streams.values().map(|s| s.size).sum();
        if size > cap
            || self.streams.len() >= MAX_STREAMS
            || self.received.contains_key(&sha)
            || self.received_bytes + open + size > SESSION_BUDGET
        {
            return Err(ErrorCode::TransferInvalid);
        }
        let id = new_id();
        self.streams.insert(id, Inbound { sha, size, data: Vec::with_capacity(size as usize), hasher: Sha256::new(), seq: 0, touched: Instant::now() });
        Ok(id)
    }

    /// Strictly contiguous: `seq` increments by one, `offset` equals the
    /// bytes received so far.
    pub fn write(&mut self, stream: &Id, seq: u64, offset: u64, data: &[u8]) -> Result<(), ErrorCode> {
        self.touch();
        let s = self.streams.get_mut(stream).ok_or(ErrorCode::TransferInvalid)?;
        let ok = seq == s.seq && offset == s.data.len() as u64 && data.len() <= CHUNK && s.data.len() as u64 + data.len() as u64 <= s.size;
        if !ok {
            self.streams.remove(stream);
            return Err(ErrorCode::TransferInvalid);
        }
        s.hasher.update(data);
        s.data.extend_from_slice(data);
        s.seq += 1;
        s.touched = Instant::now();
        Ok(())
    }

    pub fn end(&mut self, stream: &Id) -> Result<[u8; 32], ErrorCode> {
        self.touch();
        let s = self.streams.remove(stream).ok_or(ErrorCode::TransferInvalid)?;
        let digest: [u8; 32] = s.hasher.finalize().into();
        if s.data.len() as u64 != s.size || digest != s.sha {
            return Err(ErrorCode::TransferInvalid);
        }
        self.received_bytes += s.size;
        self.received.insert(s.sha, s.data);
        Ok(s.sha)
    }

    pub fn cancel(&mut self, stream: &Id) {
        self.streams.remove(stream);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sha(b: &[u8]) -> [u8; 32] {
        Sha256::digest(b).into()
    }

    /// TR-01…TR-06 at the transport level.
    #[test]
    fn stream_rules() {
        let blob = vec![7u8; CHUNK + 10];
        let mut t = Transfer::new(BTreeMap::from([(sha(&blob), blob.clone())]));
        // Outbound: chunked, only listed blobs.
        let (c1, total, eof) = t.read(&sha(&blob), 0).unwrap();
        assert_eq!((c1.len(), total, eof), (CHUNK, blob.len() as u64, false));
        assert!(t.read(&sha(&blob), CHUNK as u64).unwrap().2);
        assert_eq!(t.read(&[0; 32], 0).err(), Some(ErrorCode::TransferInvalid), "TR-01 unlisted");
        // Inbound: off the need list refused.
        let data = b"synthetic inbound blob".to_vec();
        assert!(t.begin(sha(&data), data.len() as u64).is_err(), "TR-02 not needed");
        t.expect([(sha(&data), 1024)]);
        assert!(t.begin(sha(&data), 2048).is_err(), "TR-05 oversize");
        let s = t.begin(sha(&data), data.len() as u64).unwrap();
        assert!(t.write(&s, 1, 0, &data).is_err(), "TR-03 out of order");
        let s = t.begin(sha(&data), data.len() as u64).unwrap();
        t.write(&s, 0, 0, &data[..5]).unwrap();
        t.write(&s, 1, 5, &data[5..]).unwrap();
        assert_eq!(t.end(&s).unwrap(), sha(&data));
        assert!(t.still_needed().is_empty());
        // TR-04: SHA-256 mismatch.
        let other = b"another".to_vec();
        t.expect([(sha(&other), 1024)]);
        let s = t.begin(sha(&other), other.len() as u64).unwrap();
        t.write(&s, 0, 0, b"anothes").unwrap();
        assert_eq!(t.end(&s).err(), Some(ErrorCode::TransferInvalid));
        // TR-06: cancel drops partial data.
        let s = t.begin(sha(&other), other.len() as u64).unwrap();
        t.write(&s, 0, 0, b"ano").unwrap();
        t.cancel(&s);
        assert!(t.end(&s).is_err());
    }
}
