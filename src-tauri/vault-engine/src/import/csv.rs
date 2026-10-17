//! A streaming RFC 4180 reader (spec §10.1, §10.3; Phase I). Written here
//! rather than taken from a crate (no new dependency in the helper,
//! owner preference 2026-10-16); small enough to fuzz whole.
//!
//! - One record at a time; never the whole file in memory.
//! - Quoted fields carry commas, CR/LF and doubled quotes.
//! - **Strict** mode refuses a quote inside an unquoted field or text after
//!   a closing quote; **lenient** mode keeps such quotes literally. The
//!   mode is chosen once per file from its first 100 records (§10.3).
//! - Invalid UTF-8 refuses the record (no lossy conversion for secrets).
//! - A field over 64 KiB or a record over 256 KiB is skipped whole.

use std::io::BufRead;

pub const MAX_FIELD: usize = 64 * 1024;
pub const MAX_ROW: usize = 256 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Strict,
    Lenient,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Row {
    Fields(Vec<String>),
    /// Skipped: malformed quoting, invalid UTF-8, or oversized.
    Bad,
}

pub struct Reader<R: BufRead> {
    input: R,
    mode: Mode,
    done: bool,
}

impl<R: BufRead> Reader<R> {
    pub fn new(input: R, mode: Mode) -> Self {
        Reader { input, mode, done: false }
    }

    fn byte(&mut self) -> Option<u8> {
        let mut b = [0u8; 1];
        match self.input.read(&mut b) {
            Ok(1) => Some(b[0]),
            _ => None,
        }
    }

    /// The next record, or `None` at end of input.
    pub fn next_row(&mut self) -> Option<Row> {
        if self.done {
            return None;
        }
        let (mut fields, mut field, mut size) = (Vec::new(), Vec::new(), 0usize);
        let (mut quoted, mut in_quotes, mut after_quote, mut bad, mut any) = (false, false, false, false, false);
        loop {
            let Some(b) = self.byte() else {
                self.done = true;
                if !any {
                    return None;
                }
                if in_quotes {
                    bad = true; // an unterminated quote
                }
                break;
            };
            any = true;
            size += 1;
            if in_quotes {
                if b == b'"' {
                    // `""` is a literal quote; a lone `"` closes the field.
                    let mut peek = [0u8; 1];
                    let next = match self.input.fill_buf() {
                        Ok(buf) if !buf.is_empty() => Some(buf[0]),
                        _ => None,
                    };
                    if next == Some(b'"') {
                        let _ = self.input.read(&mut peek);
                        size += 1;
                        field.push(b'"');
                    } else {
                        in_quotes = false;
                        after_quote = true;
                    }
                } else {
                    field.push(b);
                }
            } else {
                match b {
                    b',' => {
                        fields.push(std::mem::take(&mut field));
                        (quoted, after_quote) = (false, false);
                    }
                    b'\n' => break,
                    b'\r' => {}
                    b'"' if field.is_empty() && !quoted => {
                        in_quotes = true;
                        quoted = true;
                    }
                    _ => {
                        if (after_quote || b == b'"') && self.mode == Mode::Strict {
                            bad = true;
                        }
                        field.push(b);
                    }
                }
            }
            if field.len() > MAX_FIELD || size > MAX_ROW {
                bad = true;
            }
            if bad && size > MAX_ROW {
                // Drain the rest of this physical line, then skip.
                while let Some(b) = self.byte() {
                    if b == b'\n' && !in_quotes {
                        break;
                    }
                }
                return Some(Row::Bad);
            }
        }
        fields.push(field);
        if bad {
            return Some(Row::Bad);
        }
        let mut out = Vec::with_capacity(fields.len());
        for f in fields {
            match String::from_utf8(f) {
                Ok(s) => out.push(s),
                Err(_) => return Some(Row::Bad),
            }
        }
        Some(Row::Fields(out))
    }
}

/// §10.3: strict if the first 100 records parse strictly, else lenient.
pub fn choose_mode(sample: impl BufRead) -> Mode {
    let mut r = Reader::new(sample, Mode::Strict);
    for _ in 0..100 {
        match r.next_row() {
            Some(Row::Bad) => return Mode::Lenient,
            Some(Row::Fields(_)) => {}
            None => break,
        }
    }
    Mode::Strict
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows(s: &str, mode: Mode) -> Vec<Row> {
        let mut r = Reader::new(std::io::BufReader::new(s.as_bytes()), mode);
        std::iter::from_fn(|| r.next_row()).collect()
    }

    fn f(v: &[&str]) -> Row {
        Row::Fields(v.iter().map(|s| s.to_string()).collect())
    }

    #[test]
    fn rfc4180_cases() {
        assert_eq!(rows("a,b\r\nc,d\n", Mode::Strict), vec![f(&["a", "b"]), f(&["c", "d"])]);
        assert_eq!(rows("\"a,1\",\"line\nbreak\",\"q\"\"uote\"\n", Mode::Strict), vec![f(&["a,1", "line\nbreak", "q\"uote"])]);
        assert_eq!(rows("a,,c", Mode::Strict), vec![f(&["a", "", "c"])]);
        assert_eq!(rows("", Mode::Strict), vec![]);
    }

    #[test]
    fn strict_refuses_and_lenient_keeps_stray_quotes() {
        assert_eq!(rows("ab\"c,d\n", Mode::Strict), vec![Row::Bad]);
        assert_eq!(rows("ab\"c,d\n", Mode::Lenient), vec![f(&["ab\"c", "d"])]);
        assert_eq!(rows("\"unterminated,x\n", Mode::Strict), vec![Row::Bad]);
        assert_eq!(choose_mode(&b"a,b\nab\"c,d\n"[..]), Mode::Lenient);
    }

    #[test]
    fn invalid_utf8_and_oversized_rows_are_skipped() {
        let mut bad = b"ok,1\n".to_vec();
        bad.extend_from_slice(b"x\xff,2\nfine,3\n");
        let mut r = Reader::new(std::io::BufReader::new(&bad[..]), Mode::Strict);
        let got: Vec<Row> = std::iter::from_fn(|| r.next_row()).collect();
        assert_eq!(got, vec![f(&["ok", "1"]), Row::Bad, f(&["fine", "3"])]);
        let big = format!("{},x\nnext,y\n", "a".repeat(MAX_FIELD + 10));
        let got = rows(&big, Mode::Strict);
        assert_eq!(got[0], Row::Bad);
        assert_eq!(got.last().unwrap(), &f(&["next", "y"]));
    }
}
