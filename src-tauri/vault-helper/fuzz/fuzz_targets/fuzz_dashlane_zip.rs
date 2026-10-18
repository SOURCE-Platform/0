#![no_main]
//! Fuzz target (spec §10.7, Phase I; final review): arbitrary bytes as a
//! "ZIP export" through the central-directory parser, the member CRC/size
//! check and the inflate stream (flate2 → miniz_oxide). Must never panic,
//! and a member never yields more than its declared size.

use std::io::{Read, Write};

use libfuzzer_sys::fuzz_target;
use vault_helper::import::zip;

fuzz_target!(|data: &[u8]| {
    let path = std::env::temp_dir().join(format!("ov0-fuzz-zip-{}", std::process::id()));
    let Ok(mut f) = std::fs::File::create(&path) else { return };
    if f.write_all(data).is_err() {
        return;
    }
    drop(f);
    let Ok(f) = std::fs::File::open(&path) else { return };
    let _ = zip::looks_like_zip(&f);
    if let Some(members) = zip::members(&f) {
        for m in &members {
            let _ = zip::verify_member(&f, m);
            if let Some(mut r) = zip::open_member(&f, m) {
                let mut out = Vec::new();
                let _ = r.by_ref().take(zip::MAX_MEMBER + 1).read_to_end(&mut out);
                assert!(out.len() as u64 <= m.size(), "never more than declared");
            }
        }
    }
});
