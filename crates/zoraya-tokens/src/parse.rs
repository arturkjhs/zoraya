//! The `const` reader of `tokens.txt` (ADR-0161, Option C): `name = value`
//! lines, `#` comments, `light:` and `dark:` prefixes for colours. Every
//! lookup runs at compile time, so a missing name or a bad value fails the
//! build (INV-0161-2). The runtime walk over the same lines, for the tests
//! and for comparing with the mockup tool's parser, is [`lines`].

use crate::{Color, SOURCE};

/// Which set a name is looked up in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Set {
    Plain,
    Light,
    Dark,
}

/// The end of the line's content: before a comment, which is `#` at the
/// start (after spaces) or `#` after a space and before a space or the end
/// (so that `#rrggbb` values survive). As the mockup tool reads it.
const fn content_end(b: &[u8], start: usize, end: usize) -> usize {
    let mut i = start;
    while i < end && (b[i] == b' ' || b[i] == b'\t') {
        i += 1;
    }
    if i < end && b[i] == b'#' {
        return start;
    }
    let mut i = start + 1;
    while i < end {
        if b[i] == b'#' && b[i - 1] == b' ' && (i + 1 == end || b[i + 1] == b' ') {
            return i;
        }
        i += 1;
    }
    end
}

const fn is_space(c: u8) -> bool {
    c == b' ' || c == b'\t' || c == b'\r'
}

/// `(start, end)` without spaces at either end.
const fn trim(b: &[u8], mut start: usize, mut end: usize) -> (usize, usize) {
    while start < end && is_space(b[start]) {
        start += 1;
    }
    while end > start && is_space(b[end - 1]) {
        end -= 1;
    }
    (start, end)
}

const fn starts_with(b: &[u8], start: usize, end: usize, prefix: &[u8]) -> bool {
    if end - start < prefix.len() {
        return false;
    }
    let mut i = 0;
    while i < prefix.len() {
        if b[start + i] != prefix[i] {
            return false;
        }
        i += 1;
    }
    true
}

const fn equals(b: &[u8], start: usize, end: usize, name: &[u8]) -> bool {
    end - start == name.len() && starts_with(b, start, end, name)
}

/// One token line: its set, and the byte ranges of its name and value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Line {
    pub set: Set,
    pub name: (usize, usize),
    pub value: (usize, usize),
}

/// Reads the line `start..end`: None for a blank or comment line, and
/// Some(Err(())) for one with no `=`, an empty name or an empty value.
pub const fn read_line(b: &[u8], start: usize, end: usize) -> Option<Result<Line, ()>> {
    let end = content_end(b, start, end);
    let (start, end) = trim(b, start, end);
    if start == end {
        return None;
    }
    let mut eq = start;
    while eq < end && b[eq] != b'=' {
        eq += 1;
    }
    if eq == end {
        return Some(Err(()));
    }
    let (mut ks, ke) = trim(b, start, eq);
    let value = trim(b, eq + 1, end);
    let mut set = Set::Plain;
    if starts_with(b, ks, ke, b"light:") {
        set = Set::Light;
        ks += 6;
    } else if starts_with(b, ks, ke, b"dark:") {
        set = Set::Dark;
        ks += 5;
    }
    let name = trim(b, ks, ke);
    if name.0 == name.1 || value.0 == value.1 {
        return Some(Err(()));
    }
    Some(Ok(Line { set, name, value }))
}

/// The value's range of `name` in `set`: the first line that defines it.
pub const fn find(b: &[u8], set: Set, name: &[u8]) -> Option<(usize, usize)> {
    let mut start = 0;
    while start < b.len() {
        let mut end = start;
        while end < b.len() && b[end] != b'\n' {
            end += 1;
        }
        if let Some(Ok(line)) = read_line(b, start, end)
            && matches!(
                (line.set, set),
                (Set::Plain, Set::Plain) | (Set::Light, Set::Light) | (Set::Dark, Set::Dark)
            )
            && equals(b, line.name.0, line.name.1, name)
        {
            return Some(line.value);
        }
        start = end + 1;
    }
    None
}

/// The value of a plain token, as text.
pub const fn value(name: &str) -> &'static str {
    let b = SOURCE.as_bytes();
    let Some((start, end)) = find(b, Set::Plain, name.as_bytes()) else {
        panic!("a token tokens.txt does not define");
    };
    let (_, rest) = b.split_at(start);
    let (bytes, _) = rest.split_at(end - start);
    match core::str::from_utf8(bytes) {
        Ok(s) => s,
        Err(_) => panic!("a token value that is not UTF-8"),
    }
}

/// A decimal whole number.
pub const fn parse_int(s: &[u8]) -> Option<u32> {
    if s.is_empty() || s.len() > 9 {
        return None;
    }
    let mut n: u32 = 0;
    let mut i = 0;
    while i < s.len() {
        let c = s[i];
        if !c.is_ascii_digit() {
            return None;
        }
        n = n * 10 + (c - b'0') as u32;
        i += 1;
    }
    Some(n)
}

/// A plain token that is a whole number.
pub const fn int(name: &str) -> u32 {
    match parse_int(value(name).as_bytes()) {
        Some(n) => n,
        None => panic!("a token that should be a whole number"),
    }
}

/// `#rrggbb`.
pub const fn parse_color(s: &[u8]) -> Option<Color> {
    if s.len() != 7 || s[0] != b'#' {
        return None;
    }
    let mut n: u32 = 0;
    let mut i = 1;
    while i < 7 {
        let c = s[i];
        let d = match c {
            b'0'..=b'9' => c - b'0',
            b'a'..=b'f' => c - b'a' + 10,
            b'A'..=b'F' => c - b'A' + 10,
            _ => return None,
        };
        n = n << 4 | d as u32;
        i += 1;
    }
    Some(Color(n))
}

/// A colour of the light or the dark set.
pub const fn color(set: Set, name: &str) -> Color {
    let b = SOURCE.as_bytes();
    let Some((start, end)) = find(b, set, name.as_bytes()) else {
        panic!("a colour tokens.txt does not define in this set");
    };
    let (_, rest) = b.split_at(start);
    let (bytes, _) = rest.split_at(end - start);
    match parse_color(bytes) {
        Some(c) => c,
        None => panic!("a colour token that is not #rrggbb"),
    }
}

/// A decimal fraction such as `0.2` or `1`, in thousandths.
pub const fn parse_milli(s: &[u8]) -> Option<u32> {
    let mut whole: u32 = 0;
    let mut i = 0;
    let mut digits = 0;
    while i < s.len() && s[i].is_ascii_digit() {
        whole = whole * 10 + (s[i] - b'0') as u32;
        i += 1;
        digits += 1;
    }
    if digits == 0 || digits > 6 {
        return None;
    }
    let mut frac: u32 = 0;
    let mut scale: u32 = 100;
    if i < s.len() {
        if s[i] != b'.' || i + 1 == s.len() {
            return None;
        }
        i += 1;
        while i < s.len() {
            if !s[i].is_ascii_digit() || scale == 0 {
                return None;
            }
            frac += (s[i] - b'0') as u32 * scale;
            scale /= 10;
            i += 1;
        }
    }
    Some(whole * 1000 + frac)
}

/// The `n`th of the space-separated words of a plain token.
pub const fn word(name: &str, n: usize) -> &'static [u8] {
    let b = value(name).as_bytes();
    let mut i = 0;
    let mut k = 0;
    while i < b.len() {
        while i < b.len() && b[i] == b' ' {
            i += 1;
        }
        let start = i;
        while i < b.len() && b[i] != b' ' {
            i += 1;
        }
        if start < i {
            if k == n {
                let (_, rest) = b.split_at(start);
                let (w, _) = rest.split_at(i - start);
                return w;
            }
            k += 1;
        }
    }
    panic!("a token with fewer words than read")
}

/// Whether a plain token's value is `want`.
pub const fn is(name: &str, want: &str) -> bool {
    let v = value(name).as_bytes();
    equals(v, 0, v.len(), want.as_bytes())
}

/// Every token line of `text`, at run time; `Err(line number)` for a line
/// [`read_line`] refuses.
pub fn lines(text: &str) -> impl Iterator<Item = Result<(Set, &str, &str), usize>> {
    let b = text.as_bytes();
    let mut start = 0;
    let mut number = 0;
    core::iter::from_fn(move || {
        while start < b.len() {
            let mut end = start;
            while end < b.len() && b[end] != b'\n' {
                end += 1;
            }
            let at = start;
            start = end + 1;
            number += 1;
            match read_line(b, at, end) {
                None => continue,
                Some(Err(())) => return Some(Err(number)),
                Some(Ok(line)) => {
                    let name = &text[line.name.0..line.name.1];
                    let value = &text[line.value.0..line.value.1];
                    return Some(Ok((line.set, name, value)));
                }
            }
        }
        None
    })
}
