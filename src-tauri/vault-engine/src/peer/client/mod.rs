//! The phone's peer exchange (spec v0.5 §22.8; wire annex revision 3;
//! plan `phase-f2c-phone-plan.md`): `peer_status` → `peer_hello` →
//! `peer_heads` for the buckets whose digests differ → `peer_revs_get` for
//! the records whose heads differ, admitted under §22.7 with the Mac as
//! source. One request outstanding at a time; Swift only carries bytes.
//!
//! Not in this milestone (stated): `peer_state` (a provisional state moves
//! nothing the phone can act on before it has its own provider path,
//! F.2d) and `peer_revs_put` (without a verified provider exchange the
//! phone may push only provider-confirmed revisions, which came from the
//! Mac or its bundle).

pub mod envelope;
pub mod removal;
pub mod status;

use std::collections::BTreeMap;

use serde_json::{json, Value};
use vault_proto::peer::body::{heads_digest, Hello, BUCKETS};
use vault_proto::peer::exchange::{encode_heads_req, encode_revs_get, HeadsItem, HeadsResp, Revs};
use vault_proto::peer::{PeerOp, PeerStatus};

use crate::crypto::secret::SecretBytes;
use crate::errors::ErrorCode;
use crate::registry::chain::RegistryState;
use crate::registry::device::DeviceIdentity;
use crate::storage::revision_rows::all_rows;
use crate::storage::revisions::{uuid_bytes, RevisionRow};
use crate::storage::VaultStore;
use envelope::Outstanding;
use status::Standing;

/// Annex A.3.4: at most 512 records per `peer_revs_get`.
const MAX_WANTS: usize = 512;
/// A bound on one exchange's round trips (every step makes progress or
/// ends; this only stops a misbehaving responder from looping us).
const MAX_STEPS: u32 = 256;

#[derive(Debug, Default, Clone, Copy)]
pub struct Summary {
    pub admitted: u64,
    pub waiting: u64,
    pub refused: u64,
    /// Records the Mac could not serve here (left to the provider path).
    pub unavailable: u64,
    pub mac_behind: bool,
    pub limited: bool,
}

impl Summary {
    pub fn json(&self) -> Value {
        json!({ "admitted": self.admitted, "waiting": self.waiting, "refused": self.refused, "unavailable": self.unavailable, "mac_behind": self.mac_behind, "limited": self.limited })
    }
}

/// What the phone holds now: heads per record, over its admitted graph
/// minus set-aside revisions.
pub fn my_heads(store: &VaultStore) -> Result<BTreeMap<[u8; 16], Vec<[u8; 32]>>, ErrorCode> {
    let mut by: BTreeMap<[u8; 16], Vec<RevisionRow>> = BTreeMap::new();
    for row in all_rows(&store.conn)? {
        if crate::storage::set_aside::refused(&store.conn, &row.revision_id)? {
            continue;
        }
        by.entry(uuid_bytes(&row.record_id).ok_or(ErrorCode::DbCorrupt)?).or_default().push(row);
    }
    Ok(by.into_iter().map(|(id, rows)| (id, crate::peer::graph::heads(&rows))).collect())
}

pub enum Step {
    /// Send this carriage entry next.
    Request(Vec<u8>),
    Done(Summary),
    /// A verified `peer_status` removes this phone (§22.9).
    Removed { published: bool },
}

enum Phase {
    Status,
    Hello,
    Heads { asked: Vec<u8> },
    Revs { asked: Vec<([u8; 16], Vec<[u8; 32]>)> },
}

/// What one step needs from the unlocked engine.
pub struct Env<'a> {
    pub store: &'a mut VaultStore,
    pub vk: &'a SecretBytes<32>,
    pub me: &'a dyn DeviceIdentity,
    pub committed: &'a RegistryState,
    pub manifest_hash: Option<[u8; 32]>,
    pub now: u64,
}

pub struct Exchange {
    vault_id: [u8; 16],
    mac: [u8; 16],
    phase: Phase,
    out: Outstanding,
    wants: Vec<([u8; 16], Vec<[u8; 32]>)>,
    steps: u32,
    summary: Summary,
}

impl Exchange {
    /// Start with `peer_status`; returns the exchange and its first request.
    pub fn begin(env: &Env<'_>, vault_id: [u8; 16], mac: [u8; 16]) -> Result<(Exchange, Vec<u8>), ErrorCode> {
        let (bytes, out) = envelope::request(env.me, vault_id, mac, PeerOp::Status, &vault_proto::peer::body::empty(), env.now)?;
        Ok((Exchange { vault_id, mac, phase: Phase::Status, out, wants: Vec::new(), steps: 0, summary: Summary::default() }, bytes))
    }

    fn ask(&mut self, env: &Env<'_>, op: PeerOp, body: Vec<u8>, phase: Phase) -> Result<Step, ErrorCode> {
        let (bytes, out) = envelope::request(env.me, self.vault_id, self.mac, op, &body, env.now)?;
        self.out = out;
        self.phase = phase;
        Ok(Step::Request(bytes))
    }

    /// The Mac's answer to the outstanding request; the next step.
    pub fn step(&mut self, env: &mut Env<'_>, response: &[u8]) -> Result<Step, ErrorCode> {
        self.steps += 1;
        if self.steps > MAX_STEPS {
            return Err(ErrorCode::PeerLimit);
        }
        let (status, body) = envelope::response(env.committed, self.vault_id, env.me.device_id(), &self.out, response)?;
        match status {
            PeerStatus::Ok => {}
            PeerStatus::BadState => {
                self.summary.mac_behind = true; // §22.14: hello and status only
                return Ok(Step::Done(self.summary));
            }
            PeerStatus::Limit => {
                self.summary.limited = true;
                return Ok(Step::Done(self.summary));
            }
            // Status 3 is never "up to date"; 4 means our request was malformed.
            PeerStatus::NothingNewer | PeerStatus::FormatInvalid => return Err(ErrorCode::PeerAuthInvalid),
        }
        match std::mem::replace(&mut self.phase, Phase::Status) {
            Phase::Status => match status::check(env.committed, &self.vault_id, &env.me.device_id(), env.manifest_hash, env.vk, &body)? {
                Standing::Removed { published } => Ok(Step::Removed { published }),
                Standing::Active => self.ask(env, PeerOp::Hello, vault_proto::peer::body::empty(), Phase::Hello),
            },
            Phase::Hello => {
                let theirs = Hello::decode(&body).map_err(|_| ErrorCode::PeerAuthInvalid)?;
                let mine = my_heads(env.store)?;
                let digest = heads_digest(&mine.into_iter().collect::<Vec<_>>());
                let differ: Vec<u8> = (0..BUCKETS).filter(|b| digest[b * 32..b * 32 + 32] != theirs.heads_digest[b * 32..b * 32 + 32]).map(|b| b as u8).collect();
                if differ.is_empty() {
                    return Ok(Step::Done(self.summary));
                }
                self.ask(env, PeerOp::Heads, encode_heads_req(&differ), Phase::Heads { asked: differ })
            }
            Phase::Heads { asked } => self.on_heads(env, &asked, &body),
            Phase::Revs { asked } => self.on_revs(env, asked, &body),
        }
    }

    /// Whole buckets came back: want every record whose heads differ from
    /// ours; re-ask the buckets a cap left out.
    fn on_heads(&mut self, env: &mut Env<'_>, asked: &[u8], body: &[u8]) -> Result<Step, ErrorCode> {
        let resp = HeadsResp::decode(body).map_err(|_| ErrorCode::PeerAuthInvalid)?;
        if resp.buckets.iter().any(|b| !asked.contains(b)) {
            return Err(ErrorCode::PeerAuthInvalid);
        }
        let mine = my_heads(env.store)?;
        for item in &resp.items {
            match item {
                HeadsItem::Heads { record_id, heads } => {
                    let have = mine.get(record_id).cloned().unwrap_or_default();
                    if &have != heads {
                        self.wants.push((*record_id, have));
                    }
                }
                HeadsItem::Unavailable { .. } => self.summary.unavailable += 1,
            }
        }
        let rest: Vec<u8> = asked.iter().copied().filter(|b| !resp.buckets.contains(b)).collect();
        if !rest.is_empty() && !resp.buckets.is_empty() {
            return self.ask(env, PeerOp::Heads, encode_heads_req(&rest), Phase::Heads { asked: rest });
        }
        if !rest.is_empty() {
            self.summary.limited = true; // not even one bucket fits: leave it to the provider
        }
        self.next_revs(env)
    }

    fn next_revs(&mut self, env: &Env<'_>) -> Result<Step, ErrorCode> {
        if self.wants.is_empty() {
            return Ok(Step::Done(self.summary));
        }
        self.wants.sort_by_key(|w| w.0);
        let chunk: Vec<_> = self.wants.drain(..self.wants.len().min(MAX_WANTS)).collect();
        self.ask(env, PeerOp::RevsGet, encode_revs_get(&chunk), Phase::Revs { asked: chunk })
    }

    /// Admit the batch (§22.7, the Mac as source); re-ask what a cap left
    /// out; then the next chunk.
    fn on_revs(&mut self, env: &mut Env<'_>, asked: Vec<([u8; 16], Vec<[u8; 32]>)>, body: &[u8]) -> Result<Step, ErrorCode> {
        let batch = Revs::decode(body, false).map_err(|_| ErrorCode::PeerAuthInvalid)?;
        let rows = crate::peer::admit::decode_batch(&batch).map_err(|_| ErrorCode::PeerAuthInvalid)?;
        let mut served: Vec<[u8; 16]> = rows.iter().filter_map(|r| uuid_bytes(&r.record_id)).collect();
        served.extend(batch.unavailable.iter().map(|(id, _)| *id));
        if served.iter().any(|id| !asked.iter().any(|(a, _)| a == id)) {
            return Err(ErrorCode::PeerAuthInvalid); // only requested records appear (A.3.4)
        }
        self.summary.unavailable += batch.unavailable.len() as u64;
        if !rows.is_empty() {
            let c = crate::peer::admit::admit_rows(env.store, env.vk, self.mac, rows)?;
            self.summary.admitted += c.admitted;
            self.summary.waiting += c.waiting;
            self.summary.refused += c.refused;
        }
        let rest: Vec<_> = asked.into_iter().filter(|(id, _)| !served.contains(id)).collect();
        if batch.complete == Some(false) && !rest.is_empty() {
            if served.is_empty() {
                self.summary.limited = true; // no progress: the provider path
            } else {
                self.wants.extend(rest);
            }
        }
        self.next_revs(env)
    }
}
