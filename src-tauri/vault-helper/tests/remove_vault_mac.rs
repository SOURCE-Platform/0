//! "Remove this vault" is the iPhone's alone (F.2d step 6): the Mac's
//! helper answers `UNKNOWN_OP` and keeps its vault. Synthetic data only.

mod vault_fx;

use serde_json::json;
use vault_fx::*;

#[test]
fn a_mac_never_removes_its_vault_this_way() {
    let _g = serial();
    let fx = fx();
    setup_and_unlock(&fx);
    assert_eq!(err_code(&fx.op(json!({"op": "remove_vault", "confirm": true}))), "UNKNOWN_OP");
    assert_eq!(err_code(&fx.op(json!({"op": "remove_vault_preview"}))), "UNKNOWN_OP");
    assert!(fx.dir.join("vault.db").exists());
    fx.remove_dir();
}
