//! BIP-39 recovery-key codec (spec §2.4), implemented in-house over the
//! vendored official 2048-word English list (SHA-256 of the vendored file:
//! 2f5eed53a4727b4bf8880d8f3f199efc90e58503646d9ff8eff3a2ed3b24dbda).
//!
//! RK = 32 bytes of OsRng entropy shown as 24 words (256 bits entropy +
//! 8-bit checksum). BIP-39 seed stretching (PBKDF2) is deliberately NOT
//! used: the mnemonic encodes the entropy directly (§2.4).

use sha2::{Digest, Sha256};
use std::sync::OnceLock;

use super::secret::SecretBytes;
use super::CryptoError;

const WORDLIST: &str = include_str!("data/bip39-english.txt");
const WORD_COUNT: usize = 2048;
/// RK is exactly 32 bytes of entropy → 24 words (§2.4).
pub const RK_ENTROPY_BYTES: usize = 32;
pub const RK_WORDS: usize = 24;

fn wordlist() -> &'static [&'static str; WORD_COUNT] {
    static WORDS: OnceLock<[&'static str; WORD_COUNT]> = OnceLock::new();
    WORDS.get_or_init(|| {
        let mut words = [""; WORD_COUNT];
        for (i, word) in WORDLIST.lines().enumerate() {
            words[i] = word;
        }
        debug_assert_eq!(words[0], "abandon");
        debug_assert_eq!(words[WORD_COUNT - 1], "zoo");
        words
    })
}

fn word_index(word: &str) -> Option<u16> {
    wordlist().binary_search(&word).ok().map(|i| i as u16)
}

fn checksum_bits(entropy: &[u8]) -> u8 {
    let digest = Sha256::digest(entropy);
    digest[0]
}

/// Encode entropy as a BIP-39 mnemonic. Entropy must be 16/20/24/28/32
/// bytes (BIP-39 standard); the vault only ever passes 32 (§2.4).
pub fn encode(entropy: &[u8]) -> Result<Vec<&'static str>, CryptoError> {
    let ent_bits = entropy.len() * 8;
    let cs_bits = match entropy.len() {
        16 | 20 | 24 | 28 | 32 => ent_bits / 32,
        _ => return Err(CryptoError::RecoveryKeyInvalid),
    };
    let total_bits = ent_bits + cs_bits;
    let mut bits = Vec::with_capacity(total_bits);
    for byte in entropy {
        for shift in (0..8).rev() {
            bits.push((byte >> shift) & 1);
        }
    }
    let checksum = checksum_bits(entropy);
    // BIP-39 checksum = the FIRST cs_bits of SHA-256(entropy), i.e. the
    // most-significant bits of the first digest byte.
    for shift in (8 - cs_bits..8).rev() {
        bits.push((checksum >> shift) & 1);
    }
    let words = wordlist();
    Ok(bits
        .chunks_exact(11)
        .map(|chunk| {
            let index = chunk.iter().fold(0usize, |acc, &b| (acc << 1) | b as usize);
            words[index]
        })
        .collect())
}

/// Encode a 32-byte RK as its 24 display words, space-joined (§2.4).
pub fn encode_rk(rk: &SecretBytes<32>) -> String {
    encode(rk.expose())
        .expect("RK is always 32 bytes")
        .join(" ")
}

/// Normalize user entry (§2.4): lowercase, trim, collapse whitespace.
pub fn normalize(input: &str) -> Vec<String> {
    input
        .split_whitespace()
        .map(|word| word.to_lowercase())
        .collect()
}

/// Decode a normalized-or-not mnemonic back to entropy, validating
/// wordlist membership and checksum. Any failure is the single
/// RECOVERY_KEY_INVALID error — no oracle beyond "not valid" (§2.4).
pub fn decode(input: &str) -> Result<Vec<u8>, CryptoError> {
    let words = normalize(input);
    let word_count = words.len();
    if ![12, 15, 18, 21, 24].contains(&word_count) {
        return Err(CryptoError::RecoveryKeyInvalid);
    }
    let mut bits = Vec::with_capacity(word_count * 11);
    for word in &words {
        let index = word_index(word).ok_or(CryptoError::RecoveryKeyInvalid)?;
        for shift in (0..11).rev() {
            bits.push(((index >> shift) & 1) as u8);
        }
    }
    let total_bits = bits.len();
    let cs_bits = word_count / 3; // 11 words per 4 checksum+entropy bytes... see BIP-39: CS = ENT/32
    let ent_bits = total_bits - cs_bits;
    let ent_bytes = ent_bits / 8;
    let mut entropy = vec![0u8; ent_bytes];
    for (i, &bit) in bits[..ent_bits].iter().enumerate() {
        entropy[i / 8] |= bit << (7 - (i % 8));
    }
    let mut presented = 0u8;
    for (i, &bit) in bits[ent_bits..].iter().enumerate() {
        presented |= bit << (7 - i);
    }
    // Mask to cs_bits (the presented byte's low bits are zero padding).
    let mask = if cs_bits == 8 {
        0xFF
    } else {
        0xFF << (8 - cs_bits)
    };
    if (checksum_bits(&entropy) & mask) != presented {
        return Err(CryptoError::RecoveryKeyInvalid);
    }
    Ok(entropy)
}

/// Decode exactly a 24-word RK entry (§2.4). Lengths other than 24 words
/// are invalid for the vault even if they are valid BIP-39.
pub fn decode_rk(input: &str) -> Result<SecretBytes<32>, CryptoError> {
    if normalize(input).len() != RK_WORDS {
        return Err(CryptoError::RecoveryKeyInvalid);
    }
    let entropy = decode(input)?;
    let bytes: [u8; RK_ENTROPY_BYTES] = entropy
        .try_into()
        .map_err(|_| CryptoError::RecoveryKeyInvalid)?;
    Ok(SecretBytes::new(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reference_vectors_roundtrip() {
        // BIP-39 official reference vectors (128- and 256-bit rows).
        let vectors: [(&str, &str); 4] = [
            (
                "00000000000000000000000000000000",
                "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about",
            ),
            (
                "0000000000000000000000000000000000000000000000000000000000000000",
                "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon art",
            ),
            (
                "ffffffffffffffffffffffffffffffff",
                "zoo zoo zoo zoo zoo zoo zoo zoo zoo zoo zoo wrong",
            ),
            (
                "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
                "zoo zoo zoo zoo zoo zoo zoo zoo zoo zoo zoo zoo zoo zoo zoo zoo zoo zoo zoo zoo zoo zoo zoo vote",
            ),
        ];
        for (entropy_hex, phrase) in vectors {
            let entropy = super::super::hex::decode(entropy_hex).unwrap();
            let encoded = encode(&entropy).unwrap();
            assert_eq!(encoded.join(" "), phrase, "encode {entropy_hex}");
            let decoded = decode(phrase).unwrap();
            assert_eq!(decoded, entropy, "decode {entropy_hex}");
        }
    }

    #[test]
    fn rk_roundtrip_24_words() {
        let rk = SecretBytes::new([0x42u8; 32]);
        let phrase = encode_rk(&rk);
        assert_eq!(phrase.split_whitespace().count(), RK_WORDS);
        let back = decode_rk(&phrase).unwrap();
        assert_eq!(back.expose(), rk.expose());
    }

    #[test]
    fn normalization_accepts_messy_entry() {
        let phrase = encode(&[0u8; 32]).unwrap().join(" ");
        let messy = format!("  {}  ", phrase.replace(' ', "  \n ").to_uppercase());
        // Uppercase must normalize; extra whitespace collapses.
        assert!(decode(&messy).is_ok());
    }

    #[test]
    fn wrong_word_and_bad_checksum_rejected_without_oracle() {
        let mut words: Vec<String> = encode(&[0u8; 32])
            .unwrap()
            .iter()
            .map(|s| s.to_string())
            .collect();
        words[0] = "notaword".to_string();
        assert_eq!(
            decode(&words.join(" ")),
            Err(CryptoError::RecoveryKeyInvalid)
        );
        let mut words = words.clone();
        words[0] = "zoo".to_string(); // valid word, wrong checksum
        assert_eq!(
            decode(&words.join(" ")),
            Err(CryptoError::RecoveryKeyInvalid)
        );
        // wrong word count for RK even when checksum would pass
        let phrase12 = encode(&[0u8; 16]).unwrap().join(" ");
        assert!(matches!(
            decode_rk(&phrase12),
            Err(CryptoError::RecoveryKeyInvalid)
        ));
    }
}
