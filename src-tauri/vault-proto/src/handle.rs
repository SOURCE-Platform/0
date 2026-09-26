//! Recovery handle normalization (spec v0.4 §11.5). The handle is a public
//! identifier, not a secret; `handle_key` is an unsalted hash of it.
//!
//! ```text
//! h1 = NFKC(s) ; h2 = trim(h1) ; h3 = NFKC(to_lowercase(h2))
//! reject unless: 3 ≤ utf8_len(h3) ≤ 128, no Unicode White_Space, no
//!   Cc/Cf/Cs/Co/Cn code points, and NFKC(to_lowercase(h3)) == h3
//! handle_key = SHA-256("ov0/handle/v2" ‖ UTF-8(h3))
//! ```

use sha2::{Digest, Sha256};
use unicode_normalization::UnicodeNormalization;

use crate::errors::ErrorCode;

pub fn normalize(s: &str) -> Result<String, ErrorCode> {
    let h1: String = s.nfkc().collect();
    let h2 = h1.trim();
    let h3: String = h2.to_lowercase().nfkc().collect();
    let stable: String = h3.to_lowercase().nfkc().collect();
    let len_ok = (3..=128).contains(&h3.len());
    if !len_ok || stable != h3 || h3.chars().any(|c| c.is_whitespace() || excluded_category(c)) {
        return Err(ErrorCode::InvalidInput);
    }
    Ok(h3)
}

pub fn handle_key(normalized: &str) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(b"ov0/handle/v2");
    h.update(normalized.as_bytes());
    h.finalize().into()
}

/// Cc (control), Cf (format), Cs (surrogate — unrepresentable in `char`),
/// Co (private use) and Cn (unassigned). Without a Unicode-database crate,
/// Cn is approximated by the noncharacters and the code points outside
/// every assigned block this build can name; Cf is the complete list of
/// format characters in Unicode 15.1.
fn excluded_category(c: char) -> bool {
    let u = c as u32;
    c.is_control()
        || is_format(u)
        || (0xE000..=0xF8FF).contains(&u)
        || (0xF0000..=0xFFFFD).contains(&u)
        || (0x100000..=0x10FFFD).contains(&u)
        || (u & 0xFFFE) == 0xFFFE
        || (0xFDD0..=0xFDEF).contains(&u)
}

fn is_format(u: u32) -> bool {
    matches!(u,
        0x00AD | 0x0600..=0x0605 | 0x061C | 0x06DD | 0x070F | 0x0890..=0x0891 | 0x08E2
        | 0x180E | 0x200B..=0x200F | 0x202A..=0x202E | 0x2060..=0x2064 | 0x2066..=0x206F
        | 0xFEFF | 0xFFF9..=0xFFFB | 0x110BD | 0x110CD | 0x13430..=0x1343F
        | 0x1BCA0..=0x1BCA3 | 0x1D173..=0x1D17A | 0xE0001 | 0xE0020..=0xE007F)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalization_and_rejections() {
        assert_eq!(normalize("  Alice@Example.TEST ").unwrap(), "alice@example.test");
        assert_eq!(normalize("ＡＢＣ").unwrap(), "abc", "NFKC folds fullwidth");
        for bad in ["ab", "a b c", "abc\u{200B}", "ab\u{7}c", "\u{E000}abc", &"x".repeat(129)] {
            assert!(normalize(bad).is_err(), "{bad:?}");
        }
        assert_eq!(handle_key("abc"), handle_key(&normalize("ABC").unwrap()));
    }
}
