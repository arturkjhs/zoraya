//! The pinned `xterm-256color` entry, as the tests read it (ADR-0332 item
//! 10, INV-0332-1): `entry/xterm-256color.caps` is `infocmp -x -1` of the
//! file the pinned `tic` compiles (`tools/term-conformance/check.sh pin`
//! compares them), and a test feeds the terminal what the entry says to
//! send, not what the test's author remembers. A capability is covered when a
//! test calls [`cap`], [`flag`] or [`num`] with its name; the conformance
//! tool counts those calls. Only the tests use this module.

use std::string::String;
use std::vec::Vec;

const DUMP: &str = include_str!("../entry/xterm-256color.caps");

fn line(name: &str) -> &'static str {
    DUMP.lines()
        .find(|l| {
            l.strip_prefix(name)
                .is_some_and(|rest| rest.is_empty() || rest.starts_with(['=', '#']))
        })
        .unwrap_or_else(|| panic!("the entry has no capability {name}"))
}

/// A boolean capability: the entry has it.
pub(crate) fn flag(name: &str) -> bool {
    assert_eq!(line(name), name, "{name} is not a flag");
    true
}

/// A numeric capability.
pub(crate) fn num(name: &str) -> i64 {
    let v = line(name)
        .strip_prefix(name)
        .and_then(|r| r.strip_prefix('#'))
        .unwrap_or_else(|| panic!("{name} is not a number"));
    match v.strip_prefix("0x") {
        Some(hex) => i64::from_str_radix(hex, 16).unwrap(),
        None => v.parse().unwrap(),
    }
}

/// The bytes capability `name` sends, with `args` for its parameters
/// (`%p1` is `args[0]`), padding (`$<...>`) left out.
pub(crate) fn cap(name: &str, args: &[i32]) -> Vec<u8> {
    let raw = line(name)
        .strip_prefix(name)
        .and_then(|r| r.strip_prefix('='))
        .unwrap_or_else(|| panic!("{name} is not a string"));
    let text = unescape(raw);
    strip_padding(expand(&text, args))
}

/// `\E`, `\n`, `\007`, `^G` and the rest of terminfo's escapes.
fn unescape(s: &str) -> Vec<u8> {
    let b = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'%' if i + 1 < b.len() => {
                // A parameter operation: kept for `expand`.
                out.push(b'%');
                out.push(b[i + 1]);
                i += 2;
            }
            b'\\' if i + 1 < b.len() => {
                i += 1;
                match b[i] {
                    b'E' | b'e' => out.push(0x1b),
                    b'n' => out.push(b'\n'),
                    b'r' => out.push(b'\r'),
                    b't' => out.push(b'\t'),
                    b'b' => out.push(8),
                    b'f' => out.push(12),
                    b's' => out.push(b' '),
                    b'0'..=b'7' => {
                        let mut v = 0u32;
                        let mut n = 0;
                        while n < 3 && i < b.len() && (b'0'..=b'7').contains(&b[i]) {
                            v = v * 8 + u32::from(b[i] - b'0');
                            i += 1;
                            n += 1;
                        }
                        out.push(v as u8);
                        continue;
                    }
                    c => out.push(c),
                }
                i += 1;
            }
            b'^' if i + 1 < b.len() => {
                out.push(if b[i + 1] == b'?' {
                    0x7f
                } else {
                    b[i + 1] & 0x1f
                });
                i += 2;
            }
            c => {
                out.push(c);
                i += 1;
            }
        }
    }
    out
}

fn strip_padding(bytes: Vec<u8>) -> Vec<u8> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'$' && bytes.get(i + 1) == Some(&b'<') {
            while i < bytes.len() && bytes[i] != b'>' {
                i += 1;
            }
            i += 1;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    out
}

/// terminfo's parameterised strings, the part of them the entry uses.
fn expand(s: &[u8], args: &[i32]) -> Vec<u8> {
    let mut p = [0i32; 9];
    for (slot, a) in p.iter_mut().zip(args) {
        *slot = *a;
    }
    let mut stack: Vec<i32> = Vec::new();
    let mut out = Vec::new();
    let mut i = 0;
    while i < s.len() {
        if s[i] != b'%' {
            out.push(s[i]);
            i += 1;
            continue;
        }
        i += 1;
        // `%` flags, width and precision before a conversion.
        let start = i;
        while i < s.len() && (s[i].is_ascii_digit() || s[i] == b'.' || s[i] == b':') {
            i += 1;
        }
        let spec = String::from_utf8_lossy(&s[start..i]).into_owned();
        let c = s[i];
        i += 1;
        match c {
            b'%' => out.push(b'%'),
            b'p' => {
                stack.push(p[usize::from(s[i] - b'1')]);
                i += 1;
            }
            b'd' | b'X' | b'x' | b'o' => {
                let v = stack.pop().unwrap();
                let (width, precision) = match spec.split_once('.') {
                    Some((w, pr)) => (w.parse().unwrap_or(0), pr.parse().unwrap_or(0)),
                    None => (spec.parse().unwrap_or(0), 0usize),
                };
                let text = match c {
                    b'd' => std::format!("{v}"),
                    b'X' => std::format!("{v:X}"),
                    b'x' => std::format!("{v:x}"),
                    _ => std::format!("{v:o}"),
                };
                let text = format_padded(&text, width, precision);
                out.extend_from_slice(text.as_bytes());
            }
            b'c' => out.push(stack.pop().unwrap() as u8),
            b'i' => {
                p[0] += 1;
                p[1] += 1;
            }
            b'{' => {
                let end = s[i..].iter().position(|b| *b == b'}').unwrap() + i;
                stack.push(String::from_utf8_lossy(&s[i..end]).parse().unwrap());
                i = end + 1;
            }
            b'+' | b'-' | b'*' | b'/' | b'm' | b'&' | b'|' | b'^' | b'=' | b'>' | b'<' | b'A'
            | b'O' => {
                let b = stack.pop().unwrap();
                let a = stack.pop().unwrap();
                stack.push(match c {
                    b'+' => a + b,
                    b'-' => a - b,
                    b'*' => a * b,
                    b'/' => a / b,
                    b'm' => a % b,
                    b'&' => a & b,
                    b'|' => a | b,
                    b'^' => a ^ b,
                    b'=' => i32::from(a == b),
                    b'>' => i32::from(a > b),
                    b'<' => i32::from(a < b),
                    b'A' => i32::from(a != 0 && b != 0),
                    _ => i32::from(a != 0 || b != 0),
                });
            }
            b'!' => {
                let a = stack.pop().unwrap();
                stack.push(i32::from(a == 0));
            }
            b'?' | b';' => {}
            b't' => {
                if stack.pop().unwrap() == 0 {
                    i = skip(s, i, true);
                }
            }
            b'e' => i = skip(s, i, false),
            other => panic!("unsupported %{} in the entry", other as char),
        }
    }
    out
}

fn format_padded(text: &str, width: usize, precision: usize) -> String {
    let mut digits = String::from(text);
    while digits.len() < precision {
        digits.insert(0, '0');
    }
    while digits.len() < width {
        digits.insert(0, ' ');
    }
    digits
}

/// After a false `%t`: to the matching `%e` (one past it), or past the
/// `%;`; after the true branch's `%e`: past the matching `%;`.
fn skip(s: &[u8], mut i: usize, stop_at_else: bool) -> usize {
    let mut depth = 0;
    while i + 1 < s.len() {
        if s[i] == b'%' {
            match s[i + 1] {
                b'?' => depth += 1,
                b';' if depth == 0 => return i + 2,
                b';' => depth -= 1,
                b'e' if depth == 0 && stop_at_else => return i + 2,
                _ => {}
            }
            i += 2;
        } else {
            i += 1;
        }
    }
    s.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_dump_expands_the_way_terminfo_does() {
        assert_eq!(cap("cup", &[4, 9]), b"\x1b[5;10H");
        assert_eq!(cap("home", &[]), b"\x1b[H");
        assert_eq!(cap("bel", &[]), b"\x07");
        assert_eq!(cap("cr", &[]), b"\r");
        assert_eq!(cap("setaf", &[1]), b"\x1b[31m");
        assert_eq!(cap("setaf", &[9]), b"\x1b[91m");
        assert_eq!(cap("setaf", &[200]), b"\x1b[38;5;200m");
        assert_eq!(cap("setab", &[12]), b"\x1b[104m");
        assert_eq!(cap("flash", &[]), b"\x1b[?5h\x1b[?5l");
        assert_eq!(cap("rep", &[i32::from(b'x'), 5]), b"x\x1b[4b");
        assert_eq!(cap("XM", &[1]), b"\x1b[?1006;1000h");
        assert_eq!(cap("XM", &[0]), b"\x1b[?1006;1000l");
        assert_eq!(
            cap("initc", &[17, 1000, 0, 500]),
            b"\x1b]4;17;rgb:FF/00/7F\x1b\\"
        );
        assert_eq!(cap("sgr", &[0, 0, 0, 0, 0, 0, 0, 0, 0]), b"\x1b(B\x1b[0m");
        assert_eq!(cap("sgr", &[0, 0, 0, 0, 0, 0, 0, 0, 1]), b"\x1b(0\x1b[0m");
        assert_eq!(
            cap("sgr", &[1, 0, 0, 1, 0, 0, 0, 0, 0]),
            b"\x1b(B\x1b[0;7;5m"
        );
        assert!(flag("am") && flag("bce"));
        assert_eq!(num("colors"), 256);
        assert_eq!(num("cols"), 80);
    }
}
