//! `/config/settings` (ADR-0157): lines `NAME = VALUE`, `#` comment lines and
//! blank lines. `VALUE` is one word of ADR-0150's quoting with no expansion.
//! Settings are declared by the components that own them (`setting` lines in
//! their descriptions, ADR-0156); defaults live there, never in the file.
//!
//! The reader is total (INV-0157-3): any bytes give values and a list of
//! problems, each bad line reported once, and a bad line's setting keeps its
//! default. The writer changes one line and keeps every other byte
//! (INV-0157-1), and refuses an undeclared name or a value outside its type
//! without changing anything (INV-0157-2).

#![cfg_attr(not(test), no_std)]

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use zoraya_describe::{Setting, is_setting_name, quote_value, split_value};

/// Largest file read, in bytes; the rest is reported and ignored.
pub const MAX_SIZE: usize = 256 * 1024;

/// A problem with one line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Problem {
    /// From 1.
    pub line: usize,
    pub problem: String,
}

/// One line as the reader understood it.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Line<'a> {
    /// Blank or a comment.
    Other,
    Setting {
        name: &'a str,
        value: String,
    },
    Bad(&'static str),
}

fn parse_line(text: &str) -> Line<'_> {
    let t = text.trim_matches([' ', '\t']);
    if t.is_empty() || t.starts_with('#') {
        return Line::Other;
    }
    let Some((name, rest)) = t.split_once('=') else {
        return Line::Bad("not NAME = VALUE");
    };
    let name = name.trim_end_matches([' ', '\t']);
    if !is_setting_name(name) {
        return Line::Bad("not a setting name: GROUP.NAME in lower case");
    }
    match split_value(rest) {
        Ok((value, "")) => Line::Setting { name, value },
        Ok(_) => Line::Bad("text after the value; quote a value with spaces"),
        Err(problem) => Line::Bad(problem),
    }
}

/// The lines of `bytes`, each as text if it is UTF-8 without control
/// characters, with its number from 1.
pub(crate) fn lines(bytes: &[u8]) -> impl Iterator<Item = (usize, Result<&str, &'static str>)> {
    bytes.split(|b| *b == b'\n').enumerate().map(|(i, raw)| {
        let raw = raw.strip_suffix(b"\r").unwrap_or(raw);
        let text = match core::str::from_utf8(raw) {
            Err(_) => Err("not UTF-8"),
            Ok(t) if t.chars().any(|c| c.is_control() && c != '\t') => Err("a control character"),
            Ok(t) => Ok(t),
        };
        (i + 1, text)
    })
}

/// What a component reads: the value of each of its settings, and the
/// problems to report once each.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Values {
    pub values: BTreeMap<String, String>,
    pub problems: Vec<Problem>,
}

impl Values {
    pub fn get(&self, name: &str) -> Option<&str> {
        self.values.get(name).map(String::as_str)
    }
}

/// Reads the file for a component that declares `declared`. Names of other
/// groups belong to other components and are left alone; a bad value or an
/// unknown name of one of its groups is a problem, and the default applies.
pub fn read(bytes: &[u8], declared: &[Setting]) -> Values {
    let mut v = Values::default();
    for d in declared {
        v.values.insert(d.name.clone(), d.default.clone());
    }
    let groups: Vec<&str> = declared
        .iter()
        .filter_map(|d| d.name.split_once('.').map(|(g, _)| g))
        .collect();
    let mut problem = |line, problem: String| v.problems.push(Problem { line, problem });
    let (bytes, too_long) = if bytes.len() > MAX_SIZE {
        (&bytes[..MAX_SIZE], true)
    } else {
        (bytes, false)
    };
    let mut seen: Vec<String> = Vec::new();
    let mut found: Vec<(String, String)> = Vec::new();
    for (number, text) in lines(bytes) {
        let text = match text {
            Ok(t) => t,
            Err(p) => {
                problem(number, p.into());
                continue;
            }
        };
        match parse_line(text) {
            Line::Other => {}
            Line::Bad(p) => problem(number, p.into()),
            Line::Setting { name, value } => {
                let group = name.split_once('.').map_or("", |(g, _)| g);
                if !groups.contains(&group) {
                    continue;
                }
                let Some(d) = declared.iter().find(|d| d.name == name) else {
                    problem(number, format!("{name}: no such setting"));
                    continue;
                };
                if seen.iter().any(|s| s == name) {
                    problem(number, format!("{name}: set twice; the first line counts"));
                    continue;
                }
                seen.push(name.into());
                match d.ty.check(&value) {
                    Ok(()) => found.push((name.into(), value)),
                    Err(p) => problem(number, format!("{name}: {p}")),
                }
            }
        }
    }
    if too_long {
        let lines = 1 + bytes.iter().filter(|b| **b == b'\n').count();
        problem(
            lines,
            "the file is longer than 256 KiB; the rest is ignored".into(),
        );
    }
    for (name, value) in found {
        v.values.insert(name, value);
    }
    v
}

/// The line `NAME = VALUE` as the writer writes it.
pub fn line(name: &str, value: &str) -> String {
    format!("{name} = {}", quote_value(value))
}

/// Why a change was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    /// No component declares it; the nearest declared names, if any.
    Unknown(Vec<String>),
    /// The value is not of the setting's type: `maybe is not yes or no`.
    Value(String),
}

/// `settings NAME VALUE`: the file with `name` set to `value`. The first
/// line that sets `name` is replaced; any later ones (a mistake by hand) are
/// left for `--check` to report. Every other line is kept byte for byte.
pub fn set(
    bytes: &[u8],
    declared: &[Setting],
    name: &str,
    value: &str,
) -> Result<Vec<u8>, Refusal> {
    let Some(d) = declared.iter().find(|d| d.name == name) else {
        return Err(Refusal::Unknown(nearest(name, declared)));
    };
    d.ty.check(value).map_err(Refusal::Value)?;
    if value.contains(['\n', '\r']) || value.chars().any(char::is_control) {
        return Err(Refusal::Value(format!(
            "{} has a control character",
            quote_value(value)
        )));
    }
    let new = line(name, value);
    let mut out = Vec::with_capacity(bytes.len() + new.len() + 1);
    let mut done = false;
    let parts: Vec<&[u8]> = bytes.split(|b| *b == b'\n').collect();
    for (i, raw) in parts.iter().enumerate() {
        let is_it = !done
            && core::str::from_utf8(raw)
                .ok()
                .map(|t| parse_line(t.strip_suffix('\r').unwrap_or(t)))
                .is_some_and(|l| matches!(l, Line::Setting { name: n, .. } if n == name));
        if is_it {
            out.extend_from_slice(new.as_bytes());
            done = true;
        } else {
            out.extend_from_slice(raw);
        }
        if i + 1 < parts.len() {
            out.push(b'\n');
        }
    }
    if !done {
        if !out.is_empty() && !out.ends_with(b"\n") {
            out.push(b'\n');
        }
        out.extend_from_slice(new.as_bytes());
        out.push(b'\n');
    }
    Ok(out)
}

/// `settings --reset NAME`: the file without the lines that set `name`.
pub fn reset(bytes: &[u8], name: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len());
    let parts: Vec<&[u8]> = bytes.split(|b| *b == b'\n').collect();
    let mut first = true;
    for (i, raw) in parts.iter().enumerate() {
        let is_it = core::str::from_utf8(raw)
            .ok()
            .map(|t| parse_line(t.strip_suffix('\r').unwrap_or(t)))
            .is_some_and(|l| matches!(l, Line::Setting { name: n, .. } if n == name));
        if is_it {
            continue;
        }
        if i + 1 == parts.len() && raw.is_empty() {
            // The final line end belongs to the last line kept.
            if !first {
                out.push(b'\n');
            }
            break;
        }
        if !first {
            out.push(b'\n');
        }
        out.extend_from_slice(raw);
        first = false;
    }
    out
}

/// The declared names within edit distance 2 of `name`.
pub fn nearest(name: &str, declared: &[Setting]) -> Vec<String> {
    let mut found: Vec<(usize, &str)> = declared
        .iter()
        .map(|d| (distance(name, &d.name), d.name.as_str()))
        .filter(|(d, _)| *d <= 2)
        .collect();
    found.sort();
    let best = found.first().map_or(0, |(d, _)| *d);
    found
        .into_iter()
        .take_while(|(d, _)| *d == best)
        .map(|(_, n)| n.to_string())
        .collect()
}

fn distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    if a.len().abs_diff(b.len()) > 2 || a.len() > 64 || b.len() > 64 {
        return 3;
    }
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.iter().enumerate() {
        let mut prev = row[0];
        row[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let cur = row[j + 1];
            row[j + 1] = (prev + usize::from(ca != cb)).min(row[j] + 1).min(cur + 1);
            prev = cur;
        }
    }
    row[b.len()]
}

pub mod rooms;

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_rooms;

// A runaway test fails at 512 MiB instead of taking the host (docs/backlog.md).
#[cfg(test)]
zoraya_test_alloc::cap_memory!();
