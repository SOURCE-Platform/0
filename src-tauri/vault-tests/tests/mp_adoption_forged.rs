//! MA-06 / MA-07 (spec §2.7; reviews SEC-B1, SEC-I1 of F.2d step 1): the
//! master-password panel is never raised for a served state that does not
//! pass the apply's own verification — here a provider re-signs the
//! current state with a key of its own, at a new key generation — nor
//! when the served `wrap_mp` names KDF costs other than the served
//! header's (the stated stolen-signing-key residual can serve a validly
//! signed state, never a wrap that makes the password run through costs
//! of its choosing). A forging transport sits between the coordinator and
//! the provider core. Synthetic data only.

#[path = "../../vault-helper/tests/vault_fx/mod.rs"]
mod vault_fx;
mod mfx;

use std::collections::HashMap;
use std::sync::Mutex;

use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use vault_coordinator::flows::Flows;
use vault_coordinator::{Helper, HttpResponse, Transport, TransportError};
use vault_fx::{submitted, Fx, MP};
use vault_helper::backup::index::{IndexEntry, ObjectIndex, Role};
use vault_helper::backup::manifest::SignedManifest;
use vault_helper::registry::device::{DeviceIdentity, SoftwareDevice, PLATFORM_MACOS};
use vault_provider_core::stores::BlobStore;

struct FxHelper<'a>(&'a Fx);
impl Helper for FxHelper<'_> {
    fn op(&self, frame: Value) -> Result<Value, String> {
        Ok(self.0.op(frame))
    }
}

/// Passes everything to the provider core, except that a served state is
/// replaced by `forge(state)` and the forged blobs are served on request.
struct Forging<'a> {
    cloud: &'a mfx::Cloud,
    forge: Box<dyn Fn(&[u8]) -> (Vec<u8>, Vec<Vec<u8>>) + Send + Sync + 'a>,
    extra: Mutex<HashMap<String, Vec<u8>>>,
}

impl Transport for Forging<'_> {
    fn send(&self, _origin: &str, method: &str, path: &str, auth: Option<&str>, body: &[u8]) -> Result<HttpResponse, TransportError> {
        if let Some((_, b)) = self.extra.lock().unwrap().iter().find(|(h, _)| path.contains(h.as_str())) {
            return Ok(HttpResponse { status: 200, body: b.clone(), date: Some(mfx::now()) });
        }
        let r = self.cloud.send(method, path, auth, body);
        let is_state = serde_json::from_slice::<Value>(&r.body).ok().is_some_and(|v| v.get("state_commit").is_some());
        let body = if r.status == 200 && is_state {
            let (state, blobs) = (self.forge)(&r.body);
            for b in blobs {
                self.extra.lock().unwrap().insert(vault_helper::crypto::hex::encode(<[u8; 32]>::from(Sha256::digest(&b))), b);
            }
            state
        } else {
            r.body
        };
        Ok(HttpResponse { status: r.status, body, date: Some(mfx::now()) })
    }
}

/// The current state re-signed by `signer` at the next generation and key
/// generation; `edit_wrap` may replace the served `wrap_mp`.
fn forge(cloud: &mfx::Cloud, signer: &dyn DeviceIdentity, state: &[u8], edit_wrap: Option<&(dyn Fn(&mut Value) + Sync)>) -> (Vec<u8>, Vec<Vec<u8>>) {
    let real = vault_helper::sync::remote::parse(state).unwrap();
    let vid = real.manifest.vault_id;
    let get = |h: &[u8; 32]| cloud.fs.get(&vid, h).unwrap().unwrap();
    let index = ObjectIndex::decode(&get(&real.manifest.object_index_hash)).unwrap();
    let mut blobs = Vec::new();
    let mut entries = index.entries.clone();
    if let Some(edit) = edit_wrap {
        let e = entries.iter_mut().find(|e| e.role == Role::WrapMp).unwrap();
        let mut w: Value = serde_json::from_slice(&get(&e.blob)).unwrap();
        edit(&mut w);
        let bytes = serde_json::to_vec_pretty(&w).unwrap();
        *e = IndexEntry::of(Role::WrapMp, &bytes);
        blobs.push(bytes);
    }
    let forged_index = ObjectIndex { generation: real.generation + 1, entries, ..index };
    let manifest = SignedManifest {
        generation: real.generation + 1,
        vk_generation: real.vk_generation + 1,
        object_index_hash: forged_index.hash(),
        prev_manifest_hash: real.manifest_hash,
        created_at: mfx::now(),
        signer_device_id: [0; 16],
        signature: [0; 64],
        ..real.manifest.clone()
    }
    .sign(signer)
    .unwrap();
    let m = manifest.encode();
    let cp = real.checkpoint_bytes.clone();
    let digest = vault_proto::state::recovery_auth_digest(&real.recovery_auth).unwrap();
    let commit = vault_proto::state::state_commit(&vid, manifest.generation, &Sha256::digest(&m).into(), &Sha256::digest(&cp).into(), &digest);
    let mut v: Value = serde_json::from_slice(state).unwrap();
    v["manifest"] = json!(vault_proto::b64::encode(&m));
    v["generation"] = json!(manifest.generation);
    v["vk_generation"] = json!(manifest.vk_generation);
    v["state_commit"] = json!(vault_helper::crypto::hex::encode(commit));
    blobs.push(forged_index.encode());
    (serde_json::to_vec(&v).unwrap(), blobs)
}

/// A password-only Mac, set up, published and unlocked.
fn password_only_mac(tag: &str) -> (mfx::Cloud, Fx) {
    let cloud = mfx::Cloud::new(tag);
    std::env::set_var("OV0_VAULT_SE_BIOMETRY", "absent");
    let fx = vault_fx::fx();
    fx.push_panel(submitted(MP));
    let setup = fx.op(json!({ "op": "setup_vault", "handle": "synthetic-forged@example.test" }));
    std::env::remove_var("OV0_VAULT_SE_BIOMETRY");
    assert_eq!(setup["ok"], true, "{setup}");
    {
        let honest = Forging { cloud: &cloud, forge: Box::new(|s| (s.to_vec(), Vec::new())), extra: Mutex::default() };
        let mac = Flows { helper: &FxHelper(&fx), transport: &honest };
        assert_eq!(mac.run_publication(&setup["publication"]).unwrap()["committed"], true);
        assert_eq!(vault_fx::unlock(&fx, MP)["ok"], true);
        vault_fx::add_login(&fx);
        assert_eq!(mac.backup_now().unwrap()["committed"], true);
    }
    (cloud, fx)
}

fn adopt_prompts(fx: &Fx) -> usize {
    fx.panel.seen.lock().unwrap().iter().filter(|r| **r == vault_helper::vault::secure_ui::PanelRequest::MpAdopt).count()
}

#[test]
fn a_state_signed_by_no_enrolled_device_raises_no_panel() {
    let _g = vault_fx::serial();
    let (cloud, fx) = password_only_mac("forged-signer");
    let evil = SoftwareDevice::generate("Provider's device", PLATFORM_MACOS);
    let t = Forging { cloud: &cloud, forge: Box::new(|s| forge(&cloud, &evil, s, None)), extra: Mutex::default() };
    let mac = Flows { helper: &FxHelper(&fx), transport: &t };
    fx.push_panel(submitted(MP));
    assert!(mac.run_sync_by_user().is_err(), "refused");
    assert_eq!(adopt_prompts(&fx), 0, "no panel before verification");
    fx.remove_dir();
}

#[test]
fn a_wrap_with_other_kdf_costs_raises_no_panel() {
    let _g = vault_fx::serial();
    let (cloud, fx) = password_only_mac("forged-kdf");
    // The stated residual: someone holding this Mac's signing key.
    let own = vault_helper::device::SeDevice::load(&fx.dir).unwrap();
    // Another KDF block than the served header's. (A raised cost would make
    // a regression hang rather than fail; a different salt fails fast.)
    let heavy = |w: &mut Value| w["argon2id"]["salt"] = json!("ab".repeat(16));
    let t = Forging { cloud: &cloud, forge: Box::new(|s| forge(&cloud, &own, s, Some(&heavy))), extra: Mutex::default() };
    let mac = Flows { helper: &FxHelper(&fx), transport: &t };
    fx.push_panel(submitted(MP));
    assert!(mac.run_sync_by_user().is_err(), "refused");
    assert_eq!(adopt_prompts(&fx), 0, "a wrap outside the header's KDF block is never offered the password");
    fx.remove_dir();
}
