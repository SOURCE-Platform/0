//! The Dashlane ZIP export (spec §10.1; Phase I): a member list from the
//! central directory, and one member at a time as a decompressing stream.
//! Nothing is extracted to disk; nothing is held whole in memory.
//!
//! Refused (the whole file): no end-of-central-directory record, ZIP64,
//! more than 64 members, a central directory that does not parse.
//! Refused (one member): encryption, a method other than stored/deflate,
//! a declared size over 1 GiB. A member's output is cut at its declared
//! size, so a lying header cannot inflate past it (a "zip bomb" ends
//! there). The CRC is not checked: rows are imported as they stream, so
//! a check at the end could not undo them; a damaged deflate stream ends
//! the member with a read error, counted by the caller.

use std::fs::File;
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};
use flate2::read::DeflateDecoder;

const EOCD_SIG: u32 = 0x0605_4b50;
const CEN_SIG: u32 = 0x0201_4b50;
const LOC_SIG: u32 = 0x0403_4b50;
pub const MAX_MEMBERS: usize = 64;
pub const MAX_MEMBER: u64 = 1 << 30;

#[derive(Debug, Clone)]
pub struct Member {
    /// The file name, lower-cased, without any folder part.
    pub name: String,
    method: u16,
    encrypted: bool,
    compressed: u64,
    size: u64,
    local_offset: u64,
}

fn u16_at(b: &[u8], i: usize) -> Option<u16> {
    Some(u16::from_le_bytes(b.get(i..i + 2)?.try_into().ok()?))
}

fn u32_at(b: &[u8], i: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(i..i + 4)?.try_into().ok()?))
}

/// A second handle on the one opened file (the caller opened it once,
/// without following links), rewound.
pub fn rewind(f: &File) -> Option<File> {
    let mut g = f.try_clone().ok()?;
    g.seek(SeekFrom::Start(0)).ok()?;
    Some(g)
}

/// Does this file start like a ZIP (local header signature)?
pub fn looks_like_zip(f: &File) -> bool {
    let mut sig = [0u8; 4];
    rewind(f).is_some_and(|mut g| g.read_exact(&mut sig).is_ok()) && u32::from_le_bytes(sig) == LOC_SIG
}

/// The member list from the central directory.
pub fn members(f: &File) -> Option<Vec<Member>> {
    let mut f = rewind(f)?;
    let len = f.seek(SeekFrom::End(0)).ok()?;
    let tail_len = len.min(22 + 65_535);
    f.seek(SeekFrom::Start(len - tail_len)).ok()?;
    let mut tail = vec![0u8; tail_len as usize];
    f.read_exact(&mut tail).ok()?;
    let at = (0..tail.len().saturating_sub(21)).rev().find(|&i| u32_at(&tail, i) == Some(EOCD_SIG))?;
    let count = u16_at(&tail, at + 10)? as usize;
    let cd_size = u32_at(&tail, at + 12)?;
    let cd_off = u32_at(&tail, at + 16)?;
    if count == 0xFFFF || cd_size == u32::MAX || cd_off == u32::MAX || count > MAX_MEMBERS {
        return None; // ZIP64, or too many members
    }
    if cd_off as u64 + cd_size as u64 > len || cd_size > 1 << 20 {
        return None;
    }
    f.seek(SeekFrom::Start(cd_off as u64)).ok()?;
    let mut cd = vec![0u8; cd_size as usize];
    f.read_exact(&mut cd).ok()?;
    let (mut out, mut i) = (Vec::with_capacity(count), 0usize);
    for _ in 0..count {
        if u32_at(&cd, i)? != CEN_SIG {
            return None;
        }
        let flags = u16_at(&cd, i + 8)?;
        let method = u16_at(&cd, i + 10)?;
        let compressed = u32_at(&cd, i + 20)?;
        let size = u32_at(&cd, i + 24)?;
        let (n, x, c) = (u16_at(&cd, i + 28)? as usize, u16_at(&cd, i + 30)? as usize, u16_at(&cd, i + 32)? as usize);
        let local_offset = u32_at(&cd, i + 42)?;
        if [compressed, size, local_offset].contains(&u32::MAX) {
            return None;
        }
        let raw = cd.get(i + 46..i + 46 + n)?;
        let full = String::from_utf8_lossy(raw).to_ascii_lowercase();
        let name = full.rsplit(['/', '\\']).next().unwrap_or("").to_string();
        out.push(Member {
            name,
            method,
            encrypted: flags & 1 != 0,
            compressed: compressed as u64,
            size: size as u64,
            local_offset: local_offset as u64,
        });
        i += 46 + n + x + c;
    }
    Some(out)
}

/// One member as a decompressing stream, or `None` when it is refused.
pub fn open_member(f: &File, m: &Member) -> Option<Box<dyn BufRead>> {
    if m.encrypted || m.size > MAX_MEMBER || !(m.method == 0 || m.method == 8) {
        return None;
    }
    let mut f = rewind(f)?;
    f.seek(SeekFrom::Start(m.local_offset)).ok()?;
    let mut h = [0u8; 30];
    f.read_exact(&mut h).ok()?;
    if u32_at(&h, 0)? != LOC_SIG {
        return None;
    }
    let skip = u16_at(&h, 26)? as i64 + u16_at(&h, 28)? as i64;
    f.seek(SeekFrom::Current(skip)).ok()?;
    let raw = f.take(m.compressed);
    let body: Box<dyn Read> = if m.method == 0 { Box::new(raw) } else { Box::new(DeflateDecoder::new(raw)) };
    Some(Box::new(BufReader::new(body.take(m.size))))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::io::Write;
    use std::path::Path;

    /// A minimal ZIP writer for tests and fixtures (deflate members).
    pub(crate) fn write_zip(path: &Path, members: &[(&str, &[u8])]) {
        use flate2::{write::DeflateEncoder, Compression};
        let (mut out, mut cd) = (Vec::new(), Vec::new());
        for (name, data) in members {
            let mut enc = DeflateEncoder::new(Vec::new(), Compression::default());
            enc.write_all(data).unwrap();
            let comp = enc.finish().unwrap();
            let off = out.len() as u32;
            let mut lh = Vec::new();
            lh.extend_from_slice(&LOC_SIG.to_le_bytes());
            lh.extend_from_slice(&[20, 0, 0, 0, 8, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
            lh.extend_from_slice(&(comp.len() as u32).to_le_bytes());
            lh.extend_from_slice(&(data.len() as u32).to_le_bytes());
            lh.extend_from_slice(&(name.len() as u16).to_le_bytes());
            lh.extend_from_slice(&0u16.to_le_bytes());
            out.extend_from_slice(&lh);
            out.extend_from_slice(name.as_bytes());
            out.extend_from_slice(&comp);
            cd.extend_from_slice(&CEN_SIG.to_le_bytes());
            cd.extend_from_slice(&[20, 0, 20, 0, 0, 0, 8, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
            cd.extend_from_slice(&(comp.len() as u32).to_le_bytes());
            cd.extend_from_slice(&(data.len() as u32).to_le_bytes());
            cd.extend_from_slice(&(name.len() as u16).to_le_bytes());
            cd.extend_from_slice(&[0; 12]);
            cd.extend_from_slice(&off.to_le_bytes());
            cd.extend_from_slice(name.as_bytes());
        }
        let cd_off = out.len() as u32;
        out.extend_from_slice(&cd);
        out.extend_from_slice(&EOCD_SIG.to_le_bytes());
        out.extend_from_slice(&[0, 0, 0, 0]);
        out.extend_from_slice(&(members.len() as u16).to_le_bytes());
        out.extend_from_slice(&(members.len() as u16).to_le_bytes());
        out.extend_from_slice(&(cd.len() as u32).to_le_bytes());
        out.extend_from_slice(&cd_off.to_le_bytes());
        out.extend_from_slice(&[0, 0]);
        std::fs::write(path, out).unwrap();
    }

    #[test]
    fn members_stream_and_refusals() {
        let dir = std::env::temp_dir().join(format!("ov0-zip-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("export.zip");
        write_zip(&p, &[("Dashlane/credentials.csv", b"title,password\nx,fixture-1\n"), ("ids.csv", b"a\n")]);
        let f = File::open(&p).unwrap();
        assert!(looks_like_zip(&f));
        let ms = members(&f).unwrap();
        assert_eq!(ms.iter().map(|m| m.name.as_str()).collect::<Vec<_>>(), ["credentials.csv", "ids.csv"]);
        let mut s = String::new();
        open_member(&f, &ms[0]).unwrap().read_to_string(&mut s).unwrap();
        assert_eq!(s, "title,password\nx,fixture-1\n");
        let mut lying = ms[0].clone();
        lying.size = 5;
        s.clear();
        open_member(&f, &lying).unwrap().read_to_string(&mut s).unwrap();
        assert_eq!(s, "title", "output stops at the declared size");
        lying.encrypted = true;
        assert!(open_member(&f, &lying).is_none());
        std::fs::write(&p, b"PK\x03\x04 not really").unwrap();
        assert!(members(&File::open(&p).unwrap()).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
