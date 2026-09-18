//! Length-prefixed JSON framing (spec §1.4): every frame is a 4-byte
//! big-endian length followed by one UTF-8 JSON object, maximum 64 KiB.
//! An oversize length header closes the connection immediately — no error
//! frame, no partial consumption (fail-closed, spec §1.4).

use std::io::{Read, Write};

pub const MAX_FRAME_BYTES: u32 = 64 * 1024;
const LEN_BYTES: usize = 4;

#[derive(Debug, PartialEq, Eq)]
pub enum FrameError {
    /// Clean or mid-frame EOF; the peer is gone.
    Eof,
    /// Length header exceeded MAX_FRAME_BYTES. Caller must close.
    Oversize,
    /// Body was not valid UTF-8 / JSON, or the JSON value is not an object.
    Malformed,
    Io(std::io::ErrorKind),
}

impl std::fmt::Display for FrameError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FrameError::Eof => write!(f, "peer closed connection"),
            FrameError::Oversize => write!(f, "frame exceeds 64 KiB limit"),
            FrameError::Malformed => write!(f, "malformed frame"),
            FrameError::Io(k) => write!(f, "io error: {k:?}"),
        }
    }
}

fn map_io(err: std::io::Error) -> FrameError {
    match err.kind() {
        std::io::ErrorKind::UnexpectedEof => FrameError::Eof,
        k => FrameError::Io(k),
    }
}

/// Read one frame. Returns the raw JSON object. Any framing violation is an
/// error; the caller closes the connection on all of them.
pub fn read_frame(stream: &mut impl Read) -> Result<serde_json::Value, FrameError> {
    let mut len_buf = [0u8; LEN_BYTES];
    stream.read_exact(&mut len_buf).map_err(map_io)?;
    let len = u32::from_be_bytes(len_buf);
    if len > MAX_FRAME_BYTES {
        return Err(FrameError::Oversize);
    }
    let mut body = vec![0u8; len as usize];
    stream.read_exact(&mut body).map_err(map_io)?;
    let value: serde_json::Value =
        serde_json::from_slice(&body).map_err(|_| FrameError::Malformed)?;
    if !value.is_object() {
        return Err(FrameError::Malformed);
    }
    Ok(value)
}

/// Serialize and write one frame. JSON objects only (spec §1.4).
pub fn write_frame(stream: &mut impl Write, value: &serde_json::Value) -> Result<(), FrameError> {
    let body = serde_json::to_vec(value).map_err(|_| FrameError::Malformed)?;
    let len = u32::try_from(body.len()).map_err(|_| FrameError::Oversize)?;
    if len > MAX_FRAME_BYTES {
        return Err(FrameError::Oversize);
    }
    stream.write_all(&len.to_be_bytes()).map_err(map_io)?;
    stream.write_all(&body).map_err(map_io)?;
    stream.flush().map_err(map_io)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::io::Cursor;

    fn framed(value: &serde_json::Value) -> Vec<u8> {
        let body = serde_json::to_vec(value).unwrap();
        let mut out = (body.len() as u32).to_be_bytes().to_vec();
        out.extend_from_slice(&body);
        out
    }

    #[test]
    fn roundtrip() {
        let value = json!({"op": "get_state"});
        let bytes = framed(&value);
        let mut cursor = Cursor::new(bytes);
        assert_eq!(read_frame(&mut cursor).unwrap(), value);
    }

    #[test]
    fn oversize_header_is_rejected_without_reading_body() {
        let mut bytes = (MAX_FRAME_BYTES + 1).to_be_bytes().to_vec();
        bytes.extend_from_slice(&[0u8; 8]); // tiny body, never read
        let mut cursor = Cursor::new(bytes);
        assert_eq!(read_frame(&mut cursor).unwrap_err(), FrameError::Oversize);
    }

    #[test]
    fn exact_limit_is_accepted() {
        let payload = "x".repeat(MAX_FRAME_BYTES as usize - 2); // quotes make it exact
        let value = json!(payload);
        let bytes = framed(&value);
        assert_eq!(bytes.len() as u32 - 4, MAX_FRAME_BYTES);
        let mut cursor = Cursor::new(bytes);
        // valid JSON but not an object → Malformed; use an object instead
        assert_eq!(read_frame(&mut cursor).unwrap_err(), FrameError::Malformed);
        let obj = json!({"pad": "x".repeat(MAX_FRAME_BYTES as usize - 10)});
        let bytes = framed(&obj);
        assert_eq!(bytes.len() as u32 - 4, MAX_FRAME_BYTES);
        let mut cursor = Cursor::new(bytes);
        assert_eq!(read_frame(&mut cursor).unwrap(), obj);
    }

    #[test]
    fn truncated_body_is_eof() {
        let mut bytes = (10u32).to_be_bytes().to_vec();
        bytes.extend_from_slice(b"{}");
        let mut cursor = Cursor::new(bytes);
        assert_eq!(read_frame(&mut cursor).unwrap_err(), FrameError::Eof);
    }

    #[test]
    fn non_object_json_is_malformed() {
        let mut bytes = (2u32).to_be_bytes().to_vec();
        bytes.extend_from_slice(b"[]");
        let mut cursor = Cursor::new(bytes);
        assert_eq!(read_frame(&mut cursor).unwrap_err(), FrameError::Malformed);
    }

    #[test]
    fn write_refuses_oversize() {
        let obj = json!({"pad": "x".repeat(MAX_FRAME_BYTES as usize)});
        let mut out = Vec::new();
        assert_eq!(
            write_frame(&mut out, &obj).unwrap_err(),
            FrameError::Oversize
        );
        assert!(out.is_empty());
    }
}
