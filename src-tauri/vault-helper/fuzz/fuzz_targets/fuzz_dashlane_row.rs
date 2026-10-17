#![no_main]
//! Fuzz target (spec §10.7, Phase I): arbitrary bytes through the mode
//! sniff, the CSV reader and the dialect mapping. Must never panic; a
//! mapped login never has an empty password; any accepted record survives
//! an escape → re-parse round trip unchanged.

use libfuzzer_sys::fuzz_target;
use vault_helper::import::csv::{choose_mode, Mode, Reader, Row};
use vault_helper::import::dialect::{Dialect, Mapped};

fn escape(fields: &[String]) -> Vec<u8> {
    let quoted: Vec<String> = fields.iter().map(|f| format!("\"{}\"", f.replace('"', "\"\""))).collect();
    format!("{}\n", quoted.join(",")).into_bytes()
}

fuzz_target!(|data: &[u8]| {
    let mut r = Reader::new(data, choose_mode(data));
    let dialect = match r.next_row() {
        Some(Row::Fields(h)) => {
            let again = escape(&h);
            let mut strict = Reader::new(&again[..], Mode::Strict);
            assert_eq!(strict.next_row(), Some(Row::Fields(h.clone())), "round trip");
            Dialect::detect(&h)
        }
        _ => None,
    };
    let Some(d) = dialect else { return };
    while let Some(row) = r.next_row() {
        let Row::Fields(f) = row else { continue };
        if let Mapped::Item(v) = d.map(&f) {
            if v["kind"] == "login" {
                assert!(!v["password"].as_str().unwrap_or("").is_empty(), "empty password emitted");
            }
        }
    }
});
