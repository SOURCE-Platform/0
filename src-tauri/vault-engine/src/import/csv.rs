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
//! - A field over 64 KiB or a record over 256 KiB is skipped whole; the
//!   record is still read to its real end, quotes tracked, so the rows
//!   after it stay aligned.
//! - A read error is `Row::Failed`, never end of input.

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
    /// The input could not be read (a damaged file or ZIP member). Never
    /// treated as end of input (final review VER-B2); the reader stops.
    Failed,
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

    /// `Ok(None)` at end of input; `Err` when the input cannot be read.
    fn byte(&mut self) -> Result<Option<u8>, ()> {
        let mut b = [0u8; 1];
        loop {
            match self.input.read(&mut b) {
                Ok(0) => return Ok(None),
                Ok(_) => return Ok(Some(b[0])),
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(_) => return Err(()),
            }
        }
    }

    fn peek(&mut self) -> Result<Option<u8>, ()> {
        match self.input.fill_buf() {
            Ok(buf) => Ok(buf.first().copied()),
            Err(_) => Err(()),
        }
    }

    /// The next record, or `None` at end of input. An oversized record is
    /// still parsed to its real end (quotes tracked), only not kept.
    pub fn next_row(&mut self) -> Option<Row> {
        if self.done {
            return None;
        }
        let (mut fields, mut field, mut size) = (Vec::new(), Vec::new(), 0usize);
        let (mut quoted, mut in_quotes, mut after_quote, mut bad, mut any) = (false, false, false, false, false);
        loop {
            let b = match self.byte() {
                Ok(Some(b)) => b,
                Ok(None) => {
                    self.done = true;
                    if !any {
                        return None;
                    }
                    bad |= in_quotes; // an unterminated quote
                    break;
                }
                Err(()) => {
                    self.done = true;
                    return Some(Row::Failed);
                }
            };
            any = true;
            size += 1;
            if size > MAX_ROW {
                bad = true; // keep reading to the record's end, keep nothing
            }
            let keep = !bad;
            if in_quotes {
                if b == b'"' {
                    // `""` is a literal quote; a lone `"` closes the field.
                    match self.peek() {
                        Ok(Some(b'"')) => {
                            let _ = self.byte();
                            size += 1;
                            if keep {
                                field.push(b'"');
                            }
                        }
                        Ok(_) => {
                            in_quotes = false;
                            after_quote = true;
                        }
                        Err(()) => {
                            self.done = true;
                            return Some(Row::Failed);
                        }
                    }
                } else if keep {
                    field.push(b);
                }
            } else {
                match b {
                    b',' => {
                        if keep {
                            fields.push(std::mem::take(&mut field));
                        }
                        (quoted, after_quote) = (false, false);
                    }
                    b'\n' => break,
                    b'\r' => {}
                    b'"' if field.is_empty() && !quoted && !after_quote => {
                        in_quotes = true;
                        quoted = true;
                    }
                    _ => {
                        if (after_quote || b == b'"') && self.mode == Mode::Strict {
                            bad = true;
                        }
                        if keep {
                            field.push(b);
                        }
                    }
                }
            }
            if field.len() > MAX_FIELD {
                bad = true;
                field.clear();
            }
        }
        if bad {
            return Some(Row::Bad);
        }
        fields.push(field);
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
            Some(Row::Failed) | None => break,
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

    /// Final review VER-B2: an oversized quoted record with newlines inside
    /// skips exactly itself; the rows after it are kept and aligned.
    #[test]
    fn an_oversized_quoted_record_skips_only_itself() {
        let big = format!("h1,h2\n\"{}\nstill inside\n{}\",x\nr1,a\nr2,b\n", "a".repeat(MAX_ROW), "b".repeat(10));
        let got = rows(&big, Mode::Strict);
        assert_eq!(got, vec![f(&["h1", "h2"]), Row::Bad, f(&["r1", "a"]), f(&["r2", "b"])]);
        let unquoted = format!("{},\"b\nc\"\nr1,a\n", "a".repeat(MAX_FIELD + 1));
        assert_eq!(rows(&unquoted, Mode::Strict), vec![Row::Bad, f(&["r1", "a"])]);
    }

    /// A read error is reported, never taken for the end of the file.
    #[test]
    fn a_read_error_is_not_end_of_input() {
        struct Broken(usize);
        impl std::io::Read for Broken {
            fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
                let data = b"a,b\nr2,";
                if self.0 >= data.len() {
                    return Err(std::io::Error::other("damaged"));
                }
                buf[0] = data[self.0];
                self.0 += 1;
                Ok(1)
            }
        }
        let mut r = Reader::new(std::io::BufReader::new(Broken(0)), Mode::Strict);
        assert_eq!(r.next_row(), Some(f(&["a", "b"])));
        assert_eq!(r.next_row(), Some(Row::Failed));
        assert_eq!(r.next_row(), None);
    }
}
