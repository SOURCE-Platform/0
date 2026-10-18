//! PS-09 (§22.7): laundering — D delivers through the Mac to another
//! device, then D is revoked. On adopting the revocation, every revision
//! *authored* by D that the provider state carrying the revocation does
//! not list is refused, whoever delivered it (the Mac here); a
//! provider-confirmed one survives. Synthetic data only.

mod vault_fx;

use std::collections::HashSet;
use vault_fx::*;

#[test]
fn ps09_what_d_delivered_through_the_mac_does_not_survive_ds_revocation() {
    let _g = serial();
    let fx = fx();
    setup_and_unlock(&fx);
    let laundered = add_login(&fx);
    let confirmed = add_login(&fx);
    let d = [0xd0; 16];
    let d_author = vault_proto::rev::uuid_string(&d);
    let c = fx.core.lock().unwrap();
    let store = c.store.as_ref().unwrap();
    // Both records' revisions now carry D as their author (they came from
    // D, via the Mac); only one is in the provider state's index.
    store.conn.execute("UPDATE record_revs SET author_device = ?1 WHERE record_id IN (?2, ?3)", rusqlite::params![d_author, laundered, confirmed]).unwrap();
    let rev_of = |r: &str| -> [u8; 32] {
        store.conn.query_row("SELECT revision_id FROM record_revs WHERE record_id = ?1", rusqlite::params![r], |row| row.get::<_, Vec<u8>>(0)).unwrap().try_into().unwrap()
    };
    let admit: HashSet<[u8; 32]> = [rev_of(&confirmed)].into_iter().collect();
    vault_helper::storage::revoked::record(&store.conn, &d, &admit, "me").unwrap();
    let alive = |r: &str| -> i64 {
        store.conn.query_row("SELECT count(*) FROM record_tips WHERE record_id = ?1", rusqlite::params![r], |row| row.get(0)).unwrap()
    };
    assert_eq!(alive(&laundered), 0, "D's unconfirmed revision is refused");
    assert_eq!(alive(&confirmed), 1, "the provider-confirmed one survives");
    drop(c);
    fx.remove_dir();
}
