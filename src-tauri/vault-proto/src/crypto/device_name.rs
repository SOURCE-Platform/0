//! Device names for the registry (final review, core 5).

use super::registry::DEVICE_NAME_MAX_CHARS;

/// A device name as the registry stores it: NFC first, then cut to
/// `DEVICE_NAME_MAX_CHARS` (final review, core 5: cutting first let a name
/// grow past the limit when normalized, so the entry could not be read
/// back). A prefix of an NFC string never lengthens when re-normalized.
pub fn clamp_device_name(raw: &str) -> String {
    use unicode_normalization::UnicodeNormalization;
    raw.nfc().take(DEVICE_NAME_MAX_CHARS).collect()
}

#[cfg(test)]
mod clamp_tests {
    #[test]
    fn a_clamped_name_never_exceeds_the_limit_once_normalized() {
        // U+FB2A decomposes (NFC) into two characters.
        let name = "\u{FB2A}".repeat(64);
        let clamped = super::clamp_device_name(&name);
        assert!(clamped.chars().count() <= super::super::registry::DEVICE_NAME_MAX_CHARS);
        use unicode_normalization::UnicodeNormalization;
        assert_eq!(clamped.nfc().collect::<String>(), clamped, "already NFC");
    }
}
