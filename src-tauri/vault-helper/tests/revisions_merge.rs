//! §3.2 v0.4 merge rules at the storage layer (SY-01…SY-06, SY-08, SY-09,
//! SY-12): heads model, exact counter algorithm, tombstone conservatism,
//! equivocation freeze, resolution, duplicates, pending revisions and
//! permutation convergence. Ciphertexts are synthetic byte strings — the
//! merge never decrypts. Synthetic data only.

mod graph_fx;

use graph_fx::*;
use vault_helper::storage::merge::{apply_revision, retry_pending, MergeOutcome, NoCompare};
use vault_helper::storage::rev_state::{is_frozen, refused_totals};
use vault_helper::storage::revisions::{get_row, heads, REFUSED_COUNTER_REGRESSION, REFUSED_MALFORMED, REFUSED_ZERO_AUTHOR};

/// The fixture vault's local `vk_generation`.
const GEN: u32 = 0;

fn apply(g: &Graph, r: &Rev) -> MergeOutcome {
    apply_revision(&g.conn, &r.row, GEN, &NoCompare).unwrap()
}

/// SY-01: single-author linear edits fast-forward; no conflict rows.
#[test]
fn sy01_linear_fast_forward() {
    let g = Graph::new("sy01");
    let a = g.rev(A, 1, &[], false);
    let b = g.rev(A, 2, &[&a], false);
    let c = g.rev(A, 3, &[&b], false);
    for r in [&a, &b, &c] {
        assert_eq!(apply(&g, r), MergeOutcome::Applied { conflicted: false });
    }
    assert_eq!(heads(&g.conn, REC).unwrap(), vec![c.row.revision_id]);
}

/// SY-02: concurrent edits by two devices → both heads, no timestamp pick
/// (the later-timestamped revision does not win).
#[test]
fn sy02_concurrent_edits_conflict() {
    let g = Graph::new("sy02");
    let base = g.rev(A, 1, &[], false);
    let mut x = g.rev(A, 2, &[&base], false);
    let mut y = g.rev(B, 1, &[&base], false);
    x.set_times(10, 9_999_999); // x "newer" by wall clock
    y.set_times(10, 1);
    apply(&g, &base);
    apply(&g, &x);
    assert_eq!(apply(&g, &y), MergeOutcome::Applied { conflicted: true });
    assert_eq!(heads(&g.conn, REC).unwrap(), sorted(&[&x, &y]));
}

/// SY-03: an edit concurrent with a delete is a conflict, never a silent
/// delete-wins.
#[test]
fn sy03_edit_vs_delete_conflict() {
    let g = Graph::new("sy03");
    let base = g.rev(A, 1, &[], false);
    let del = g.rev(A, 2, &[&base], true);
    let edit = g.rev(B, 1, &[&base], false);
    for r in [&base, &del, &edit] {
        apply(&g, r);
    }
    assert_eq!(heads(&g.conn, REC).unwrap(), sorted(&[&del, &edit]));
}

/// SY-04: an edit claiming ancestry after a tombstone never resurrects:
/// the tombstone stays a head beside it.
#[test]
fn sy04_edit_after_tombstone_is_conflict() {
    let g = Graph::new("sy04");
    let base = g.rev(A, 1, &[], false);
    let del = g.rev(A, 2, &[&base], true);
    let zombie = g.rev(B, 1, &[&del], false);
    apply(&g, &base);
    apply(&g, &del);
    assert_eq!(apply(&g, &zombie), MergeOutcome::Applied { conflicted: true });
    assert_eq!(heads(&g.conn, REC).unwrap(), sorted(&[&del, &zombie]));
}

/// SY-05: the same author equivocates (two children, same counter) →
/// both kept, record frozen.
#[test]
fn sy05_equivocation_freezes() {
    let g = Graph::new("sy05");
    let base = g.rev(A, 1, &[], false);
    let e1 = g.rev(A, 2, &[&base], false);
    let e2 = g.rev(A, 2, &[&base], false);
    apply(&g, &base);
    apply(&g, &e1);
    assert!(!is_frozen(&g.conn, REC).unwrap());
    assert_eq!(apply(&g, &e2), MergeOutcome::Applied { conflicted: true });
    assert!(is_frozen(&g.conn, REC).unwrap());
}

/// SY-06: a revision whose own ancestry holds a same-author revision with
/// a counter ≥ its own is rejected and counted; the record is unchanged.
#[test]
fn sy06_counter_regression_rejected() {
    let g = Graph::new("sy06");
    let a2 = g.rev(A, 2, &[], false);
    let back = g.rev(A, 1, &[&a2], false);
    apply(&g, &a2);
    assert_eq!(apply(&g, &back), MergeOutcome::Rejected(REFUSED_COUNTER_REGRESSION));
    assert_eq!(heads(&g.conn, REC).unwrap(), vec![a2.row.revision_id]);
    assert!(refused_totals(&g.conn).unwrap().contains(&(REFUSED_COUNTER_REGRESSION, 1)));
}

/// A later author fork (counters differ, neither is an ancestor) freezes
/// just like equivocation.
#[test]
fn author_fork_freezes() {
    let g = Graph::new("fork");
    let base = g.rev(A, 1, &[], false);
    let f1 = g.rev(A, 2, &[&base], false);
    let f2 = g.rev(A, 3, &[&base], false);
    for r in [&base, &f1, &f2] {
        apply(&g, r);
    }
    assert!(is_frozen(&g.conn, REC).unwrap());
}

/// SY-08 (storage level): a resolution whose parents are all heads
/// collapses them — including keeping an edit over a competing tombstone.
#[test]
fn sy08_resolution_collapses_heads() {
    let g = Graph::new("sy08");
    let base = g.rev(A, 1, &[], false);
    let del = g.rev(A, 2, &[&base], true);
    let edit = g.rev(B, 1, &[&base], false);
    for r in [&base, &del, &edit] {
        apply(&g, r);
    }
    let fix = g.rev(A, 3, &[&del, &edit], false);
    assert_eq!(apply(&g, &fix), MergeOutcome::Applied { conflicted: false });
    assert_eq!(heads(&g.conn, REC).unwrap(), vec![fix.row.revision_id]);
}

/// SY-09: every ordering of the same revisions — including children
/// before parents — converges to identical heads and freeze state.
#[test]
fn sy09_permutations_converge() {
    let build = |tag: &str| {
        let g = Graph::new(tag);
        let base = g.rev(A, 1, &[], false);
        let x = g.rev(A, 2, &[&base], false);
        let y = g.rev(B, 1, &[&base], false);
        let z = g.rev(B, 2, &[&y], false);
        let w = g.rev(A, 3, &[&x, &z], false);
        let del = g.rev(C, 1, &[&y], true);
        (g, vec![base, x, y, z, w, del])
    };
    let orders: [&[usize]; 5] = [&[0, 1, 2, 3, 4, 5], &[5, 4, 3, 2, 1, 0], &[4, 0, 3, 5, 1, 2], &[2, 5, 0, 4, 3, 1], &[3, 1, 4, 2, 5, 0]];
    let mut results = Vec::new();
    for (i, order) in orders.iter().enumerate() {
        let (g, mut revs) = build(&format!("sy09-{i}"));
        // Same logical revisions every time: reuse the first build's ids.
        if let Some((_, ref first)) = results.first() {
            let first: &Vec<Rev> = first;
            for (r, f) in revs.iter_mut().zip(first) {
                *r = f.clone();
            }
        }
        for &k in order.iter() {
            apply(&g, &revs[k]);
        }
        retry_pending(&g.conn, GEN, &NoCompare).unwrap();
        let state = (heads(&g.conn, REC).unwrap(), is_frozen(&g.conn, REC).unwrap());
        results.push((state, revs));
    }
    let first = &results[0].0;
    assert_eq!(first.0.len(), 2, "w and the tombstone branch remain heads");
    for (state, _) in &results {
        assert_eq!(state, first);
    }
}

/// SY-12: duplicate ids — identical bytes are a no-op; a different graph
/// freezes; a copy at a generation other than the local one is never
/// stored (SEC-B1); a same-graph copy that brings a row up to the local
/// generation replaces it (VER-O3 checks the stored row).
#[test]
fn sy12_duplicates() {
    let g = Graph::new("sy12");
    let base = g.rev(A, 1, &[], false);
    apply(&g, &base);
    assert_eq!(apply(&g, &base), MergeOutcome::AlreadyKnown);
    let stored = |g: &Graph| get_row(&g.conn, &base.row.revision_id).unwrap().unwrap();
    let mut high = base.clone();
    high.reseal(99);
    assert_eq!(apply(&g, &high), MergeOutcome::Superseded);
    assert_eq!(stored(&g).vk_generation, 0, "a higher-generation copy is never stored");
    assert!(!is_frozen(&g.conn, REC).unwrap());
    let mut up = base.clone();
    up.reseal(1);
    assert_eq!(apply_revision(&g.conn, &up.row, 1, &NoCompare).unwrap(), MergeOutcome::Superseded);
    assert_eq!(stored(&g).vk_generation, 1, "brought up to the local generation");
    let mut forged = base.clone();
    forged.row.counter = 7;
    forged.refresh();
    assert_eq!(apply(&g, &forged), MergeOutcome::Frozen);
    assert!(is_frozen(&g.conn, REC).unwrap());
}

/// Zero author (v0.4) and non-canonical parents are rejected and counted.
#[test]
fn zero_author_and_malformed_parents_rejected() {
    let g = Graph::new("zero");
    let z = g.rev(ZERO, 1, &[], false);
    assert_eq!(apply(&g, &z), MergeOutcome::Rejected(REFUSED_ZERO_AUTHOR));
    let p1 = g.rev(A, 1, &[], false);
    let p2 = g.rev(B, 1, &[], false);
    apply(&g, &p1);
    apply(&g, &p2);
    let mut bad = g.rev(A, 2, &[&p1, &p2], false);
    bad.row.parent_ids.reverse(); // unsorted
    assert_eq!(apply(&g, &bad), MergeOutcome::Rejected(REFUSED_MALFORMED));
}
