//! Guard rails for the Tauri asset protocol scope in `tauri.conf.json`.
//!
//! Security architecture invariant: vault paths must be *structurally*
//! outside the asset protocol's allowed scope, not merely unused by current
//! frontend code. These tests parse the real config and prove that a vault
//! path is rejected while the media paths the frontend serves still load.
//!
//! The matcher intentionally supports only the pattern subset we ship:
//! `"$HOME/<root>/**"` allow entries and `"!$HOME/<root>/**"` deny entries.
//! Anything else fails the parse test, forcing a deliberate review of both
//! the scope and this guard when the patterns change.

use serde_json::Value;
use std::path::{Path, PathBuf};

pub struct AssetScope {
    allows: Vec<PathBuf>,
    denies: Vec<PathBuf>,
}

impl AssetScope {
    /// Parse the asset scope out of tauri.conf.json content.
    pub fn from_config(config_json: &str, home: &Path) -> Result<Self, String> {
        let config: Value =
            serde_json::from_str(config_json).map_err(|e| format!("invalid config json: {e}"))?;
        let entries = config
            .pointer("/app/security/assetProtocol/scope")
            .and_then(Value::as_array)
            .ok_or("missing app.security.assetProtocol.scope")?;

        let mut allows = Vec::new();
        let mut denies = Vec::new();
        for entry in entries {
            let raw = entry.as_str().ok_or("scope entry is not a string")?;
            let (deny, pattern) = match raw.strip_prefix('!') {
                Some(rest) => (true, rest),
                None => (false, raw),
            };
            let relative = pattern
                .strip_prefix("$HOME/")
                .ok_or_else(|| format!("unsupported scope root (expected $HOME/): {raw}"))?;
            let relative = relative
                .strip_suffix("/**")
                .ok_or_else(|| format!("unsupported scope pattern (expected trailing /**): {raw}"))?;
            if relative.contains('*') {
                return Err(format!("unsupported glob inside scope pattern: {raw}"));
            }
            let root = home.join(relative);
            if deny {
                denies.push(root);
            } else {
                allows.push(root);
            }
        }
        Ok(Self { allows, denies })
    }

    /// Component-wise prefix semantics: a path is served only when it sits
    /// under an allowed root and no denied root.
    pub fn is_allowed(&self, path: &Path) -> bool {
        let allowed = self.allows.iter().any(|root| path.starts_with(root));
        let denied = self.denies.iter().any(|root| path.starts_with(root));
        allowed && !denied
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn load_real_scope() -> (AssetScope, PathBuf) {
        let config = std::fs::read_to_string("tauri.conf.json").expect("read tauri.conf.json");
        let home = dirs_home();
        let scope = AssetScope::from_config(&config, &home).expect("parse real scope");
        (scope, home)
    }

    fn dirs_home() -> PathBuf {
        PathBuf::from(std::env::var("HOME").expect("HOME set"))
    }

    #[test]
    fn vault_paths_are_rejected() {
        let (scope, home) = load_real_scope();
        for path in [
            home.join(".observer_data/vault"),
            home.join(".observer_data/vault/vault.db"),
            home.join(".observer_data/vault/wraps/password.wrap"),
        ] {
            assert!(
                !scope.is_allowed(&path),
                "vault path must not be asset-servable: {}",
                path.display()
            );
        }
    }

    #[test]
    fn required_media_paths_still_load() {
        let (scope, home) = load_real_scope();
        for path in [
            home.join(".observer_data/recordings/session-1/segments/seg.mp4"),
            home.join(".observer_data/recordings/session-1/ocr_frames/frame.png"),
            home.join(".observer_data/recordings/session-1/frames/base.png"),
        ] {
            assert!(
                scope.is_allowed(&path),
                "media path must stay servable: {}",
                path.display()
            );
        }
    }

    #[test]
    fn unrelated_home_paths_are_rejected() {
        let (scope, home) = load_real_scope();
        for path in [
            home.join(".ssh/id_ed25519"),
            home.join(".observer_data/database/observer.db"),
            home.join(".observer_data/mobile/key.pem"),
            home.join(".observer_data/vault-adjacent-decoy/file"),
        ] {
            assert!(
                !scope.is_allowed(&path),
                "path outside the media roots must be rejected: {}",
                path.display()
            );
        }
    }

    #[test]
    fn sibling_prefix_does_not_escape() {
        // Component-wise matching must not let "recordings-evil" pass for
        // a "recordings" allow root.
        let scope = AssetScope {
            allows: vec![PathBuf::from("/home/u/.observer_data/recordings")],
            denies: vec![PathBuf::from("/home/u/.observer_data/vault")],
        };
        assert!(!scope.is_allowed(Path::new("/home/u/.observer_data/recordings-evil/x.png")));
        assert!(scope.is_allowed(Path::new("/home/u/.observer_data/recordings/x.png")));
    }

    #[test]
    fn unknown_patterns_fail_parse() {
        let config = r#"{"app":{"security":{"assetProtocol":{"enable":true,"scope":["$HOME/**"]}}}}"#;
        assert!(AssetScope::from_config(config, Path::new("/home/u")).is_err());
        let config = r#"{"app":{"security":{"assetProtocol":{"enable":true,"scope":["/tmp/**"]}}}}"#;
        assert!(AssetScope::from_config(config, Path::new("/home/u")).is_err());
    }
}
