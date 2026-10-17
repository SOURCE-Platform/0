//! `secure_note@1` (spec §10.8 Choice A, owner decision 2026-10-16): a
//! title and a body. The body lives only inside the record ciphertext —
//! never in metadata (the title is the only metadata-class field); notes
//! never fill a website; reveal shows the body on the capture-suppressed
//! surfaces only. Also the Luhn check the card validation uses.

use serde::{Deserialize, Serialize};

use crate::errors::ErrorCode;

pub const SCHEMA_NOTE: &str = "secure_note@1";
const TITLE_MAX: usize = 200;
/// The importer's field cap (§10.3: a field over 64 KiB is skipped).
pub const BODY_MAX: usize = 64 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecureNoteRecord {
    pub schema: String,
    pub title: String,
    pub body: String,
    pub created_at: u64,
    pub updated_at: u64,
}

pub fn validate_note(rec: &mut SecureNoteRecord) -> Result<(), ErrorCode> {
    use unicode_normalization::UnicodeNormalization;
    rec.title = rec.title.trim().nfc().collect();
    if rec.title.is_empty() || rec.title.chars().count() > TITLE_MAX || rec.body.len() > BODY_MAX {
        return Err(ErrorCode::InvalidInput);
    }
    rec.schema = SCHEMA_NOTE.to_string();
    Ok(())
}

pub(crate) fn luhn_ok(digits: &str) -> bool {
    let mut sum = 0u32;
    for (i, c) in digits.chars().rev().enumerate() {
        let mut d = c.to_digit(10).unwrap_or(0);
        if i % 2 == 1 {
            d *= 2;
            if d > 9 {
                d -= 9;
            }
        }
        sum += d;
    }
    sum % 10 == 0
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_note_needs_a_title_and_a_bounded_body() {
        let mut n = SecureNoteRecord { schema: String::new(), title: "  Wi-Fi  ".into(), body: "synthetic".into(), created_at: 1, updated_at: 1 };
        validate_note(&mut n).unwrap();
        assert_eq!((n.title.as_str(), n.schema.as_str()), ("Wi-Fi", SCHEMA_NOTE));
        n.body = "x".repeat(BODY_MAX + 1);
        assert!(validate_note(&mut n).is_err());
        n.body.clear();
        n.title.clear();
        assert!(validate_note(&mut n).is_err());
    }
}
