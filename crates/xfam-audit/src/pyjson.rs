//! CPython 3.14 `json.loads` / `json.dumps` / `repr` / `str` on the values JSON can produce.
//!
//! loads (C scanner, strict=True): JSON whitespace is only space, \t, \n, \r; NaN, Infinity and
//! -Infinity are accepted; control characters below U+0020 inside strings are rejected; `\uXXXX`
//! high+low surrogate escapes join, any other surrogate stays a lone code point; duplicate keys
//! keep the first key's position and the last value (`PyDict_SetItem`); ints are arbitrary
//! precision but `int()` refuses more than 4300 digits (sys.int_info.default_max_str_digits),
//! which makes the whole `json.loads` raise ValueError. Nesting deeper than `MAX_DEPTH` is
//! reported as `TooDeep`. CPython 3.14 raises RecursionError (not caught by the oracle, so
//! exit 1) only when the C stack of the 8 MiB main thread runs out. That depth cannot be
//! measured here because Python is not run on this host. `MAX_DEPTH` is a guess set below it.
//! Up to `MAX_DEPTH` the port matches the oracle: a deep valid line parses and a deep malformed
//! line is only unparsed. Beyond it the port fails loudly instead of guessing. repr() and
//! json.dumps() of such values recurse in CPython too, with the same kind of stack limit.
//!
//! dumps (default encoder): ensure_ascii, separators ", " / ": ", insertion order, float repr,
//! NaN/Infinity/-Infinity for non-finite floats, lowercase `\uXXXX`, astral code points as
//! surrogate pairs.

use crate::pytext::{py, PyStr};
use std::collections::HashMap;

/// `int()` limit of CPython >= 3.11 (`sys.int_info.default_max_str_digits`).
pub const INT_MAX_STR_DIGITS: usize = 4300;
/// Container nesting the port accepts; deeper input is `LoadError::TooDeep`. Callers must
/// give `loads`/`repr`/`dumps` a stack for this depth (src/main.rs: 256 MiB worker thread).
/// CPython 3.14 spends about 0.2-0.8 KiB of C stack per JSON level (an estimate, not a
/// measurement), so 10 000 levels should be below its 8 MiB limit.
pub const MAX_DEPTH: usize = 10_000;
/// Stack for a thread that parses, prints and drops values nested `MAX_DEPTH` deep (debug-build
/// frames included). Only the pages actually touched are committed.
pub const STACK_BYTES: usize = 256 << 20;

/// A Python object produced by `json.loads`.
#[derive(Debug, Clone)]
pub enum Val {
    None,
    Bool(bool),
    /// canonical decimal text (`-` only for non-zero values), as `repr(int)` prints it
    Int(String),
    Float(f64),
    Str(PyStr),
    List(Vec<Val>),
    Dict(Dict),
}

/// A Python dict with str keys: insertion order, re-insert keeps the original position.
#[derive(Debug, Clone, Default)]
pub struct Dict {
    items: Vec<(PyStr, Val)>,
    index: HashMap<PyStr, usize>,
}

impl Dict {
    /// `d[k] = v`.
    pub fn insert(&mut self, k: PyStr, v: Val) {
        match self.index.get(&k) {
            Some(&i) => self.items[i].1 = v,
            None => {
                self.index.insert(k.clone(), self.items.len());
                self.items.push((k, v));
            }
        }
    }

    /// `d.get(k)` for an ASCII/BMP key given as Rust text.
    pub fn get(&self, k: &str) -> Option<&Val> {
        self.index.get(py(k).as_slice()).map(|&i| &self.items[i].1)
    }

    pub fn items(&self) -> &[(PyStr, Val)] {
        &self.items
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

impl Val {
    /// `bool(v)`.
    pub fn truthy(&self) -> bool {
        match self {
            Val::None => false,
            Val::Bool(b) => *b,
            Val::Int(d) => d != "0",
            Val::Float(x) => *x != 0.0, // NaN is truthy
            Val::Str(s) => !s.is_empty(),
            Val::List(l) => !l.is_empty(),
            Val::Dict(d) => !d.is_empty(),
        }
    }

    /// `type(v).__name__`.
    pub fn type_name(&self) -> &'static str {
        match self {
            Val::None => "NoneType",
            Val::Bool(_) => "bool",
            Val::Int(_) => "int",
            Val::Float(_) => "float",
            Val::Str(_) => "str",
            Val::List(_) => "list",
            Val::Dict(_) => "dict",
        }
    }
}

/// Why `json.loads` did not return a value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoadError {
    /// ValueError (JSONDecodeError or the int digit limit): the oracle counts an unparsed line.
    Value,
    /// Deeper than `MAX_DEPTH`: CPython raises RecursionError at some larger, unmeasured depth.
    TooDeep,
}

/// `json.loads(s)` on an already-decoded str.
pub fn loads(s: &[char]) -> Result<Val, LoadError> {
    let mut p = Parser { s, i: 0 };
    p.ws();
    let v = p.value(0)?;
    p.ws();
    if p.i == s.len() {
        Ok(v)
    } else {
        Err(LoadError::Value) // "Extra data"
    }
}

struct Parser<'a> {
    s: &'a [char],
    i: usize,
}

impl Parser<'_> {
    fn peek(&self) -> Option<char> {
        self.s.get(self.i).copied()
    }

    fn ws(&mut self) {
        while matches!(self.peek(), Some(' ' | '\t' | '\n' | '\r')) {
            self.i += 1;
        }
    }

    fn lit(&mut self, word: &str) -> bool {
        let n = word.len(); // ASCII literals only
        let ok = self
            .s
            .get(self.i..self.i + n)
            .is_some_and(|w| w.iter().copied().eq(word.chars()));
        if ok {
            self.i += n;
        }
        ok
    }

    fn value(&mut self, depth: usize) -> Result<Val, LoadError> {
        match self.peek() {
            Some('"') => {
                self.i += 1;
                self.string().map(Val::Str)
            }
            Some('{') => {
                if depth >= MAX_DEPTH {
                    return Err(LoadError::TooDeep);
                }
                self.i += 1;
                self.object(depth + 1)
            }
            Some('[') => {
                if depth >= MAX_DEPTH {
                    return Err(LoadError::TooDeep);
                }
                self.i += 1;
                self.array(depth + 1)
            }
            Some('n') if self.lit("null") => Ok(Val::None),
            Some('t') if self.lit("true") => Ok(Val::Bool(true)),
            Some('f') if self.lit("false") => Ok(Val::Bool(false)),
            Some('N') if self.lit("NaN") => Ok(Val::Float(f64::NAN)),
            Some('I') if self.lit("Infinity") => Ok(Val::Float(f64::INFINITY)),
            Some('-') if self.lit("-Infinity") => Ok(Val::Float(f64::NEG_INFINITY)),
            _ => self.number(),
        }
    }

    fn object(&mut self, depth: usize) -> Result<Val, LoadError> {
        let mut d = Dict::default();
        self.ws();
        if self.peek() == Some('}') {
            self.i += 1;
            return Ok(Val::Dict(d));
        }
        loop {
            if self.peek() != Some('"') {
                return Err(LoadError::Value); // "Expecting property name enclosed in double quotes"
            }
            self.i += 1;
            let k = self.string()?;
            self.ws();
            if self.peek() != Some(':') {
                return Err(LoadError::Value);
            }
            self.i += 1;
            self.ws();
            let v = self.value(depth)?;
            d.insert(k, v);
            self.ws();
            match self.peek() {
                Some('}') => {
                    self.i += 1;
                    return Ok(Val::Dict(d));
                }
                Some(',') => {
                    self.i += 1;
                    self.ws();
                }
                _ => return Err(LoadError::Value),
            }
        }
    }

    fn array(&mut self, depth: usize) -> Result<Val, LoadError> {
        let mut items = Vec::new();
        self.ws();
        if self.peek() == Some(']') {
            self.i += 1;
            return Ok(Val::List(items));
        }
        loop {
            items.push(self.value(depth)?);
            self.ws();
            match self.peek() {
                Some(']') => {
                    self.i += 1;
                    return Ok(Val::List(items));
                }
                Some(',') => {
                    self.i += 1;
                    self.ws();
                }
                _ => return Err(LoadError::Value),
            }
        }
    }

    /// `_match_number_unicode`: -?(0|[1-9][0-9]*)(\.[0-9]+)?([eE][-+]?[0-9]+)? where a '.' or
    /// exponent without digits is left unconsumed (and then always fails as extra data).
    fn number(&mut self) -> Result<Val, LoadError> {
        let s = self.s;
        let digit = |j: usize| s.get(j).is_some_and(char::is_ascii_digit);
        let start = self.i;
        let mut j = start;
        if s.get(j) == Some(&'-') {
            j += 1;
        }
        match s.get(j) {
            Some('1'..='9') => {
                j += 1;
                while digit(j) {
                    j += 1;
                }
            }
            Some('0') => j += 1,
            _ => return Err(LoadError::Value), // "Expecting value"
        }
        let mut is_float = false;
        if s.get(j) == Some(&'.') && digit(j + 1) {
            is_float = true;
            j += 2;
            while digit(j) {
                j += 1;
            }
        }
        if matches!(s.get(j), Some('e' | 'E')) {
            let mut k = j + 1;
            if matches!(s.get(k), Some('+' | '-')) {
                k += 1;
            }
            if digit(k) {
                while digit(k) {
                    k += 1;
                }
                is_float = true;
                j = k;
            }
        }
        self.i = j;
        let text: String = s[start..j].iter().collect();
        if is_float {
            // float(): correctly rounded, overflow -> inf, underflow -> 0.0 (as Rust's parser)
            return text.parse::<f64>().map(Val::Float).map_err(|_| LoadError::Value);
        }
        let digits = text.strip_prefix('-').unwrap_or(&text);
        if digits.len() > INT_MAX_STR_DIGITS {
            return Err(LoadError::Value); // "Exceeds the limit (4300 digits) for integer string conversion"
        }
        Ok(Val::Int(if digits == "0" { "0".to_string() } else { text }))
    }

    fn hex4(&self, at: usize) -> Option<u32> {
        self.s
            .get(at..at + 4)?
            .iter()
            .try_fold(0u32, |acc, c| c.to_digit(16).map(|d| acc * 16 + d))
    }

    /// `scanstring` after the opening quote.
    fn string(&mut self) -> Result<PyStr, LoadError> {
        let mut out = Vec::new();
        loop {
            let c = self.peek().ok_or(LoadError::Value)?; // "Unterminated string"
            self.i += 1;
            match c {
                '"' => return Ok(out),
                '\\' => {
                    let e = self.peek().ok_or(LoadError::Value)?;
                    self.i += 1;
                    let cp = match e {
                        '"' => 0x22,
                        '\\' => 0x5c,
                        '/' => 0x2f,
                        'b' => 0x08,
                        'f' => 0x0c,
                        'n' => 0x0a,
                        'r' => 0x0d,
                        't' => 0x09,
                        'u' => {
                            let hi = self.hex4(self.i).ok_or(LoadError::Value)?;
                            self.i += 4;
                            if (0xD800..0xDC00).contains(&hi)
                                && self.peek() == Some('\\')
                                && self.s.get(self.i + 1) == Some(&'u')
                            {
                                if let Some(lo) = self.hex4(self.i + 2).filter(|lo| (0xDC00..0xE000).contains(lo)) {
                                    self.i += 6;
                                    out.push(0x10000 + ((hi - 0xD800) << 10) + (lo - 0xDC00));
                                    continue;
                                }
                            }
                            hi
                        }
                        _ => return Err(LoadError::Value), // "Invalid \\escape"
                    };
                    out.push(cp);
                }
                c if u32::from(c) < 0x20 => return Err(LoadError::Value), // "Invalid control character"
                c => out.push(u32::from(c)),
            }
        }
    }
}

/// Split `{:e}` / `{:.Ne}` output ("d.ddde-7") into its digits and decimal exponent.
fn sci_parts(sci: &str) -> (String, i32) {
    let (mant, exp) = sci.split_once('e').expect("LowerExp has an exponent");
    (mant.chars().filter(|&c| c != '.').collect(), exp.parse().expect("integer exponent"))
}

/// The shortest digits CPython picks when Rust's differ: an exact tie.
///
/// Both pick, among the shortest digit strings that round back to `ax`, the one closest to
/// `ax`. They differ only when two such strings are exactly equally close. Rust's flt2dec then
/// falls back to Dragon, which rounds the tie up (`mant * 2 >= scale`). CPython's
/// `_Py_dg_dtoa` mode 0 (repr, json.dumps) bumps the last digit only when it is odd
/// (`j1 == 0 && dig & 1`), so the even digit wins. A tie means the exact binary value has
/// exactly one more significant digit than Rust's `n` digits, and that digit is a 5. Then the
/// candidates are the truncation `lower` and `lower + 1`, and Rust printed `lower + 1`. If
/// `lower` ends in an even digit and still rounds to `ax`, CPython prints `lower`. At a power of
/// two the interval below is half as wide, so the round-trip check is required. Ties need a
/// fractional value below 2^53 (no tie exists for integers >= 2^53).
fn cpython_tie_digits(ax: f64, digits: &str, exp: i32) -> Option<String> {
    // a double has at most 767 significant digits, so 801 of them are exact (zero padded)
    let (exact, exact_exp) = sci_parts(&format!("{ax:.800e}"));
    let exact = exact.trim_end_matches('0');
    let n = digits.len();
    if exact_exp != exp || exact.len() != n + 1 || !exact.ends_with('5') {
        return None;
    }
    let lower = &exact[..n];
    if lower == digits || (lower.as_bytes()[n - 1] - b'0') % 2 == 1 {
        return None; // Rust already printed the truncation, or the even digit is the upper one
    }
    let rounds_back = format!("{lower}e{}", exp + 1 - n as i32).parse::<f64>() == Ok(ax);
    rounds_back.then(|| lower.trim_end_matches('0').to_string())
}

/// `repr(float)` (also `str(float)`): the shortest round-trip digits, closest to the value, with
/// an exact tie going to the even digit as in CPython's dtoa. Scientific notation is used when
/// the decimal exponent is < -4 or >= 16, with a sign and at least two exponent digits.
pub fn float_repr(x: f64) -> String {
    if x.is_nan() {
        return "nan".into();
    }
    if x.is_infinite() {
        return if x > 0.0 { "inf" } else { "-inf" }.into();
    }
    if x == 0.0 {
        return if x.is_sign_negative() { "-0.0" } else { "0.0" }.into();
    }
    // shortest round-trip digits, e.g. "1.2345e-7"
    let (mut digits, exp) = sci_parts(&format!("{:e}", x.abs()));
    if let Some(even) = cpython_tie_digits(x.abs(), &digits, exp) {
        digits = even;
    }
    let nd = digits.len() as i32;
    let decpt = exp + 1; // value = 0.DIGITS * 10^decpt
    let mut out = String::new();
    if x < 0.0 {
        out.push('-');
    }
    if decpt <= -4 || decpt > 16 {
        out.push_str(&digits[..1]);
        if nd > 1 {
            out.push('.');
            out.push_str(&digits[1..]);
        }
        out.push_str(&format!("e{}{:02}", if exp < 0 { '-' } else { '+' }, exp.unsigned_abs()));
    } else if decpt <= 0 {
        out.push_str("0.");
        out.push_str(&"0".repeat((-decpt) as usize));
        out.push_str(&digits);
    } else if decpt >= nd {
        out.push_str(&digits);
        out.push_str(&"0".repeat((decpt - nd) as usize));
        out.push_str(".0");
    } else {
        out.push_str(&digits[..decpt as usize]);
        out.push('.');
        out.push_str(&digits[decpt as usize..]);
    }
    out
}

const HEX: &[u8; 16] = b"0123456789abcdef";

fn push_hex(out: &mut Vec<u32>, prefix: &str, cp: u32, width: u32) {
    out.extend(prefix.chars().map(u32::from));
    for k in (0..width).rev() {
        out.push(u32::from(HEX[((cp >> (4 * k)) & 0xf) as usize]));
    }
}

fn in_ranges(table: &[(char, char)], c: char) -> bool {
    table
        .binary_search_by(|&(lo, hi)| {
            if hi < c {
                std::cmp::Ordering::Less
            } else if lo > c {
                std::cmp::Ordering::Greater
            } else {
                std::cmp::Ordering::Equal
            }
        })
        .is_ok()
}

/// `Py_UNICODE_ISPRINTABLE`: false for categories Cc Cf Cs Co Cn Zl Zp Zs except U+0020, from
/// the Unicode 16.0.0 tables CPython 3.14 uses (not Rust's own table, which is 17.0.0 in Rust
/// 1.97 and would print newly assigned code points that CPython escapes as unassigned).
pub fn is_printable(cp: u32) -> bool {
    if cp == 0x20 {
        return true;
    }
    let Some(c) = char::from_u32(cp) else {
        return false; // lone surrogate (Cs)
    };
    !in_ranges(crate::unicode16::OTHER, c) && !in_ranges(crate::unicode16::SEPARATOR, c)
}

/// `repr(str)`: single quotes unless the text has ' and no ", backslash escapes for the quote,
/// `\\`, \t \n \r, `\xhh` / `\uhhhh` / `\Uhhhhhhhh` for non-printable code points.
pub fn repr_str(s: &[u32], out: &mut Vec<u32>) {
    let has_sq = s.contains(&0x27);
    let quote = if has_sq && !s.contains(&0x22) { 0x22 } else { 0x27 };
    out.push(quote);
    for &cp in s {
        match cp {
            _ if cp == quote || cp == 0x5c => out.extend([0x5c, cp]),
            0x09 => out.extend([0x5c, u32::from(b't')]),
            0x0a => out.extend([0x5c, u32::from(b'n')]),
            0x0d => out.extend([0x5c, u32::from(b'r')]),
            _ if cp < 0x20 || cp == 0x7f => push_hex(out, "\\x", cp, 2),
            _ if cp < 0x7f => out.push(cp),
            _ if is_printable(cp) => out.push(cp),
            _ if cp <= 0xff => push_hex(out, "\\x", cp, 2),
            _ if cp <= 0xffff => push_hex(out, "\\u", cp, 4),
            _ => push_hex(out, "\\U", cp, 8),
        }
    }
    out.push(quote);
}

/// `repr(v)`.
pub fn repr(v: &Val, out: &mut Vec<u32>) {
    match v {
        Val::None => out.extend(py("None")),
        Val::Bool(b) => out.extend(py(if *b { "True" } else { "False" })),
        Val::Int(d) => out.extend(py(d)),
        Val::Float(x) => out.extend(py(&float_repr(*x))),
        Val::Str(s) => repr_str(s, out),
        Val::List(items) => {
            out.push(u32::from(b'['));
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.extend(py(", "));
                }
                repr(item, out);
            }
            out.push(u32::from(b']'));
        }
        Val::Dict(d) => {
            out.push(u32::from(b'{'));
            for (i, (k, item)) in d.items().iter().enumerate() {
                if i > 0 {
                    out.extend(py(", "));
                }
                repr_str(k, out);
                out.extend(py(": "));
                repr(item, out);
            }
            out.push(u32::from(b'}'));
        }
    }
}

/// `str(v)`: the text itself for str, `repr(v)` for everything else.
pub fn py_str(v: &Val) -> PyStr {
    match v {
        Val::Str(s) => s.clone(),
        _ => {
            let mut out = Vec::new();
            repr(v, &mut out);
            out
        }
    }
}

fn dumps_str(s: &[u32], out: &mut String) {
    fn u_escape(out: &mut String, cp: u32) {
        out.push_str("\\u");
        for k in (0..4).rev() {
            out.push(char::from(HEX[((cp >> (4 * k)) & 0xf) as usize]));
        }
    }
    out.push('"');
    for &cp in s {
        match cp {
            0x22 => out.push_str("\\\""),
            0x5c => out.push_str("\\\\"),
            0x08 => out.push_str("\\b"),
            0x0c => out.push_str("\\f"),
            0x0a => out.push_str("\\n"),
            0x0d => out.push_str("\\r"),
            0x09 => out.push_str("\\t"),
            0x20..=0x7e => out.push(char::from(cp as u8)),
            0x10000.. => {
                let v = cp - 0x10000;
                u_escape(out, 0xD800 | (v >> 10));
                u_escape(out, 0xDC00 | (v & 0x3ff));
            }
            _ => u_escape(out, cp),
        }
    }
    out.push('"');
}

/// `json.dumps(v)` with default arguments; the result is pure ASCII.
pub fn dumps(v: &Val, out: &mut String) {
    match v {
        Val::None => out.push_str("null"),
        Val::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Val::Int(d) => out.push_str(d),
        Val::Float(x) if x.is_nan() => out.push_str("NaN"),
        Val::Float(x) if x.is_infinite() => out.push_str(if *x > 0.0 { "Infinity" } else { "-Infinity" }),
        Val::Float(x) => out.push_str(&float_repr(*x)),
        Val::Str(s) => dumps_str(s, out),
        Val::List(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                dumps(item, out);
            }
            out.push(']');
        }
        Val::Dict(d) => dumps_dict(d, out),
    }
}

/// `json.dumps(d)` for a dict.
pub fn dumps_dict(d: &Dict, out: &mut String) {
    out.push('{');
    for (i, (k, item)) in d.items().iter().enumerate() {
        if i > 0 {
            out.push_str(", ");
        }
        dumps_str(k, out);
        out.push_str(": ");
        dumps(item, out);
    }
    out.push('}');
}

#[cfg(test)]
mod tests {
    use super::*;

    fn load(s: &str) -> Result<Val, LoadError> {
        loads(&s.chars().collect::<Vec<_>>())
    }

    fn dump(s: &str) -> String {
        let mut out = String::new();
        dumps(&load(s).expect("valid JSON"), &mut out);
        out
    }

    fn rep(s: &str) -> String {
        let mut out = Vec::new();
        repr(&load(s).expect("valid JSON"), &mut out);
        out.into_iter().map(|cp| char::from_u32(cp).expect("repr is escaped")).collect()
    }

    fn rep_cps(cps: &[u32]) -> String {
        let mut out = Vec::new();
        repr_str(cps, &mut out);
        out.into_iter().map(|cp| char::from_u32(cp).expect("repr is escaped")).collect()
    }

    #[test]
    fn rejects_what_python_rejects() {
        for bad in [
            "", "[1,]", "{\"a\":1,}", "01", "-", "1.", ".5", "1e", "1e+", "+1", "nul", "true false",
            "[1 2]", "{\"a\" 1}", "{a:1}", "{'a':1}", "\"\t\"", "\"\\x41\"", "\"\\u12\"", "\"\\ud800\\u12g4\"",
            "\"abc", "[", "{", "-NaN", "nan", "infinity", "\u{feff}{}", "\"a\"\"b\"", "1 2", "--1", "-01",
        ] {
            assert_eq!(load(bad).unwrap_err(), LoadError::Value, "{bad:?}");
        }
    }

    #[test]
    fn accepts_python_extensions_and_edge_numbers() {
        assert_eq!(dump("[NaN, Infinity, -Infinity]"), "[NaN, Infinity, -Infinity]");
        assert_eq!(rep("[NaN, Infinity, -Infinity, -0, -0.0, 0e0, 1E5, 1e400, -1e-400]"),
                   "[nan, inf, -inf, 0, -0.0, 0.0, 100000.0, inf, -0.0]");
        assert_eq!(dump(" \t\r\n[ 1 , {\"a\" :\n2 } ] \r\n"), "[1, {\"a\": 2}]");
        assert_eq!(dump("1e99999999999999999999"), "Infinity");
        // big ints survive exactly up to the 4300-digit limit
        let big = "9".repeat(4300);
        assert_eq!(dump(&format!("-{big}")), format!("-{big}"));
        assert_eq!(load(&"9".repeat(4301)).unwrap_err(), LoadError::Value);
        assert_eq!(load(&format!("-{}", "1".repeat(4301))).unwrap_err(), LoadError::Value);
        // floats are not digit-limited
        assert_eq!(dump(&format!("{}.5", "1".repeat(5000))), "Infinity");
    }

    #[test]
    fn duplicate_keys_keep_first_position_last_value() {
        assert_eq!(dump("{\"a\":1,\"b\":2,\"a\":3}"), "{\"a\": 3, \"b\": 2}");
        assert_eq!(rep("{\"b\":{},\"a\":[],\"b\":null}"), "{'b': None, 'a': []}");
    }

    #[test]
    fn string_escapes_and_surrogates() {
        let s = |j: &str| match load(j).unwrap() {
            Val::Str(s) => s,
            v => panic!("not a str: {v:?}"),
        };
        assert_eq!(s(r#""\"\\\/\b\f\n\r\t\u0000\u00e9""#), [0x22, 0x5c, 0x2f, 8, 0xc, 0xa, 0xd, 9, 0, 0xe9]);
        assert_eq!(s(r#""\ud83d\ude00""#), [0x1f600]);
        assert_eq!(s(r#""\uD83D\uDE00""#), [0x1f600]);
        assert_eq!(s(r#""\ud83d\u0041""#), [0xd83d, 0x41]);
        assert_eq!(s(r#""\ude00\ud83d""#), [0xde00, 0xd83d]);
        assert_eq!(s(r#""\ud800""#), [0xd800]);
        assert_eq!(s(r#""\ud800\ud800\udc00""#), [0xd800, 0x10000]);
        assert_eq!(s("\"\u{7f}\u{1f600}\""), [0x7f, 0x1f600]);
        assert_eq!(dump(r#""\ud800x\udfff""#), r#""\ud800x\udfff""#);
        assert_eq!(dump("\"\u{7f}\u{e9}\u{1f600}\\u0001\""), r#""\u007f\u00e9\ud83d\ude00\u0001""#);
        assert_eq!(dump(r#""q\"b\\s\/""#), r#""q\"b\\s/""#);
    }

    #[test]
    fn float_repr_matches_python() {
        for (x, want) in [
            (1e16, "1e+16"), (1e15, "1000000000000000.0"), (1234567890123456.7, "1234567890123456.8"),
            (0.0001, "0.0001"), (0.00012, "0.00012"), (1e-5, "1e-05"), (1.5e-7, "1.5e-07"),
            (1.5e300, "1.5e+300"), (5e-324, "5e-324"), (0.1, "0.1"), (1.0 / 3.0, "0.3333333333333333"),
            (123456789.123, "123456789.123"), (-2.5, "-2.5"), (100.0, "100.0"), (1.7976931348623157e308, "1.7976931348623157e+308"),
            (123456789012345678.0, "1.2345678901234568e+17"), (0.118845942, "0.118845942"), (-0.0, "-0.0"),
        ] {
            assert_eq!(float_repr(x), want, "{x:e}");
        }
    }

    #[test]
    // the literals are exact binary values (1 + 2^-17, 2^49 + 1/4, ...): that is the point
    #[allow(clippy::excessive_precision)]
    fn float_repr_exact_ties_go_to_even_digit() {
        // CPython formatting regression (dtoa mode 0 keeps an
        // even last digit, Rust's {:e} rounds the tie up). An odd truncation still rounds up.
        for (x, want) in [
            (1.00000762939453125, "1.0000076293945312"), (562949953421312.25, "562949953421312.2"),
            (70368744177664.125, "70368744177664.12"), (8.0000152587890625, "8.000015258789062"),
            (562949953421312.75, "562949953421312.8"), (-1.00000762939453125, "-1.0000076293945312"),
            // 2^-25 = 2.98023223876953125e-08: a tie at a power of two (narrower interval below);
            // value derived from dtoa's rule, not observed from CPython
            (2f64.powi(-25), "2.9802322387695312e-08"),
        ] {
            assert_eq!(float_repr(x), want, "{x:e}");
        }
        // the same value through json.dumps and repr
        assert_eq!(dump("[1.00000762939453125, 562949953421312.25]"), "[1.0000076293945312, 562949953421312.2]");
        assert_eq!(rep("[8.0000152587890625]"), "[8.000015258789062]");
    }

    /// Independent reference for x = m * 2^-s, m in [2^52, 2^53), s in 0..=62, i.e. x in
    /// [2^-10, 2^53): fewest fraction digits f that round back to x, closest to x, exact tie to
    /// the even candidate. All in u128: m * 10^f < 2^120 for f <= 20.
    fn reference_repr(m: u64, s: u32) -> (String, bool) {
        let m = u128::from(m);
        let pow2 = m == 1 << 52; // the gap below x is half the gap above
        for f in 0..=20u32 {
            let p = 10u128.pow(f);
            let xm = m * p; // x * 10^f * 2^s
            let lo = xm >> s; // floor(x * 10^f)
            let mut inside = Vec::new(); // (distance * 10^f * 2^s, candidate)
            for c in [lo, lo + 1] {
                let cs = c << s;
                let (d, below) = if cs >= xm { (cs - xm, false) } else { (xm - cs, true) };
                // |c/10^f - x| <= half gap  <=>  2d <= 10^f (4d below a power of two); the
                // boundary itself rounds to x only when m is even (round half to even)
                let scaled = if below && pow2 { 4 * d } else { 2 * d };
                if scaled < p || (scaled == p && m.is_multiple_of(2)) {
                    inside.push((d, c));
                }
            }
            let tie = inside.len() == 2 && inside[0].0 == inside[1].0;
            let best = match inside.as_slice() {
                [] => continue,
                [(_, c)] => *c,
                [(d0, c0), (d1, c1)] => if d0 < d1 || (d0 == d1 && c0.is_multiple_of(2)) { *c0 } else { *c1 },
                _ => unreachable!(),
            };
            let int = best / p;
            let frac = format!("{:0width$}", best % p, width = f as usize);
            let frac = frac.trim_end_matches('0');
            return (format!("{int}.{}", if frac.is_empty() { "0" } else { frac }), tie);
        }
        unreachable!("17 significant digits always round-trip")
    }

    #[test]
    fn float_repr_agrees_with_exact_reference() {
        let mut state = 0x9E37_79B9_7F4A_7C15u64;
        let mut next = move || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state
        };
        let (mut ties, mut rust_raw_differs) = (0, 0);
        for i in 0..60_000 {
            let s = (next() % 63) as u32;
            let mut m = (1u64 << 52) | (next() & ((1 << 52) - 1));
            if i % 2 == 1 && s > 0 {
                // keep only t fraction bits so the exact decimal ends in ...5 after t digits
                // (s - t <= 52 keeps the leading bit of m)
                let t_min = s.saturating_sub(52).max(1);
                let t = t_min + (next() % u64::from(s - t_min + 1)) as u32;
                m = ((m >> (s - t)) | 1) << (s - t);
            }
            if i % 1000 == 7 {
                m = 1 << 52; // powers of two
            }
            let x = m as f64 / 2f64.powi(s as i32);
            assert_eq!(x * 2f64.powi(s as i32), m as f64, "exact construction");
            let (want, tie) = reference_repr(m, s);
            assert_eq!(float_repr(x), want, "m={m} s={s} x={x:e}");
            assert_eq!(want.parse::<f64>(), Ok(x));
            if tie {
                ties += 1;
                if format!("{x}") != want {
                    rust_raw_differs += 1;
                }
            }
        }
        // the sample must exercise the bug: many ties, and Rust's own Display rounds some up
        eprintln!("exact ties sampled: {ties}, of which Rust's Display rounds up: {rust_raw_differs}");
        assert!(ties >= 500 && rust_raw_differs >= 100, "ties={ties} rust_raw_differs={rust_raw_differs}");
    }

    #[test]
    fn repr_quotes_and_escapes() {
        assert_eq!(rep_cps(&py("it's")), "\"it's\"");
        assert_eq!(rep_cps(&py("say \"hi\"")), "'say \"hi\"'");
        assert_eq!(rep_cps(&py("both ' \"")), "'both \\' \"'");
        assert_eq!(rep_cps(&py("\\ \t\n\r\x00\x1f\x7f")), "'\\\\ \\t\\n\\r\\x00\\x1f\\x7f'");
        assert_eq!(rep_cps(&py("\u{80}\u{a0}\u{ad}\u{e9}\u{301}")), "'\\x80\\xa0\\xad\u{e9}\u{301}'");
        assert_eq!(rep_cps(&py("\u{200b}\u{2028}\u{3000}\u{feff}\u{e000}\u{378}\u{fffd}")),
                   "'\\u200b\\u2028\\u3000\\ufeff\\ue000\\u0378\u{fffd}'");
        assert_eq!(rep_cps(&py("\u{1f600}\u{e0001}\u{f0000}\u{10ffff}")), "'\u{1f600}\\U000e0001\\U000f0000\\U0010ffff'");
        assert_eq!(rep_cps(&[0xd800, 0xdc80]), "'\\ud800\\udc80'");
        assert_eq!(rep_cps(&py("")), "''");
    }

    #[test]
    fn repr_and_str_of_containers() {
        assert_eq!(rep(r#"{"a": [1, 2.5, null, true, false, "x'"], "b": {}, "c": -7}"#),
                   "{'a': [1, 2.5, None, True, False, \"x'\"], 'b': {}, 'c': -7}");
        assert_eq!(dump(r#"{"a": [1, 2.5, null, true, false, "x'"], "b": {}, "c": [], "d": -7}"#),
                   r#"{"a": [1, 2.5, null, true, false, "x'"], "b": {}, "c": [], "d": -7}"#);
        let v = load(r#""plain""#).unwrap();
        assert_eq!(py_str(&v), py("plain"));
        assert_eq!(py_str(&load("null").unwrap()), py("None"));
        assert_eq!(py_str(&load("1e16").unwrap()), py("1e+16"));
    }

    #[test]
    fn truthiness() {
        for (j, t) in [("null", false), ("false", false), ("0", false), ("-0", false), ("0.0", false), ("-0.0", false),
                       ("\"\"", false), ("[]", false), ("{}", false), ("NaN", true), ("1", true), ("\" \"", true),
                       ("[0]", true), ("{\"a\":null}", true), ("true", true), ("1e-400", false)] {
            assert_eq!(load(j).unwrap().truthy(), t, "{j}");
        }
    }

    #[test]
    fn depth_limit_is_loud() {
        // debug-build frames are large; the binary runs the audit on a thread of this size too
        std::thread::Builder::new()
            .stack_size(STACK_BYTES)
            .spawn(|| {
                let ok = format!("{}0{}", "[{\"a\":".repeat(MAX_DEPTH / 2), "}]".repeat(MAX_DEPTH / 2));
                assert_eq!(dump(&ok).len(), ok.len() + MAX_DEPTH / 2);
                assert!(rep(&ok).starts_with("[{'a': [{'a': "));
                let deep = format!("{}{}", "[".repeat(MAX_DEPTH + 1), "]".repeat(MAX_DEPTH + 1));
                assert_eq!(load(&deep).unwrap_err(), LoadError::TooDeep);
                // a syntax error before the depth limit is a plain ValueError, as in CPython
                assert_eq!(load(&format!("[x{}", "[".repeat(MAX_DEPTH + 1))).unwrap_err(), LoadError::Value);
                // Depth regression: 1001 unclosed '[' is a ValueError in CPython 3.14, so the
                // line is only unparsed; the same holds up to MAX_DEPTH
                assert_eq!(load(&"[".repeat(1001)).unwrap_err(), LoadError::Value);
                assert_eq!(load(&"[".repeat(MAX_DEPTH)).unwrap_err(), LoadError::Value);
                assert_eq!(load(&"[".repeat(MAX_DEPTH + 1)).unwrap_err(), LoadError::TooDeep);
            })
            .expect("spawn")
            .join()
            .expect("depth test thread");
    }

    #[test]
    fn printable_table_agrees_with_rust_except_new_assignments() {
        // Rust's escape_debug uses the same category list (core/src/unicode/printable.py) at
        // char::UNICODE_VERSION; escape_debug escapes grapheme extenders only at the start of a
        // str, so probe after an 'a'. A newer Unicode only assigns former Cn code points, so every
        // disagreement must be "unassigned in 16.0 (escaped by CPython), printable in Rust".
        let rust_printable = |c: char| {
            let probe: String = ['a', c].iter().collect();
            probe.escape_debug().eq(probe.chars())
        };
        let mut newly_assigned = 0usize;
        for c in (0..=0x10FFFFu32).filter_map(char::from_u32) {
            if c.is_ascii() {
                assert_eq!(is_printable(u32::from(c)), (' '..='~').contains(&c), "{c:?}");
            } else if is_printable(u32::from(c)) != rust_printable(c) {
                assert!(rust_printable(c) && !is_printable(u32::from(c)), "U+{:04X}", u32::from(c));
                newly_assigned += 1;
            }
        }
        let (a, b, v) = char::UNICODE_VERSION;
        eprintln!("rust Unicode {a}.{b}.{v}: {newly_assigned} code points printable there but Cn in 16.0.0");
        if char::UNICODE_VERSION == (16, 0, 0) {
            assert_eq!(newly_assigned, 0);
        }
        assert!(newly_assigned < 10_000);
        // spot checks against known CPython 3.14 results
        for (cp, want) in [(0xa0, false), (0xad, false), (0x301, true), (0x378, false), (0x200b, false),
                           (0x2028, false), (0x3000, false), (0xfeff, false), (0xfffd, true), (0xe000, false),
                           (0x1f600, true), (0xe0001, false), (0xf0000, false), (0x10ffff, false), (0xd800, false)] {
            assert_eq!(is_printable(cp), want, "U+{cp:04X}");
        }
    }
}
