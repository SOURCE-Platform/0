//! `import_dashlane {path}` (spec §10.4; Phase I; Mac only — not in
//! `IOS_OPS`). The app hands over the path the file dialog returned; the
//! helper opens the file itself:
//!
//! - an absolute path to a regular file, opened without following a link
//!   (`O_NOFOLLOW`) and checked on the open handle, so a swap after the
//!   check cannot redirect it;
//! - not inside the vault directory;
//! - at most 2 GiB.
//!
//! One presence check covers the whole import ("Source Vault: import from
//! Dashlane"); the run happens inside that authorized window.

use std::fs::{File, OpenOptions};
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;
use std::sync::{Arc, Mutex};

use serde_json::Value;

use super::gate::{finish_authorized, presence_gate};
use super::import_run::Run;
use super::{lock_core, Deps, OpOutcome, VaultCore};
use crate::crypto::secret::SecretBytes;
use crate::errors::ErrorCode;
use crate::storage::VaultStore;

pub const MAX_FILE: u64 = 2 << 30;

/// Open the export, or say why not (before any presence prompt).
pub fn open_export(path: &str, vault_dir: &Path) -> Result<File, ErrorCode> {
    let p = Path::new(path);
    if !p.is_absolute() || path.contains('\0') {
        return Err(ErrorCode::InvalidInput);
    }
    let f = OpenOptions::new().read(true).custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK).open(p).map_err(|_| ErrorCode::InvalidInput)?;
    let meta = f.metadata().map_err(|_| ErrorCode::InvalidInput)?;
    if !meta.is_file() || meta.len() > MAX_FILE {
        return Err(ErrorCode::InvalidInput);
    }
    let (Ok(real), Ok(vault)) = (p.canonicalize(), vault_dir.canonicalize()) else {
        return Err(ErrorCode::InvalidInput);
    };
    if real.starts_with(&vault) {
        return Err(ErrorCode::InvalidInput);
    }
    Ok(f)
}

pub fn import_dashlane(core: &Arc<Mutex<VaultCore>>, frame: &Value, deps: &Deps) -> OpOutcome {
    let Some(path) = frame.get("path").and_then(Value::as_str) else {
        return OpOutcome::err(ErrorCode::InvalidInput);
    };
    let vault_dir = lock_core(core).vault_dir.clone();
    let file = match open_export(path, &vault_dir) {
        Ok(f) => f,
        Err(e) => return OpOutcome::err(e),
    };
    if let Err(o) = presence_gate(core, deps, "Source Vault: import from Dashlane") {
        return o;
    }
    finish_authorized!(core, deps, move |store: &mut VaultStore, vk: &SecretBytes<32>| {
        let started = std::time::Instant::now();
        let mut run = Run::new(store, vk)?;
        // A failure mid-way keeps the batches already committed; the answer
        // still carries what came in (final review SPEC-I6 #4).
        let outcome = run.file(&file);
        let mut report = run.report.to_json(started.elapsed().as_millis());
        if let Err(e) = outcome {
            report["stopped"] = serde_json::json!(e.as_str());
            report["keep_export"] = serde_json::json!(true);
        }
        Ok(serde_json::json!({ "report": report }))
    })
}
