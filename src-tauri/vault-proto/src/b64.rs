//! Unpadded base64url (RFC 4648 §5) for the `Ov0-Auth` header (§11.4).
//! Decoding is strict: canonical alphabet, no padding, no trailing bits.

const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

pub fn encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for i in 0..=chunk.len() {
            out.push(ALPHABET[((n >> (18 - 6 * i)) & 63) as usize] as char);
        }
    }
    out
}

pub fn decode(s: &str) -> Option<Vec<u8>> {
    let val = |c: u8| ALPHABET.iter().position(|&a| a == c).map(|p| p as u32);
    let bytes = s.as_bytes();
    if bytes.len() % 4 == 1 {
        return None;
    }
    let mut out = Vec::with_capacity(bytes.len() * 3 / 4);
    for chunk in bytes.chunks(4) {
        let mut n = 0u32;
        for (i, &c) in chunk.iter().enumerate() {
            n |= val(c)? << (18 - 6 * i);
        }
        let take = chunk.len() - 1;
        let full = n.to_be_bytes();
        out.extend_from_slice(&full[1..1 + take]);
        // Canonical: the unused low bits of a short final chunk are zero.
        if take < 3 && (n & ((1 << (8 * (3 - take))) - 1)) != 0 {
            return None;
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_and_strictness() {
        for n in 0..40 {
            let v: Vec<u8> = (0..n).map(|i| (i * 37 + 11) as u8).collect();
            assert_eq!(decode(&encode(&v)).unwrap(), v);
        }
        assert_eq!(encode(b"\xfb\xff"), "-_8");
        assert!(decode("-_9").is_none(), "non-zero trailing bits");
        assert!(decode("A").is_none());
        assert!(decode("AA==").is_none(), "padding refused");
        assert!(decode("A+").is_none(), "standard alphabet refused");
    }
}
