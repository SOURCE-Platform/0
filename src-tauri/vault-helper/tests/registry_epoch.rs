//! Registry chain + recovery_epoch verification (spec §4.4–4.6; RG-03,
//! RG-04, RG-05, RG-08, RG-09, RG-11, RG-13…RG-17). Synthetic devices
//! (software P-256 rehearsal identities), synthetic VKs.

use std::collections::HashMap;

use vault_helper::crypto::registry::{verify_recovery_proof, RegistryEntry};
use vault_helper::crypto::secret::{random_secret, SecretBytes};
use vault_helper::errors::ErrorCode;
use vault_helper::registry::build;
use vault_helper::registry::chain::{check_extends, verify_chain, EpochContext, RegistryState};
use vault_helper::registry::device::{DeviceIdentity, SoftwareDevice, PLATFORM_IOS, PLATFORM_MACOS};

const VAULT: [u8; 16] = [0x5A; 16];
const OTHER_VAULT: [u8; 16] = [0x5B; 16];
const M_OLD: [u8; 32] = [0x11; 32];
const M_NEW: [u8; 32] = [0x22; 32];

struct Ctx {
    vks: HashMap<[u8; 32], [u8; 32]>,
    superseded: Vec<[u8; 32]>,
}

impl EpochContext for Ctx {
    fn vk_for_manifest(&self, h: &[u8; 32]) -> Option<SecretBytes<32>> {
        self.vks.get(h).map(|k| SecretBytes::new(*k))
    }
    fn manifest_acceptable(&self, h: &[u8; 32]) -> bool {
        !self.superseded.contains(h)
    }
}

struct Fx {
    mac: SoftwareDevice,
    phone: SoftwareDevice,
    base: Vec<RegistryEntry>,
    st: RegistryState,
    vk: SecretBytes<32>,
    ctx: Ctx,
}

fn fx() -> Fx {
    let mac = SoftwareDevice::generate("Synthetic Mac", PLATFORM_MACOS);
    let phone = SoftwareDevice::generate("Synthetic iPhone", PLATFORM_IOS);
    let vk = random_secret();
    let ctx = Ctx { vks: HashMap::from([(M_OLD, *vk.expose())]), superseded: Vec::new() };
    let g = build::genesis(&mac).unwrap();
    let st0 = verify_chain(&[g.clone()], &VAULT, &ctx).unwrap();
    let en = build::enroll(&st0, &mac, &phone).unwrap();
    let base = vec![g, en];
    let st = verify_chain(&base, &VAULT, &ctx).unwrap();
    Fx { mac, phone, base, st, vk, ctx }
}

fn with(base: &[RegistryEntry], e: RegistryEntry) -> Vec<RegistryEntry> {
    let mut v = base.to_vec();
    v.push(e);
    v
}

fn epoch_for(f: &Fx, dev: &SoftwareDevice) -> RegistryEntry {
    build::recovery_epoch(&f.st, VAULT, M_OLD, &f.vk, dev).unwrap()
}

#[test]
fn rg17_valid_epoch_installs_exactly_the_bound_device() {
    let f = fx();
    let newdev = SoftwareDevice::generate("Replacement Mac", PLATFORM_MACOS);
    let ep = epoch_for(&f, &newdev);
    let chain = with(&f.base, ep);
    let st = verify_chain(&chain, &VAULT, &f.ctx).expect("valid epoch");
    assert_eq!(st.epoch, 1);
    let d = st.active_device(&newdev.device_id()).expect("installed");
    assert_eq!((d.sign_pub, d.agree_pub, d.platform), (newdev.sign_pub(), newdev.agree_pub(), PLATFORM_MACOS));
    // The next entry verifies under exactly the bound sign_pub…
    let third = SoftwareDevice::generate("Next iPhone", PLATFORM_IOS);
    let next = build::enroll(&st, &newdev, &third).unwrap();
    verify_chain(&with(&chain, next), &VAULT, &f.ctx).expect("signed by installed device");
    // …and not under a look-alike key reusing the same device_id.
    let imposter = SoftwareDevice::generate("Replacement Mac", PLATFORM_MACOS);
    let mut forged = build::enroll(&st, &imposter, &third).unwrap();
    forged.authorizer = Some(newdev.device_id());
    assert_eq!(verify_chain(&with(&chain, forged), &VAULT, &f.ctx).unwrap_err(), ErrorCode::SignatureInvalid);
}

/// RG-08 / RG-13…15: every bound field. Each alteration is rejected by
/// the chain AND the proof alone fails (so the rejection is the proof,
/// not only the hash link).
#[test]
fn rg08_rg13_15_any_bound_field_altered_is_rejected() {
    let f = fx();
    let newdev = SoftwareDevice::generate("Replacement Mac", PLATFORM_MACOS);
    let other = SoftwareDevice::generate("Attacker", PLATFORM_MACOS);
    let good = epoch_for(&f, &newdev);
    let alterations: Vec<(&str, Box<dyn Fn(&mut RegistryEntry)>)> = vec![
        ("vault_id", Box::new(|e| e.vault_id = Some(OTHER_VAULT))),
        ("prev_hash", Box::new(|e| e.prev_hash[0] ^= 1)),
        ("manifest_hash", Box::new(|e| e.manifest_hash = Some(M_NEW))),
        ("prior_epoch", Box::new(|e| e.prior_epoch = Some(7))),
        ("epoch", Box::new(|e| e.epoch = 9)),
        ("device_id", Box::new(|e| e.device_id[0] ^= 1)),
        ("sign_pub (RG-13)", Box::new(move |e| e.sign_pub = Some(other.sign_pub()))),
        ("agree_pub (RG-14)", Box::new(|e| e.agree_pub = Some(SoftwareDevice::generate("x", 1).agree_pub()))),
        ("platform (RG-15)", Box::new(|e| e.platform = Some(PLATFORM_IOS))),
        ("device_name (RG-15)", Box::new(|e| e.device_name = Some("Renamed".into()))),
        ("recovery_nonce", Box::new(|e| e.recovery_nonce = Some([0xEE; 16]))),
        ("enrolled_at", Box::new(|e| e.enrolled_at = Some(1))),
    ];
    for (name, alter) in alterations {
        let mut bad = good.clone();
        alter(&mut bad);
        let mh = bad.manifest_hash.unwrap();
        assert!(verify_recovery_proof(&f.vk, &mh, &bad).is_err(), "{name}: proof still verifies");
        assert!(verify_chain(&with(&f.base, bad), &VAULT, &f.ctx).is_err(), "{name}: chain accepted");
    }
    // Wrong VK: a proof made with any other key never verifies.
    let wrong = build::recovery_epoch(&f.st, VAULT, M_OLD, &random_secret(), &newdev).unwrap();
    assert_eq!(verify_chain(&with(&f.base, wrong), &VAULT, &f.ctx).unwrap_err(), ErrorCode::SignatureInvalid);
}

#[test]
fn wrong_vault_and_wrong_epoch_rejected_even_with_valid_proofs() {
    let f = fx();
    let newdev = SoftwareDevice::generate("Replacement Mac", PLATFORM_MACOS);
    // Correct proof, but for another vault id.
    let other_vault = build::recovery_epoch(&f.st, OTHER_VAULT, M_OLD, &f.vk, &newdev).unwrap();
    assert_eq!(verify_chain(&with(&f.base, other_vault), &VAULT, &f.ctx).unwrap_err(), ErrorCode::DeviceNotAuthorized);
    // Correct proof, stale prior epoch (state says 0; entry claims 1 → 2).
    let mut st_later = f.st.clone();
    st_later.epoch = 1;
    let bad_epoch = build::recovery_epoch(&st_later, VAULT, M_OLD, &f.vk, &newdev).unwrap();
    assert_eq!(verify_chain(&with(&f.base, bad_epoch), &VAULT, &f.ctx).unwrap_err(), ErrorCode::DeviceNotAuthorized);
}

/// RG-09 / RG-16: an epoch bound to a manifest this verifier has
/// superseded is a rollback; a copied transition cannot be replayed.
#[test]
fn rg09_rg16_stale_binding_and_replay_rejected() {
    let mut f = fx();
    let newdev = SoftwareDevice::generate("Replacement Mac", PLATFORM_MACOS);
    let ep = epoch_for(&f, &newdev);
    f.ctx.superseded.push(M_OLD);
    assert_eq!(verify_chain(&with(&f.base, ep.clone()), &VAULT, &f.ctx).unwrap_err(), ErrorCode::ManifestRollback);
    f.ctx.superseded.clear();
    // Accepted once; replaying the same bytes later fails the chain link…
    let chain = with(&f.base, ep.clone());
    let st = verify_chain(&chain, &VAULT, &f.ctx).unwrap();
    let mut replay = ep.clone();
    replay.seq = st.next_seq();
    assert!(verify_chain(&with(&chain, ep), &VAULT, &f.ctx).is_err(), "verbatim replay");
    // …and re-pointing it at the new head breaks the proof (prev/seq bound).
    replay.prev_hash = st.head;
    assert!(verify_chain(&with(&chain, replay), &VAULT, &f.ctx).is_err(), "re-linked replay");
}

#[test]
fn verifier_without_the_bound_vk_never_accepts_on_trust() {
    let f = fx();
    let ep = epoch_for(&f, &SoftwareDevice::generate("Replacement Mac", PLATFORM_MACOS));
    let blind = Ctx { vks: HashMap::new(), superseded: Vec::new() };
    assert_eq!(verify_chain(&with(&f.base, ep), &VAULT, &blind).unwrap_err(), ErrorCode::DeviceNotAuthorized);
}

#[test]
fn epoch_cannot_reinstall_an_existing_device_id() {
    let f = fx();
    let ep = build::recovery_epoch(&f.st, VAULT, M_OLD, &f.vk, &f.phone).unwrap();
    assert_eq!(verify_chain(&with(&f.base, ep), &VAULT, &f.ctx).unwrap_err(), ErrorCode::DeviceNotAuthorized);
}

/// §4.4 rules for signed kinds: self-signed enroll (RG-11), unenrolled
/// authorizer / forged revocation (RG-03, RG-07), truncation (RG-04),
/// fork (RG-05).
#[test]
fn signed_kind_rules_truncation_and_fork() {
    let f = fx();
    let stranger = SoftwareDevice::generate("Stranger", PLATFORM_MACOS);
    let self_enroll = build::enroll(&f.st, &stranger, &stranger).unwrap();
    assert!(verify_chain(&with(&f.base, self_enroll), &VAULT, &f.ctx).is_err(), "RG-11");
    let forged_revoke = build::revoke(&f.st, &stranger, f.mac.device_id()).unwrap();
    assert_eq!(verify_chain(&with(&f.base, forged_revoke), &VAULT, &f.ctx).unwrap_err(), ErrorCode::DeviceNotAuthorized);
    let ok_revoke = build::revoke(&f.st, &f.mac, f.phone.device_id()).unwrap();
    let chain = with(&f.base, ok_revoke);
    let st = verify_chain(&chain, &VAULT, &f.ctx).unwrap();
    assert!(st.active_device(&f.phone.device_id()).is_none());
    // A revoked device can no longer authorize anything.
    let late = build::enroll(&st, &f.phone, &stranger).unwrap();
    assert!(verify_chain(&with(&chain, late), &VAULT, &f.ctx).is_err());
    // Truncation and fork against the locally accepted chain.
    assert_eq!(check_extends(&chain, &f.base).unwrap_err(), ErrorCode::RegistryTruncated);
    let fork_tip = build::enroll(&f.st, &f.mac, &stranger).unwrap();
    assert_eq!(check_extends(&chain, &with(&f.base, fork_tip)).unwrap_err(), ErrorCode::RegistryFork);
    check_extends(&f.base, &chain).unwrap();
}
