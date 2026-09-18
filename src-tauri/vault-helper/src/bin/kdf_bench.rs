//! Argon2id calibration benchmark (spec §2.3).
//!
//! Measures the v1 tuple and candidate neighbors on this machine so the
//! committed defaults carry evidence. Run in release:
//!
//!   cargo run -p source-vault-helper --release --bin kdf_bench
//!
//! Emits a Markdown table for docs/security/argon2-calibration.md.
//! Debug builds warn: debug Argon2id is not representative.

use std::process::ExitCode;
use std::time::Instant;

use argon2::{Algorithm, Argon2, Params, Version};
use vault_helper::crypto::hex;

struct Candidate {
    memory_kib: u32,
    time: u32,
    lanes: u32,
    note: &'static str,
}

/// v1 tuple first, then upward candidates (§2.3's 128 MiB alternative
/// included; time-up and memory-up neighbors bracket the latency budget).
const CANDIDATES: &[Candidate] = &[
    Candidate {
        memory_kib: 65536,
        time: 3,
        lanes: 1,
        note: "spec v1 tuple",
    },
    Candidate {
        memory_kib: 65536,
        time: 4,
        lanes: 1,
        note: "time-up neighbor",
    },
    Candidate {
        memory_kib: 131072,
        time: 1,
        lanes: 1,
        note: "memory-up neighbor",
    },
    Candidate {
        memory_kib: 131072,
        time: 2,
        lanes: 1,
        note: "128 MiB candidate",
    },
    Candidate {
        memory_kib: 131072,
        time: 3,
        lanes: 1,
        note: "§2.3 upgrade candidate",
    },
];

const SAMPLES: usize = 3;

fn peak_rss_kib() -> u64 {
    // SAFETY: getrusage with a valid out-pointer; RUSAGE_SELF.
    unsafe {
        let mut usage: libc::rusage = std::mem::zeroed();
        libc::getrusage(libc::RUSAGE_SELF, &mut usage);
        usage.ru_maxrss as u64 / 1024 // macOS reports bytes
    }
}

fn run_candidate(c: &Candidate, password: &[u8], salt: &[u8]) -> (u128, u64) {
    let params = Params::new(c.memory_kib, c.time, c.lanes, Some(32)).unwrap();
    let argon = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);
    let mut out = [0u8; 32];
    // warmup
    argon.hash_password_into(password, salt, &mut out).unwrap();
    let mut best_ms = u128::MAX;
    let base_rss = peak_rss_kib();
    for _ in 0..SAMPLES {
        let start = Instant::now();
        argon.hash_password_into(password, salt, &mut out).unwrap();
        best_ms = best_ms.min(start.elapsed().as_millis());
    }
    (
        best_ms,
        peak_rss_kib().saturating_sub(base_rss) + u64::from(c.memory_kib),
    )
}

fn machine_info() -> String {
    let model = std::process::Command::new("sysctl")
        .args(["-n", "hw.model"])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| "unknown".into());
    let os = std::process::Command::new("sw_vers")
        .arg("-productVersion")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| "unknown".into());
    format!("{model}, macOS {os}")
}

fn main() -> ExitCode {
    let debug = cfg!(debug_assertions);
    let password = b"ov0 calibration synthetic password";
    let salt = hex::decode("30313233343536373839414243444546").unwrap();

    println!("## Argon2id calibration");
    println!();
    println!("machine: {}", machine_info());
    println!(
        "build: {}",
        if debug {
            "DEBUG (not representative)"
        } else {
            "release"
        }
    );
    println!("samples: {SAMPLES} (min latency), warmup 1");
    println!();
    println!("| memory (KiB) | time | lanes | min latency (ms) | ~memory (KiB) | note |");
    println!("|---|---|---|---|---|---|");
    for c in CANDIDATES {
        let (ms, rss) = run_candidate(c, password, &salt);
        println!(
            "| {} | {} | {} | {} | {} | {} |",
            c.memory_kib, c.time, c.lanes, ms, rss, c.note
        );
    }
    if debug {
        eprintln!("warning: debug build — rerun with --release for committed numbers");
    }
    ExitCode::SUCCESS
}
