//! §3.2 determinism and the reviewed merge edge cases (VER-B1/I1/I6/M1/M2,
//! SEC-B1/I1/I2/I4): heads depend only on the admitted graph, whatever the
//! delivery order, including tombstones under resolutions and deep zombie
//! edits. Synthetic ciphertexts only.

mod graph_fx;

use graph_fx::*;
use vault_helper::storage::merge::{apply_batch, apply_revision, retry_pending, ContentCompare, MergeOutcome, NoCompare};
use vault_helper::storage::rev_state::{is_frozen, pending_count};
use vault_helper::storage::revisions::{get_row, heads, RevisionRow, REFUSED_GENERATION, REFUSED_MALFORMED};

fn apply(g: &Graph, r: &Rev) -> MergeOutcome {
    apply_revision(&g.conn, &r.row, 0, &NoCompare).unwrap()
}

/// Deterministic shuffles (xorshift) — no external RNG in the test graph.
fn permutations(n: usize, count: usize) -> Vec<Vec<usize>> {
    let mut x: u64 = 0x9e37_79b9_7f4a_7c15;
    (0..count)
        .map(|_| {
            let mut v: Vec<usize> = (0..n).collect();
            for i in (1..n).rev() {
                x ^= x << 13;
                x ^= x >> 7;
                x ^= x << 17;
                v.swap(i, (x % (i as u64 + 1)) as usize);
            }
            v
        })
        .collect()
}

fn converge(graph: impl Fn(&Graph) -> Vec<Rev>, tag: &str) -> (Vec<[u8; 32]>, bool) {
    let proto = Graph::new(&format!("{tag}-proto"));
    let revs = graph(&proto);
    let mut first = None;
    for (i, order) in permutations(revs.len(), 120).into_iter().enumerate() {
        let g = Graph::new(&format!("{tag}-{i}"));
        for k in order {
            apply(&g, &revs[k]);
        }
        retry_pending(&g.conn, 0, &NoCompare).unwrap();
        assert_eq!(pending_count(&g.conn).unwrap(), 0);
        let state = (heads(&g.conn, REC).unwrap(), is_frozen(&g.conn, REC).unwrap());
        match &first {
            None => first = Some(state),
            Some(f) => assert_eq!(&state, f, "order {i} diverged"),
        }
    }
    first.unwrap()
}

/// VER-B1 / SEC-I1: a resolution covering a tombstone and one edit, with
/// another concurrent edit arriving before or after it.
#[test]
fn tombstone_under_resolution_is_order_independent() {
    let (hs, frozen) = converge(
        |g| {
            let e0 = g.rev(A, 1, &[], false);
            let t = g.rev(B, 1, &[&e0], true);
            let e1 = g.rev(C, 1, &[&e0], false);
            let e3 = g.rev(A, 2, &[&e0], false);
            let r = g.rev(B, 2, &[&t, &e1], false);
            vec![e0, t, e1, e3, r]
        },
        "tomb-res",
    );
    assert_eq!(hs.len(), 2, "the concurrent edit and the resolution");
    assert!(!frozen);
}

/// A larger graph: zombies at depth, a partial resolution, a later delete.
#[test]
fn mixed_graph_is_order_independent() {
    converge(
        |g| {
            let e0 = g.rev(A, 1, &[], false);
            let t = g.rev(B, 1, &[&e0], true);
            let z = g.rev(C, 1, &[&t], false);
            let z2 = g.rev(C, 2, &[&z], false);
            let e1 = g.rev(A, 2, &[&e0], false);
            let r = g.rev(B, 2, &[&t, &e1], false);
            let e4 = g.rev(A, 3, &[&r], false);
            vec![e0, t, z, z2, e1, r, e4]
        },
        "mixed",
    );
}

/// VER-I1: an edit two levels below a tombstone never resurrects.
#[test]
fn deep_zombie_keeps_the_tombstone() {
    let g = Graph::new("deep");
    let base = g.rev(A, 1, &[], false);
    let t = g.rev(A, 2, &[&base], true);
    let z = g.rev(B, 1, &[&t], false);
    let z2 = g.rev(B, 2, &[&z], false);
    for r in [&base, &t, &z] {
        apply(&g, r);
    }
    assert_eq!(apply(&g, &z2), MergeOutcome::Applied { conflicted: true });
    assert_eq!(heads(&g.conn, REC).unwrap(), sorted(&[&t, &z2]));
}

/// SEC-B1: a new revision sealed under another generation is refused.
#[test]
fn other_generation_refused() {
    let g = Graph::new("gen");
    let mut r = g.rev(A, 1, &[], false);
    r.reseal(3);
    assert_eq!(apply(&g, &r), MergeOutcome::Rejected(REFUSED_GENERATION));
    assert!(get_row(&g.conn, &r.row.revision_id).unwrap().is_none());
}

/// SEC-I2: counters beyond i64::MAX are malformed.
#[test]
fn huge_counter_refused() {
    let g = Graph::new("ctr");
    let r = g.rev(A, 1u64 << 63, &[], false);
    assert_eq!(apply(&g, &r), MergeOutcome::Rejected(REFUSED_MALFORMED));
}

/// SEC-I4: a second, different copy of an id in the same batch meets the
/// duplicate rule instead of vanishing.
#[test]
fn same_batch_duplicate_freezes() {
    let g = Graph::new("batchdup");
    let a = g.rev(A, 1, &[], false);
    let mut b = a.clone();
    b.row.counter = 5;
    let out = apply_batch(&g.conn, &[a.row.clone(), b.row.clone()], 0, &NoCompare).unwrap();
    assert_eq!(out, vec![MergeOutcome::Applied { conflicted: false }, MergeOutcome::Frozen]);
    assert!(is_frozen(&g.conn, REC).unwrap());
}

/// VER-M1: two different copies of a held-back id freeze the record.
#[test]
fn pending_duplicate_freezes() {
    let g = Graph::new("penddup");
    let parent = g.rev(A, 1, &[], false);
    let child = g.rev(A, 2, &[&parent], false);
    let mut other = child.clone();
    other.row.ct = b"different-content".to_vec();
    assert_eq!(apply(&g, &child), MergeOutcome::Pending);
    assert_eq!(apply(&g, &other), MergeOutcome::Frozen);
    assert!(is_frozen(&g.conn, REC).unwrap());
}

/// VER-M2: a forged copy claiming another record freezes the record the
/// admitted revision belongs to.
#[test]
fn cross_record_forgery_freezes_the_real_record() {
    let g = Graph::new("xrec");
    let r = g.rev(A, 1, &[], false);
    apply(&g, &r);
    let mut forged = r.clone();
    forged.row.record_id = "f0000000-0000-4000-8000-00000000000f".into();
    assert_eq!(apply(&g, &forged), MergeOutcome::Frozen);
    assert!(is_frozen(&g.conn, REC).unwrap());
}

struct Same(bool);
impl ContentCompare for Same {
    fn same_content(&self, _: &RevisionRow, _: &RevisionRow) -> Result<bool, vault_helper::errors::ErrorCode> {
        Ok(self.0)
    }
}

/// VER-I6: same generation, different bytes — benign when the plaintext
/// is identical (lower blob hash kept), equivocation otherwise.
#[test]
fn same_generation_duplicates() {
    let blob = |r: &RevisionRow| vault_helper::backup::object::blob_hash(&vault_helper::backup::object::encode(r).unwrap());
    let g = Graph::new("benign");
    let a = g.rev(A, 1, &[], false);
    let mut b = a.clone();
    b.row.ct = b"re-sealed-identical".to_vec();
    b.row.nonce = [9; 24];
    apply(&g, &a);
    assert_eq!(apply_revision(&g.conn, &b.row, 0, &Same(true)).unwrap(), MergeOutcome::AlreadyKnown);
    let kept = get_row(&g.conn, &a.row.revision_id).unwrap().unwrap();
    assert_eq!(blob(&kept), blob(&a.row).min(blob(&b.row)));
    assert!(!is_frozen(&g.conn, REC).unwrap());
    let g2 = Graph::new("equiv");
    apply(&g2, &a);
    assert_eq!(apply_revision(&g2.conn, &b.row, 0, &Same(false)).unwrap(), MergeOutcome::Frozen);
}
