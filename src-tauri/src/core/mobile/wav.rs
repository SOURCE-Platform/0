//! WAV files for phone audio: 16 kHz mono Int16 little-endian, the format both
//! the live clip stream and push-to-talk send.

use std::path::Path;

/// Bytes of phone PCM per second of audio.
pub const BYTES_PER_SECOND: usize = 32_000;

/// A 44-byte header for `data_len` bytes of phone PCM.
pub fn wav_header(data_len: u32) -> Vec<u8> {
    let mut header = Vec::with_capacity(44);
    header.extend_from_slice(b"RIFF");
    header.extend_from_slice(&(36u32 + data_len).to_le_bytes());
    header.extend_from_slice(b"WAVEfmt ");
    header.extend_from_slice(&16u32.to_le_bytes());
    header.extend_from_slice(&1u16.to_le_bytes());
    header.extend_from_slice(&1u16.to_le_bytes());
    header.extend_from_slice(&16000u32.to_le_bytes());
    header.extend_from_slice(&(BYTES_PER_SECOND as u32).to_le_bytes());
    header.extend_from_slice(&2u16.to_le_bytes());
    header.extend_from_slice(&16u16.to_le_bytes());
    header.extend_from_slice(b"data");
    header.extend_from_slice(&data_len.to_le_bytes());
    header
}

/// Write phone PCM as a WAV file, creating its folder if needed.
pub fn write_wav(path: &Path, pcm: &[u8]) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| format!("Failed to create audio folder: {error}"))?;
    }
    let mut bytes = wav_header(pcm.len() as u32);
    bytes.extend_from_slice(pcm);
    std::fs::write(path, bytes).map_err(|error| format!("Failed to write audio: {error}"))
}

/// Give a raw PCM spool file a WAV header, unless it already has one.
pub fn ensure_wav_header(path: &Path) -> Result<(), String> {
    let bytes = std::fs::read(path).map_err(|error| format!("Failed to read spool: {error}"))?;
    if bytes.len() >= 12 && &bytes[0..4] == b"RIFF" {
        return Ok(());
    }
    write_wav(path, &bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_sizes_match_the_audio_length() {
        let header = wav_header(96_000);
        assert_eq!(header.len(), 44);
        assert_eq!(&header[0..4], b"RIFF");
        assert_eq!(u32::from_le_bytes(header[4..8].try_into().unwrap()), 36 + 96_000);
        assert_eq!(u32::from_le_bytes(header[40..44].try_into().unwrap()), 96_000);
    }

    #[test]
    fn writes_a_wav_and_wraps_raw_pcm_only_once() {
        let dir = std::env::temp_dir().join("source_wav_test");
        let _ = std::fs::remove_dir_all(&dir);
        let written = dir.join("talk.wav");
        write_wav(&written, &[1, 0, 2, 0]).unwrap();
        assert_eq!(std::fs::read(&written).unwrap().len(), 48);

        let raw = dir.join("raw.wav");
        std::fs::write(&raw, [1u8, 0, 2, 0]).unwrap();
        ensure_wav_header(&raw).unwrap();
        ensure_wav_header(&raw).unwrap();
        assert_eq!(std::fs::read(&raw).unwrap().len(), 48, "wrapped once");
    }
}
