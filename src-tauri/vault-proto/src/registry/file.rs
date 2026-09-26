//! `registry.json` storage form (spec §4.1): one entry per line (JSONL),
//! each line `{"tlv":"<hex of the canonical TLV>"}`. Two files are
//! equivalent iff their TLV decodings are identical — the TLV is the only
//! thing that is hashed or signed.

use crate::crypto::hex;
use crate::crypto::registry::RegistryEntry;
use crate::errors::ErrorCode;

pub fn encode(entries: &[RegistryEntry]) -> Result<Vec<u8>, ErrorCode> {
    let mut out = Vec::new();
    for e in entries {
        let tlv = e.encode_tlv(true).map_err(|_| ErrorCode::Internal)?;
        out.extend_from_slice(format!("{{\"tlv\":\"{}\"}}\n", hex::encode(tlv)).as_bytes());
    }
    Ok(out)
}

pub fn decode(bytes: &[u8]) -> Result<Vec<RegistryEntry>, ErrorCode> {
    let text = std::str::from_utf8(bytes).map_err(|_| ErrorCode::SignatureInvalid)?;
    let mut out = Vec::new();
    if text.trim() == "[]" {
        return Ok(out); // Phase C wrote an empty JSON array
    }
    for line in text.lines().filter(|l| !l.trim().is_empty()) {
        let v: serde_json::Value = serde_json::from_str(line).map_err(|_| ErrorCode::SignatureInvalid)?;
        let tlv_hex = v.get("tlv").and_then(|t| t.as_str()).ok_or(ErrorCode::SignatureInvalid)?;
        let tlv = hex::decode(tlv_hex).ok_or(ErrorCode::SignatureInvalid)?;
        out.push(RegistryEntry::decode_tlv(&tlv).map_err(|_| ErrorCode::SignatureInvalid)?);
    }
    Ok(out)
}
