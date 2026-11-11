//! Python text semantics the oracle relies on.
//!
//! A Python `str` is a sequence of code points that may hold lone surrogates (JSON `\ud800`
//! escapes produce them), so it is modelled as `PyStr = Vec<u32>`, not `String`.
//!
//! The historical reference used CPython 3.14 in UTF-8 mode: files were UTF-8/strict,
//! stdout used UTF-8/surrogateescape, and argv used surrogateescape so printed paths
//! retained their original bytes.

/// A Python `str`: code points, lone surrogates allowed.
pub type PyStr = Vec<u32>;

/// `PyStr` from Rust text.
pub fn py(s: &str) -> PyStr {
    s.chars().map(u32::from).collect()
}

/// `open(src, encoding="utf-8", errors="replace")` iterated line by line, streaming as the oracle
/// does: only the current line is held in memory (as raw bytes, then as chars), never the file.
///
/// errors="replace" emits one U+FFFD per maximal invalid subpart, as `from_utf8_lossy` does.
/// Lines are split on raw bytes. '\n' and '\r' never occur inside a UTF-8 sequence and they end
/// every invalid subpart, so decoding each line separately gives the same result as decoding
/// the whole file. Text mode reads with universal newlines: "\r\n", "\r" and "\n" all end a line
/// and come back as "\n" ("\r\n" also when a buffer boundary splits it). A final line without a
/// terminator is still a line; an empty file has no lines. A read error ends the iteration
/// with `Err` (in the oracle an OSError escapes before anything is printed).
pub fn lines<R: std::io::BufRead>(reader: R) -> Lines<R> {
    Lines { reader, buf: Vec::new(), failed: false }
}

/// Iterator returned by [`lines`].
pub struct Lines<R> {
    reader: R,
    buf: Vec<u8>,
    failed: bool,
}

impl<R: std::io::BufRead> Lines<R> {
    /// Read the next line's bytes (terminator excluded) into `buf`: Ok(Some(terminated)), or
    /// Ok(None) at the end of the file.
    fn next_raw(&mut self) -> std::io::Result<Option<bool>> {
        self.buf.clear();
        loop {
            let chunk = match self.reader.fill_buf() {
                Ok(c) => c,
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(e),
            };
            if chunk.is_empty() {
                return Ok((!self.buf.is_empty()).then_some(false));
            }
            match chunk.iter().position(|&b| b == b'\n' || b == b'\r') {
                None => {
                    let n = chunk.len();
                    self.buf.extend_from_slice(chunk);
                    self.reader.consume(n);
                }
                Some(i) => {
                    let cr = chunk[i] == b'\r';
                    self.buf.extend_from_slice(&chunk[..i]);
                    self.reader.consume(i + 1);
                    if cr {
                        self.skip_lf()?;
                    }
                    return Ok(Some(true));
                }
            }
        }
    }

    /// After a '\r': a '\n' right after it belongs to the same line end.
    fn skip_lf(&mut self) -> std::io::Result<()> {
        loop {
            match self.reader.fill_buf() {
                Ok(c) => {
                    if c.first() == Some(&b'\n') {
                        self.reader.consume(1);
                    }
                    return Ok(());
                }
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(e),
            }
        }
    }
}

impl<R: std::io::BufRead> Iterator for Lines<R> {
    type Item = std::io::Result<Vec<char>>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.failed {
            return None;
        }
        match self.next_raw() {
            Ok(None) => None,
            Ok(Some(terminated)) => {
                let mut line: Vec<char> = String::from_utf8_lossy(&self.buf).chars().collect();
                if terminated {
                    line.push('\n');
                }
                Some(Ok(line))
            }
            Err(e) => {
                self.failed = true;
                Some(Err(e))
            }
        }
    }
}

/// `str.isspace()`: Unicode White_Space plus U+001C..U+001F (bidi class B/S in CPython's table).
pub fn is_py_space(c: char) -> bool {
    c.is_whitespace() || ('\u{1c}'..='\u{1f}').contains(&c)
}

/// `str.strip()` with no argument.
pub fn py_strip(line: &[char]) -> &[char] {
    let start = line.iter().position(|&c| !is_py_space(c)).unwrap_or(line.len());
    let end = line.iter().rposition(|&c| !is_py_space(c)).map_or(start, |i| i + 1);
    &line[start..end]
}

fn push_utf8(cp: u32, out: &mut Vec<u8>) {
    // callers pass only non-surrogate code points <= 0x10FFFF
    let c = char::from_u32(cp).expect("non-surrogate code point");
    let mut buf = [0u8; 4];
    out.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
}

/// stdout encoding: UTF-8 with errors="surrogateescape". U+DC80..U+DCFF become the raw bytes
/// 0x80..0xFF; any other surrogate is unencodable and Python raises UnicodeEncodeError for the
/// whole `write()` (nothing of it reaches the buffer). Err carries the offending index.
pub fn encode_surrogateescape(s: &[u32]) -> Result<Vec<u8>, usize> {
    let mut out = Vec::with_capacity(s.len());
    for (i, &cp) in s.iter().enumerate() {
        match cp {
            0xDC80..=0xDCFF => out.push((cp - 0xDC00) as u8),
            0xD800..=0xDFFF => return Err(i),
            _ => push_utf8(cp, &mut out),
        }
    }
    Ok(out)
}

/// `open(dst, "w").write(answer)`: UTF-8 strict, so any surrogate raises. Newline translation
/// is "\n" -> os.linesep, a no-op on Linux. Err carries the offending index.
pub fn encode_strict(s: &[u32]) -> Result<Vec<u8>, usize> {
    let mut out = Vec::with_capacity(s.len());
    for (i, &cp) in s.iter().enumerate() {
        if (0xD800..=0xDFFF).contains(&cp) {
            return Err(i);
        }
        push_utf8(cp, &mut out);
    }
    Ok(out)
}

/// Python-style hex escape of a code point, e.g. `\ud800`, for error messages.
pub fn escape_cp(cp: u32) -> String {
    format!("\\u{cp:04x}")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Lines read through a BufReader of `cap` bytes.
    fn split_with(b: &[u8], cap: usize) -> Vec<String> {
        lines(std::io::BufReader::with_capacity(cap, b))
            .map(|l| l.expect("in-memory read").into_iter().collect())
            .collect()
    }

    fn split(b: &[u8]) -> Vec<String> {
        split_with(b, 1 << 16)
    }

    /// Serves `data`, then fails every read: anything yielded before the error was produced
    /// without reading past it.
    struct FailAfter {
        data: Vec<u8>,
        pos: usize,
    }

    impl std::io::Read for FailAfter {
        fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
            if self.pos == self.data.len() {
                return Err(std::io::Error::other("read past the served bytes"));
            }
            let n = out.len().min(self.data.len() - self.pos);
            out[..n].copy_from_slice(&self.data[self.pos..self.pos + n]);
            self.pos += n;
            Ok(n)
        }
    }

    #[test]
    fn streams_one_line_at_a_time() {
        // Streaming regression: lines come out while the rest of the file is still unread
        let mut it = lines(std::io::BufReader::with_capacity(4, FailAfter { data: b"ab\ncd\r\nef".to_vec(), pos: 0 }));
        let mut next = || it.next().map(|r| r.map(|l| l.into_iter().collect::<String>()).map_err(|e| e.to_string()));
        assert_eq!(next(), Some(Ok("ab\n".to_string())));
        assert_eq!(next(), Some(Ok("cd\n".to_string())));
        // "ef" has no terminator, so EOF must be read: here the reader fails instead
        assert_eq!(next(), Some(Err("read past the served bytes".to_string())));
        assert_eq!(next(), None);
    }

    #[test]
    fn buffer_boundaries_do_not_matter() {
        // a buffer boundary may split "\r\n", a UTF-8 sequence or an invalid subpart
        let input = "x\r\n\u{e9}\u{1f600}\r\r\n\u{3000}{}\n".bytes().chain(*b"\xe2\x82\r\xf0\x9f\x98\n\xed\xa0\x80z").collect::<Vec<u8>>();
        let whole = split(&input);
        assert_eq!(whole, ["x\n", "\u{e9}\u{1f600}\n", "\n", "\u{3000}{}\n", "\u{fffd}\n", "\u{fffd}\n", "\u{fffd}\u{fffd}\u{fffd}z"]);
        for cap in 1..=9 {
            assert_eq!(split_with(&input, cap), whole, "capacity {cap}");
        }
    }

    #[test]
    fn universal_newlines() {
        assert_eq!(split(b"a\r\nb\rc\nd"), ["a\n", "b\n", "c\n", "d"]);
        assert_eq!(split(b"a\n\nb\n"), ["a\n", "\n", "b\n"]);
        assert_eq!(split(b"\r\r\n"), ["\n", "\n"]);
        assert!(split(b"").is_empty());
        // only \n and \r split; other line breaks stay inside the line
        assert_eq!(split("a\u{2028}b\x0bc\x0cd\u{85}e".as_bytes()).len(), 1);
    }

    #[test]
    fn lossy_decode_maximal_subparts() {
        // truncated 3-byte sequence = 1 U+FFFD; encoded surrogate ED A0 80 = 3; lone continuation = 1
        assert_eq!(split(b"\xe2\x82x"), ["\u{fffd}x"]);
        assert_eq!(split(b"\xed\xa0\x80"), ["\u{fffd}\u{fffd}\u{fffd}"]);
        assert_eq!(split(b"\x80\n"), ["\u{fffd}\n"]);
        assert_eq!(split(b"\xf0\x9f\x98"), ["\u{fffd}"]);
        // the BOM is kept (encoding="utf-8", not utf-8-sig)
        assert_eq!(split(b"\xef\xbb\xbf{}"), ["\u{feff}{}"]);
    }

    #[test]
    fn strip_matches_str_isspace() {
        let l: Vec<char> = "\x1c\x1d\x1e\x1f \t\u{a0}\u{3000}{}\u{2029}\u{85}\x0b\n".chars().collect();
        assert_eq!(py_strip(&l), ['{', '}']);
        // U+200B (Cf) and U+FEFF are not whitespace in Python
        let l: Vec<char> = "\u{200b}x\u{feff}".chars().collect();
        assert_eq!(py_strip(&l).len(), 3);
        let l: Vec<char> = " \t ".chars().collect();
        assert!(py_strip(&l).is_empty());
    }

    #[test]
    fn surrogateescape_and_strict() {
        assert_eq!(encode_surrogateescape(&[0x41, 0xDC80, 0xDCFF, 0xE9]).unwrap(), b"A\x80\xff\xc3\xa9");
        assert_eq!(encode_surrogateescape(&[0x41, 0xDC7F]), Err(1));
        assert_eq!(encode_surrogateescape(&[0xD800]), Err(0));
        assert_eq!(encode_strict(&[0x41, 0x1F600]).unwrap(), "A\u{1f600}".as_bytes());
        assert_eq!(encode_strict(&[0x41, 0xDC80]), Err(1));
    }
}
