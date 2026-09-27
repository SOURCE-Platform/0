//! BK-28 (spec v0.4 §1.5 `setup_retry_handle`): the first `create` meets
//! `HANDLE_TAKEN` — twice — and the retry issues a new RK and rotates the
//! VK in one journaled commit, re-staging the `create`. The new handle
//! resolves; the first attempt's RK opens only the retired VK, which opens
//! nothing in the committed state; local records survive. Synthetic only.

mod mfx;

use mfx::recover::{self, into_mac};
use mfx::*;
use vault_helper::crypto::secret::{random_secret, SecretBytes};
use vault_helper::crypto::wrap::{open_wrap_rk, RecoveryWrapFile};
use vault_helper::errors::ErrorCode;
use vault_helper::recovery::complete::Plan;
use vault_helper::recovery::total_loss::Credential;
use vault_helper::storage::store::RECOVERY_WRAP_NAME;
use vault_helper::storage::VaultStore;
use vault_helper::sync::pending;
use vault_helper::vault::{backup_ops, recovery_ops, retry_handle, setup};

const TAKEN_1: &str = "synthetic-taken-one@example.test";
const TAKEN_2: &str = "synthetic-taken-two@example.test";
const FREE: &str = "synthetic-free@example.test";

#[test]
fn bk28_handle_taken_then_retry() {
    let cloud = Cloud::new("bk28");
    let mut x1 = Mac::new("bk28-x1");
    x1.setup(&cloud, TAKEN_1).unwrap();
    let mut x2 = Mac::new("bk28-x2");
    x2.setup(&cloud, TAKEN_2).unwrap();

    // A sets up as the helper does (pending `vault_create` + kv handle).
    let mut a = Mac::new("bk28-a");
    let rk0 = random_secret();
    let (header, vk) = vault_helper::vault::create::create_vault(&a.dir, MP, &rk0, &a.dev).unwrap();
    let updates = vault_helper::sync::change::updates_for(&header, Some(MP), Some(&rk0)).unwrap();
    let first = setup::stage_create(&a.dir, &a.dev, &vk, TAKEN_1, updates).unwrap();
    a.store = Some(VaultStore::open(&a.dir).unwrap());
    a.vk = Some(vk);
    a.add("local-before-retry");
    assert_eq!(a.accept(&first.staging, &a.post(&cloud, &first.staging)), Err(ErrorCode::HandleTaken));
    let first_wrap = std::fs::read(a.dir.join(RECOVERY_WRAP_NAME)).unwrap();
    let vk0 = SecretBytes::new(*a.vk.as_ref().unwrap().expose());

    let mut rk = SecretBytes::new(*rk0.expose());
    for (handle, taken) in [(TAKEN_2, true), (FREE, false)] {
        let (store, vk) = (a.store.take().unwrap(), a.vk.take().unwrap());
        let pk_now = recovery_ops::prove_mp(&store, MP).unwrap();
        rk = random_secret();
        let new_vk = retry_handle::rotate(store, &vk, &pk_now, &rk, handle).unwrap();
        let store = VaultStore::open(&a.dir).unwrap();
        let p = pending::load(&store.conn).unwrap().unwrap();
        assert_eq!(p.ops, vec![pending::PendingOp::VaultCreate], "still the first create");
        a.store = Some(store);
        a.vk = Some(new_vk);
        let st = backup_ops::restage_create(a.store(), &a.registry(), a.vk.as_ref().unwrap(), &a.dev).unwrap();
        let r = a.post(&cloud, &st);
        let got = a.accept(&st, &r);
        if taken {
            assert_eq!(got, Err(ErrorCode::HandleTaken), "a second HANDLE_TAKEN repeats the retry");
        } else {
            got.unwrap();
        }
    }
    assert!(pending::load(&a.store().conn).unwrap().is_none(), "the create committed");
    assert_eq!(a.titles(), vec!["local-before-retry"], "local records survive");
    assert_eq!(a.store().header.vk_generation, 3, "one rotation per retry");

    // The first sheet opens only the retired VK, which opens nothing now.
    let f: RecoveryWrapFile = serde_json::from_slice(&first_wrap).unwrap();
    let old = open_wrap_rk(&f, &rk0, &a.vid()).unwrap();
    assert_eq!(old.vk.expose(), vk0.expose());
    let readable = a.store().list_records(&vk0).map_or(false, |v| v.iter().any(|i| i["title"] == "local-before-retry"));
    assert!(!readable, "the retired VK opens no record");
    let current: RecoveryWrapFile = serde_json::from_slice(&std::fs::read(a.dir.join(RECOVERY_WRAP_NAME)).unwrap()).unwrap();
    assert!(open_wrap_rk(&current, &rk0, &a.vid()).is_err());

    // The new handle resolves and recovers with the new RK.
    let out = recover::run(&cloud, FREE, Credential::Rk(&rk), Plan { new_mp: Some(b"synthetic-bk28-new-master-password"), keep_rk: None }, None).expect("recovered by the new handle");
    assert_eq!(into_mac(out).titles(), vec!["local-before-retry"]);
    // The first-sheet RK authenticates nothing at the provider.
    let r = recover::run(&cloud, FREE, Credential::Rk(&rk0), Plan { new_mp: Some(b"synthetic-bk28-new-master-password"), keep_rk: None }, None);
    assert_eq!(r.err(), Some(ErrorCode::WrongCredential));
}
