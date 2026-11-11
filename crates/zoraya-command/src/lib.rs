//! What every native command shares (ADR-0155): its arguments parsed from
//! its description (ADR-0156), `--help` and usage errors in one format, error
//! lines `NAME: SUBJECT: PROBLEM` with statuses in words, and the exit codes.
//! A native command never parses argv itself (rule 7, INV-0155-2).
//!
//! Syntax (rule 2): every option has a long form `--name`, some a short form
//! `-n`; short options without an argument group (`-fv`); an argument follows
//! as `--name value`, `--name=value`, `-n value` or `-nvalue`; options may come
//! anywhere before `--`, which ends them. `-` alone is an operand, and so is a
//! word that starts with `-` and a digit unless the command has a digit option
//! (`math -1 + 2`). A word that starts with `./` never starts with `-`, which
//! is how a glob gives names that do (ADR-0150 rule 7).

#![cfg_attr(not(test), no_std)]

extern crate alloc;

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::fmt::Write;

use zoraya_abi::Status;
use zoraya_describe::{Arity, Description, Kind, What};
use zoraya_proto_file::status as file;

pub use zoraya_describe;

/// Exit codes (rule 6).
pub mod exit {
    pub const SUCCESS: u8 = 0;
    pub const FAILURE: u8 = 1;
    pub const USAGE: u8 = 2;
    /// A question's "no".
    pub const NO: u8 = 1;
    /// A question's failure.
    pub const QUESTION_FAILED: u8 = 3;
}

/// The code a command exits with when it failed (1, or 3 for a question).
pub fn failure_code(d: &Description) -> u8 {
    if d.question {
        exit::QUESTION_FAILED
    } else {
        exit::FAILURE
    }
}

/// What the arguments asked for.
#[derive(Debug, PartialEq, Eq)]
pub enum Outcome<'a> {
    Run(Parsed<'a>),
    /// `--help` was given: print [`help`] and exit 0.
    Help,
}

/// The arguments, sorted into options and operands.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Parsed<'a> {
    /// Each option given, by index into the description's options, with its
    /// argument. In the order given.
    pub options: Vec<(usize, Option<&'a str>)>,
    /// Each operand argument, by index into the description's operands.
    pub operands: Vec<(usize, &'a str)>,
}

impl<'a> Parsed<'a> {
    fn option_index(d: &Description, long: &str) -> usize {
        d.option_by_long(long)
            .unwrap_or_else(|| panic!("{}: no option --{long} in the description", d.name))
    }

    /// Whether `--long` was given. Panics if the description has no such
    /// option: that is a mistake in the program, not in its arguments.
    pub fn flag(&self, d: &Description, long: &str) -> bool {
        let i = Parsed::option_index(d, long);
        self.options.iter().any(|(o, _)| *o == i)
    }

    /// The argument of the last `--long` given.
    pub fn value(&self, d: &Description, long: &str) -> Option<&'a str> {
        let i = Parsed::option_index(d, long);
        self.options
            .iter()
            .rev()
            .find(|(o, _)| *o == i)
            .and_then(|(_, v)| *v)
    }

    /// The arguments of operand `NAME`, in order.
    pub fn operand_values(&self, d: &Description, name: &str) -> Vec<&'a str> {
        let i = d
            .operand_by_name(name)
            .unwrap_or_else(|| panic!("{}: no operand {name} in the description", d.name));
        self.operands
            .iter()
            .filter(|(o, _)| *o == i)
            .map(|(_, v)| *v)
            .collect()
    }

    /// The one argument of operand `NAME`, if given.
    pub fn operand(&self, d: &Description, name: &str) -> Option<&'a str> {
        self.operand_values(d, name).first().copied()
    }

    /// The kind of operand `index` with the options given (`when`).
    pub fn kind_of(&self, d: &Description, index: usize) -> Kind {
        let mut kind = d.operands[index].kind;
        for w in &d.whens {
            if w.operand == index && self.options.iter().any(|(o, _)| *o == w.option) {
                kind = w.kind;
            }
        }
        kind
    }
}

/// A usage mistake: the problem, as `NAME: PROBLEM` puts it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsageError(pub String);

fn is_option_word(d: &Description, arg: &str) -> bool {
    let Some(rest) = arg.strip_prefix('-') else {
        return false;
    };
    match rest.chars().next() {
        None => false, // `-` alone
        Some('-') => true,
        Some(c) if c.is_ascii_digit() => d
            .options
            .iter()
            .any(|o| o.short.is_some_and(|s| s.is_ascii_digit())),
        Some(_) => true,
    }
}

/// Sorts `args` (without the command's name) by `d`.
pub fn parse<'a>(d: &Description, args: &[&'a str]) -> Result<Outcome<'a>, UsageError> {
    let end = args.iter().position(|a| *a == "--").unwrap_or(args.len());
    if args[..end].contains(&"--help") {
        return Ok(Outcome::Help);
    }
    let mut parsed = Parsed::default();
    let mut positional: Vec<&'a str> = Vec::new();
    let mut i = 0;
    while i < args.len() {
        let (at, arg) = (i, args[i]);
        i += 1;
        // Everything after `--` is an operand; `--` itself is not.
        if at >= end {
            if at > end {
                positional.push(arg);
            }
            continue;
        }
        if !is_option_word(d, arg) {
            positional.push(arg);
            continue;
        }
        if let Some(long) = arg.strip_prefix("--") {
            let (name, inline) = match long.split_once('=') {
                Some((n, v)) => (n, Some(v)),
                None => (long, None),
            };
            let Some(o) = d.option_by_long(name) else {
                return Err(UsageError(format!("--{name}: no such option")));
            };
            let value = match (&d.options[o].argument, inline) {
                (None, None) => None,
                (None, Some(_)) => return Err(UsageError(format!("--{name}: takes no value"))),
                (Some(_), Some(v)) => Some(v),
                (Some(_), None) => {
                    let Some(v) = args.get(i).filter(|_| i < end) else {
                        return Err(UsageError(format!("--{name}: needs a value")));
                    };
                    i += 1;
                    Some(*v)
                }
            };
            parsed.options.push((o, value));
            continue;
        }
        // A group of short options.
        let group = &arg[1..];
        for (at, c) in group.char_indices() {
            let Some(o) = d.option_by_short(c) else {
                return Err(UsageError(format!("-{c}: no such option")));
            };
            if d.options[o].argument.is_none() {
                parsed.options.push((o, None));
                continue;
            }
            let rest = &group[at + c.len_utf8()..];
            let value = if !rest.is_empty() {
                rest
            } else {
                let Some(v) = args.get(i).filter(|_| i < end) else {
                    return Err(UsageError(format!("-{c}: needs a value")));
                };
                i += 1;
                v
            };
            parsed.options.push((o, Some(value)));
            break;
        }
    }
    assign(d, &positional, &mut parsed)?;
    Ok(Outcome::Run(parsed))
}

fn assign<'a>(
    d: &Description,
    positional: &[&'a str],
    parsed: &mut Parsed<'a>,
) -> Result<(), UsageError> {
    let required = d.operands.iter().filter(|o| o.arity.required()).count();
    if positional.len() < required {
        // The first required operand the arguments do not reach.
        let mut left = positional.len();
        for o in &d.operands {
            if o.arity.required() {
                if left == 0 {
                    return Err(UsageError(format!("missing operand {}", o.name)));
                }
                left -= 1;
            }
        }
    }
    let mut extra = positional.len().saturating_sub(required);
    let mut at = 0;
    for (index, o) in d.operands.iter().enumerate() {
        let take = match o.arity {
            Arity::One => 1,
            Arity::Optional => {
                let t = usize::from(extra > 0);
                extra -= t;
                t
            }
            Arity::Many => {
                let t = 1 + extra;
                extra = 0;
                t
            }
            Arity::Any => core::mem::take(&mut extra),
        };
        for v in &positional[at..at + take] {
            parsed.operands.push((index, v));
        }
        at += take;
    }
    if let Some(next) = positional.get(at) {
        return Err(UsageError(format!("{next}: unexpected operand")));
    }
    Ok(())
}

fn pad(s: &mut String, text: &str, width: usize) {
    s.push_str(text);
    for _ in text.chars().count()..width {
        s.push(' ');
    }
}

fn usage_lines(d: &Description, s: &mut String) {
    for (i, u) in d.usage.iter().enumerate() {
        s.push_str(if i == 0 { "usage: " } else { "       " });
        s.push_str(u);
        s.push('\n');
    }
}

/// The options `--help` lists, `--help` last: `(label, text)`.
pub fn help_options(d: &Description) -> Vec<(String, String)> {
    let mut list: Vec<(String, String)> = d
        .options
        .iter()
        .map(|o| (o.label(), o.text.clone()))
        .collect();
    list.push(("--help".into(), "show this help".into()));
    list
}

/// The options Tab lists after `--` (ADR-0154 rule 3): long forms only.
pub fn completion_options(d: &Description) -> Vec<(String, String)> {
    let mut list: Vec<(String, String)> = d
        .options
        .iter()
        .map(|o| (format!("--{}", o.long), o.text.clone()))
        .collect();
    list.push(("--help".into(), "show this help".into()));
    list
}

/// Lines `label   text`, each label padded to the longest plus `gap`.
pub fn columns(items: &[(String, String)], indent: &str, gap: usize) -> String {
    let width = items
        .iter()
        .map(|(l, _)| l.chars().count())
        .max()
        .unwrap_or(0)
        + gap;
    let mut s = String::new();
    for (label, text) in items {
        s.push_str(indent);
        pad(&mut s, label, width);
        s.push_str(text);
        s.push('\n');
    }
    s
}

/// `NAME --help` (rule 3).
pub fn help(d: &Description) -> String {
    let mut s = format!("{}: {}\n", d.name, d.summary);
    usage_lines(d, &mut s);
    s.push_str(&columns(&help_options(d), "  ", 3));
    let _ = writeln!(s, "more: help {}", d.name);
    s
}

/// A usage mistake (rule 3): the problem, the usage and where to read more.
pub fn usage_error(d: &Description, e: &UsageError) -> String {
    let mut s = format!("{}: {}\n", d.name, e.0);
    usage_lines(d, &mut s);
    let _ = writeln!(s, "more: help {}", d.name);
    s
}

/// The page `help NAME` shows (ADR-0156 item 4), after `first` (what NAME is).
pub fn page(d: &Description, first: &str) -> String {
    let mut s = String::new();
    if !first.is_empty() {
        s.push_str(first);
        s.push_str("\n\n");
    }
    s.push_str(&d.name.to_uppercase());
    s.push_str("\n\n");
    s.push_str(&d.summary);
    s.push('\n');
    if d.what == What::Command {
        s.push('\n');
        usage_lines(d, &mut s);
        let operands: Vec<(String, String)> = d
            .operands
            .iter()
            .map(|o| {
                let text = if o.kind.is_path() {
                    format!("{} ({})", o.text, o.kind.shown())
                } else {
                    o.text.clone()
                };
                (o.name.clone(), text)
            })
            .collect();
        let options: Vec<(String, String)> = d
            .options
            .iter()
            .map(|o| (o.label(), o.text.clone()))
            .collect();
        if !operands.is_empty() || !options.is_empty() {
            s.push('\n');
            s.push_str(&columns(&operands, "  ", 3));
            s.push_str(&columns(&options, "  ", 3));
        }
    }
    let mut exits_done = d.exits.is_empty();
    let mut settings_done = d.settings.is_empty();
    for section in &d.sections {
        let late = matches!(section.title.as_deref(), Some("SEE ALSO"));
        if late {
            write_exits(d, &mut s, &mut exits_done);
            write_settings(d, &mut s, &mut settings_done);
        }
        s.push('\n');
        if let Some(t) = &section.title {
            s.push_str(t);
            s.push('\n');
        }
        for line in &section.lines {
            s.push_str(line);
            s.push('\n');
        }
    }
    write_exits(d, &mut s, &mut exits_done);
    write_settings(d, &mut s, &mut settings_done);
    s
}

fn write_exits(d: &Description, s: &mut String, done: &mut bool) {
    if *done {
        return;
    }
    *done = true;
    s.push_str("\nEXIT STATUS\n");
    let items: Vec<(String, String)> = d
        .exits
        .iter()
        .map(|e| (e.code.to_string(), e.text.clone()))
        .collect();
    s.push_str(&columns(&items, "  ", 2));
}

fn write_settings(d: &Description, s: &mut String, done: &mut bool) {
    if *done {
        return;
    }
    *done = true;
    s.push_str("\nSETTINGS\n");
    let items: Vec<(String, String)> = d
        .settings
        .iter()
        .map(|t| {
            (
                t.name.clone(),
                format!(
                    "{}, default {}: {}",
                    t.ty.word(),
                    zoraya_describe::quote_value(&t.default),
                    t.text
                ),
            )
        })
        .collect();
    s.push_str(&columns(&items, "  ", 2));
}

/// A kernel status in words (rule 5): never the bare name, never a number.
pub fn status_problem(status: Status) -> String {
    let fixed = match status {
        Status::AccessDenied => "not granted",
        Status::NoMemory => "out of memory",
        Status::LimitReached => "a limit was reached",
        Status::DeadlineExceeded => "took too long",
        Status::PeerClosed => "the other end is gone",
        Status::BadHandle => "not a usable handle",
        Status::InvalidArgument => "not a valid argument",
        Status::NotSupported => "not supported by this machine",
        Status::OverQuota => "over quota",
        _ => "",
    };
    if !fixed.is_empty() {
        return fixed.into();
    }
    words(&format!("{status:?}"))
}

/// `CamelCase` to `camel case`.
fn words(name: &str) -> String {
    let mut s = String::new();
    for (i, c) in name.chars().enumerate() {
        if c.is_ascii_uppercase() {
            if i > 0 {
                s.push(' ');
            }
            s.push(c.to_ascii_lowercase());
        } else {
            s.push(c);
        }
    }
    s
}

/// The status the file client gives when the connection itself is gone
/// (`zoraya_user::files::GONE`).
pub const FILE_GONE: u32 = 1000;

/// A file-protocol status in words (rule 5).
pub fn file_problem(status: u32) -> String {
    let text = match status {
        file::OK => "succeeded",
        file::NOT_FOUND => "not found",
        file::EXISTS => "already exists",
        file::NO_SPACE => "the volume is full",
        file::NOT_EMPTY => "not empty",
        file::INVALID_NAME => "not a valid name",
        file::NOT_DIRECTORY => "not a directory",
        file::IS_DIRECTORY => "is a directory",
        file::IS_LINK => "is a link",
        file::INVALID_ARGUMENT => "not a valid argument",
        file::CORRUPT => "the volume is damaged",
        file::STORAGE_FAILED => "the volume has failed",
        file::ACCESS_DENIED => "not granted",
        file::TOO_MANY_OPEN => "too many open files",
        file::LIMIT_REACHED => "a limit was reached",
        file::UNSUPPORTED => "not supported",
        FILE_GONE => "the file service is gone",
        other => return format!("unknown error (file status {other})"),
    };
    text.into()
}

/// One error line (rule 5): `NAME: SUBJECT: PROBLEM`, or `NAME: PROBLEM`
/// without a subject, then each hint as `  hint: HINT`.
pub fn error(name: &str, subject: Option<&str>, problem: &str, hints: &[&str]) -> String {
    let mut s = String::from(name);
    s.push_str(": ");
    if let Some(subject) = subject {
        s.push_str(subject);
        s.push_str(": ");
    }
    s.push_str(problem);
    s.push('\n');
    for h in hints {
        s.push_str("  hint: ");
        s.push_str(h);
        s.push('\n');
    }
    s
}

/// What a command does when writing its output failed with `status`: a
/// closed reader ends it quietly with 0 (rule 6, INV-0155-4); anything else
/// is a failure to report.
pub fn output_failed(status: Status) -> Option<u8> {
    (status == Status::PeerClosed).then_some(exit::SUCCESS)
}

#[cfg(test)]
mod tests;

// A runaway test fails at 512 MiB instead of taking the host (docs/backlog.md).
#[cfg(test)]
zoraya_test_alloc::cap_memory!();
