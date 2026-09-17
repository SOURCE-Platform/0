//! macOS virtual key code to character mapping, split out of
//! `keyboard_macos.rs` to keep the listener under the repo file-length cap.

/// Convert macOS key code to character.
///
/// Simplified mapping; a full implementation would handle keyboard layouts
/// and Unicode.
pub(crate) fn key_code_to_char(key_code: u32) -> Option<char> {
    match key_code {
        // Letters
        0 => Some('a'),
        11 => Some('b'),
        8 => Some('c'),
        2 => Some('d'),
        14 => Some('e'),
        3 => Some('f'),
        5 => Some('g'),
        4 => Some('h'),
        34 => Some('i'),
        38 => Some('j'),
        40 => Some('k'),
        37 => Some('l'),
        46 => Some('m'),
        45 => Some('n'),
        31 => Some('o'),
        35 => Some('p'),
        12 => Some('q'),
        15 => Some('r'),
        1 => Some('s'),
        17 => Some('t'),
        32 => Some('u'),
        9 => Some('v'),
        13 => Some('w'),
        7 => Some('x'),
        16 => Some('y'),
        6 => Some('z'),

        // Numbers
        29 => Some('0'),
        18 => Some('1'),
        19 => Some('2'),
        20 => Some('3'),
        21 => Some('4'),
        23 => Some('5'),
        22 => Some('6'),
        26 => Some('7'),
        28 => Some('8'),
        25 => Some('9'),

        // Special keys
        36 => Some('\n'), // Return
        48 => Some('\t'), // Tab
        49 => Some(' '),  // Space

        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_key_code_to_char() {
        assert_eq!(key_code_to_char(0), Some('a'));
        assert_eq!(key_code_to_char(1), Some('s'));
        assert_eq!(key_code_to_char(18), Some('1'));
        assert_eq!(key_code_to_char(49), Some(' '));
        assert_eq!(key_code_to_char(999), None);
    }
}
