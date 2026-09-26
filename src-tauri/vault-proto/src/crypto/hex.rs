//! In-house hex helpers for JSON file fields (§2.5) and vector files
//! (§16.8). Avoids a `hex` crate in the helper dependency tree.

pub fn encode(bytes: impl AsRef<[u8]>) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let bytes = bytes.as_ref();
    let mut out = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0x0f) as usize] as char);
    }
    out
}

fn nibble(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        _ => None,
    }
}

pub fn decode(s: &str) -> Option<Vec<u8>> {
    let bytes = s.as_bytes();
    if !bytes.len().is_multiple_of(2) {
        return None;
    }
    let mut out = Vec::with_capacity(bytes.len() / 2);
    for pair in bytes.chunks_exact(2) {
        out.push((nibble(pair[0])? << 4) | nibble(pair[1])?);
    }
    Some(out)
}

pub fn decode_array<const N: usize>(s: &str) -> Option<[u8; N]> {
    let vec = decode(s)?;
    vec.try_into().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_and_strictness() {
        assert_eq!(encode([0x00, 0xff, 0x1a]), "00ff1a");
        assert_eq!(decode("00ff1a"), Some(vec![0x00, 0xff, 0x1a]));
        assert_eq!(decode("00FF1A"), None); // lowercase only, no mixed input
        assert_eq!(decode("0g"), None);
        assert_eq!(decode("abc"), None);
        assert_eq!(decode_array::<2>("00ff"), Some([0x00, 0xff]));
        assert_eq!(decode_array::<3>("00ff"), None);
    }
}
