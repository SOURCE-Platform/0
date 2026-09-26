//! Canonical TLV binary encoding (spec §4.2) — the deterministic form used
//! for registry signatures, hashes, approval payloads, and wrap payloads.
//!
//! Canonicalization rules (all enforced on decode; violations are hard
//! errors, never best-effort):
//! - tags strictly ascending within an entry; 0x00 (document wrapper) and
//!   0xFF (entry terminator) are reserved and never valid field tags;
//! - integers are minimal-length big-endian (`0` ↔ `[0x00]`); padded or
//!   >8-byte integers are rejected;
//! - strings are UTF-8, NFC-normalized at encode; decode rejects non-NFC;
//! - `Len` counts value bytes only; every length must be exactly consumed;
//!   trailing bytes after the terminator are rejected.

use unicode_normalization::{is_nfc, UnicodeNormalization};

pub const TAG_DOCUMENT: u8 = 0x00;
pub const TAG_END_OF_ENTRY: u8 = 0xFF;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TlvError {
    TagOrder,
    ReservedTag,
    PaddedInteger,
    IntegerTooLong,
    EmptyInteger,
    NonNfcString,
    StringTooLong,
    InvalidUtf8,
    Truncated,
    TrailingBytes,
    MissingTerminator,
    MalformedDocument,
}

impl std::fmt::Display for TlvError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for TlvError {}

/// Minimal-length big-endian encoding of a u64 (`0` → `[0x00]`).
pub fn encode_uint(value: u64) -> Vec<u8> {
    if value == 0 {
        return vec![0x00];
    }
    let bytes = value.to_be_bytes();
    let first_nonzero = bytes.iter().position(|&b| b != 0).unwrap();
    bytes[first_nonzero..].to_vec()
}

pub fn decode_uint(bytes: &[u8]) -> Result<u64, TlvError> {
    if bytes.is_empty() {
        return Err(TlvError::EmptyInteger);
    }
    if bytes.len() > 8 {
        return Err(TlvError::IntegerTooLong);
    }
    if bytes.len() > 1 && bytes[0] == 0 {
        return Err(TlvError::PaddedInteger);
    }
    let mut padded = [0u8; 8];
    padded[8 - bytes.len()..].copy_from_slice(bytes);
    Ok(u64::from_be_bytes(padded))
}

/// Entry encoder. Fields must be added in ascending tag order.
#[derive(Default)]
pub struct EntryBuilder {
    buf: Vec<u8>,
    last_tag: Option<u8>,
}

impl EntryBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    fn push(&mut self, tag: u8, value: &[u8]) -> Result<(), TlvError> {
        if tag == TAG_DOCUMENT || tag == TAG_END_OF_ENTRY {
            return Err(TlvError::ReservedTag);
        }
        if self.last_tag.is_some_and(|last| tag <= last) {
            return Err(TlvError::TagOrder);
        }
        self.last_tag = Some(tag);
        self.buf.push(tag);
        self.buf
            .extend_from_slice(&(value.len() as u32).to_be_bytes());
        self.buf.extend_from_slice(value);
        Ok(())
    }

    pub fn field_bytes(mut self, tag: u8, value: &[u8]) -> Result<Self, TlvError> {
        self.push(tag, value)?;
        Ok(self)
    }

    pub fn field_uint(self, tag: u8, value: u64) -> Result<Self, TlvError> {
        self.field_bytes(tag, &encode_uint(value))
    }

    /// Strings are NFC-normalized at encode (§4.2).
    pub fn field_string(self, tag: u8, value: &str) -> Result<Self, TlvError> {
        let normalized: String = value.nfc().collect();
        self.field_bytes(tag, normalized.as_bytes())
    }

    pub fn build(mut self) -> Vec<u8> {
        self.buf.push(TAG_END_OF_ENTRY);
        self.buf
    }
}

/// Strictly decoded entry: field list in ascending tag order.
pub struct EntryReader<'a> {
    fields: Vec<(u8, &'a [u8])>,
}

impl<'a> EntryReader<'a> {
    /// Parse one entry: Field* 0xFF, then EOF. Trailing bytes are an error.
    pub fn parse(bytes: &'a [u8]) -> Result<Self, TlvError> {
        let (reader, rest) = Self::parse_prefix(bytes)?;
        if !rest.is_empty() {
            return Err(TlvError::TrailingBytes);
        }
        Ok(reader)
    }

    /// Parse one entry from the front of `bytes`, returning the remainder.
    pub fn parse_prefix(bytes: &'a [u8]) -> Result<(Self, &'a [u8]), TlvError> {
        let mut fields = Vec::new();
        let mut cursor = bytes;
        let mut last_tag: Option<u8> = None;
        loop {
            let (&tag, rest) = cursor.split_first().ok_or(TlvError::MissingTerminator)?;
            cursor = rest;
            if tag == TAG_END_OF_ENTRY {
                return Ok((EntryReader { fields }, cursor));
            }
            if tag == TAG_DOCUMENT {
                return Err(TlvError::ReservedTag);
            }
            if last_tag.is_some_and(|last| tag <= last) {
                return Err(TlvError::TagOrder);
            }
            last_tag = Some(tag);
            if cursor.len() < 4 {
                return Err(TlvError::Truncated);
            }
            let len = u32::from_be_bytes([cursor[0], cursor[1], cursor[2], cursor[3]]) as usize;
            cursor = &cursor[4..];
            if cursor.len() < len {
                return Err(TlvError::Truncated);
            }
            fields.push((tag, &cursor[..len]));
            cursor = &cursor[len..];
        }
    }

    pub fn get(&self, tag: u8) -> Option<&'a [u8]> {
        self.fields.iter().find(|(t, _)| *t == tag).map(|(_, v)| *v)
    }

    pub fn tags(&self) -> impl Iterator<Item = u8> + '_ {
        self.fields.iter().map(|(t, _)| *t)
    }

    pub fn get_uint(&self, tag: u8) -> Result<Option<u64>, TlvError> {
        self.get(tag).map(decode_uint).transpose()
    }

    /// Decode a string: UTF-8, NFC required, `max_chars` enforced (§4.2).
    pub fn get_string(&self, tag: u8, max_chars: usize) -> Result<Option<String>, TlvError> {
        let Some(bytes) = self.get(tag) else {
            return Ok(None);
        };
        let s = std::str::from_utf8(bytes).map_err(|_| TlvError::InvalidUtf8)?;
        if !is_nfc(s) {
            return Err(TlvError::NonNfcString);
        }
        if s.chars().count() > max_chars {
            return Err(TlvError::StringTooLong);
        }
        Ok(Some(s.to_string()))
    }
}

/// Document := Tag(0x00) Len(u32be) Entry* — storage/transport wrapper.
pub fn encode_document(entries: &[Vec<u8>]) -> Vec<u8> {
    let body: Vec<u8> = entries.concat();
    let mut out = vec![TAG_DOCUMENT];
    out.extend_from_slice(&(body.len() as u32).to_be_bytes());
    out.extend_from_slice(&body);
    out
}

/// Strict document decode: wrapper tag/len exact, each entry strict,
/// no trailing bytes.
pub fn decode_document(bytes: &[u8]) -> Result<Vec<Vec<u8>>, TlvError> {
    let (&tag, rest) = bytes.split_first().ok_or(TlvError::MalformedDocument)?;
    if tag != TAG_DOCUMENT || rest.len() < 4 {
        return Err(TlvError::MalformedDocument);
    }
    let len = u32::from_be_bytes([rest[0], rest[1], rest[2], rest[3]]) as usize;
    let body = &rest[4..];
    if body.len() != len {
        return Err(TlvError::MalformedDocument);
    }
    let mut entries = Vec::new();
    let mut cursor = body;
    while !cursor.is_empty() {
        let (_, rest) = EntryReader::parse_prefix(cursor)?;
        let entry_len = cursor.len() - rest.len();
        entries.push(cursor[..entry_len].to_vec());
        cursor = rest;
    }
    Ok(entries)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uint_minimal_big_endian() {
        assert_eq!(encode_uint(0), vec![0x00]);
        assert_eq!(encode_uint(255), vec![0xff]);
        assert_eq!(encode_uint(256), vec![0x01, 0x00]);
        assert_eq!(decode_uint(&[0x00]), Ok(0));
        assert_eq!(decode_uint(&[0x01, 0x00]), Ok(256));
        assert_eq!(decode_uint(&[0x00, 0x01]), Err(TlvError::PaddedInteger));
        assert_eq!(decode_uint(&[]), Err(TlvError::EmptyInteger));
        assert_eq!(decode_uint(&[0u8; 9]), Err(TlvError::IntegerTooLong));
    }

    #[test]
    fn entry_roundtrip() {
        let bytes = EntryBuilder::new()
            .field_uint(0x01, 2)
            .unwrap()
            .field_string(0x07, "Adám's Mac")
            .unwrap()
            .field_bytes(0x09, &[0x04; 65])
            .unwrap()
            .build();
        let reader = EntryReader::parse(&bytes).unwrap();
        assert_eq!(reader.get_uint(0x01).unwrap(), Some(2));
        assert_eq!(
            reader.get_string(0x07, 64).unwrap().as_deref(),
            Some("Adám's Mac")
        );
        assert_eq!(reader.get(0x09), Some(&[0x04; 65][..]));
        assert_eq!(reader.get(0x10), None);
    }

    #[test]
    fn document_roundtrip() {
        let a = EntryBuilder::new().field_uint(0x01, 0).unwrap().build();
        let b = EntryBuilder::new().field_uint(0x01, 1).unwrap().build();
        let doc = encode_document(&[a.clone(), b.clone()]);
        assert_eq!(decode_document(&doc).unwrap(), vec![a, b]);
    }
}
