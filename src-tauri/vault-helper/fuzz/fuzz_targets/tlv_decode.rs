#![no_main]
//! Fuzz target (spec §16.1, Phase B): the canonical TLV decoder and the
//! registry-entry decoder over arbitrary bytes. These parsers face
//! untrusted disk content; they must reject, never panic, overflow, or
//! loop. Any accepted input must re-encode byte-identically (RG-10
//! round-trip property checked here as an oracle).

use libfuzzer_sys::fuzz_target;
use vault_helper::crypto::registry::RegistryEntry;
use vault_helper::crypto::tlv::EntryReader;

fuzz_target!(|data: &[u8]| {
    let _ = EntryReader::parse(data);
    if let Ok(entry) = RegistryEntry::decode_tlv(data) {
        let re = entry.encode_tlv(true).expect("accepted entry must encode");
        assert_eq!(re, data, "canonical round-trip violated");
    }
});
