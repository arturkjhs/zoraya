//! Command descriptions (ADR-0156): one `NAME.cmd` file per command, parsed
//! into a typed form that argv parsing, `--help`, completion, grants and
//! `help NAME` all read, so that they cannot disagree.
//!
//! A description is UTF-8 text. The header is lines `KEY VALUE` up to a line
//! `---`; the body follows.
//!
//! ```text
//! name     cp
//! summary  copy a file
//! usage    cp [--force] FROM TO
//! operand  FROM  read    the file to copy
//! operand  TO    create  the name of the copy
//! option   -f --force    replace TO if it exists
//! exit     0  copied
//! exit     1  not copied
//! ---
//! Copies the file FROM to a new file TO.
//!
//! EXAMPLES
//!   cp notes.txt notes.bak
//! ```
//!
//! Keys: `name` or `topic`, `summary`, `usage` (one or more), `operand NAME
//! KIND TEXT`, `option [SHORT] LONG [ARG KIND] TEXT` (an upper-case word is
//! the argument only when a kind follows it), `when OPTION OPERAND
//! KIND`, `cwd read|none`, `question`, `program PATH`, `exit CODE TEXT` and
//! `setting NAME TYPE DEFAULT TEXT`.
//!
//! An operand's NAME says how many arguments it takes, as the usage line
//! writes it: `FILE` exactly one, `[FILE]` zero or one, `FILE...` one or
//! more, `[FILE...]` zero or more. At most one operand takes several, and no
//! operand follows an optional one but another optional one.
//!
//! The parser is total and bounded (INV-0156-1): any bytes give a
//! description or an [`Error`] with a line number, and nothing it keeps is
//! larger than the input.

#![cfg_attr(not(test), no_std)]

extern crate alloc;

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::fmt;

/// Largest description accepted, in bytes.
pub const MAX_SIZE: usize = 64 * 1024;
/// Longest line, in bytes.
pub const MAX_LINE: usize = 1024;
/// Most entries in any one list (operands, options, exits, ...).
pub const MAX_ITEMS: usize = 64;
/// Longest command or topic name. With `.cmd` it fits a ramdisk name.
pub const MAX_NAME: usize = 24;

/// What a description is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum What {
    Command,
    /// A page that is not a command (`help language`).
    Topic,
}

/// What an operand or option argument is (ADR-0153 rule 2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Read,
    Write,
    Create,
    Remove,
    RemoveTree,
    Rename,
    Tree,
    /// Not a path.
    Text,
}

impl Kind {
    pub const ALL: [Kind; 8] = [
        Kind::Read,
        Kind::Write,
        Kind::Create,
        Kind::Remove,
        Kind::RemoveTree,
        Kind::Rename,
        Kind::Tree,
        Kind::Text,
    ];

    pub fn parse(word: &str) -> Option<Kind> {
        Kind::ALL.into_iter().find(|k| k.word() == word)
    }

    /// The word in a description.
    pub fn word(self) -> &'static str {
        match self {
            Kind::Read => "read",
            Kind::Write => "write",
            Kind::Create => "create",
            Kind::Remove => "remove",
            Kind::RemoveTree => "remove-tree",
            Kind::Rename => "rename",
            Kind::Tree => "tree",
            Kind::Text => "text",
        }
    }

    /// How `help NAME` describes the access: `(read)`, `(created)`.
    pub fn shown(self) -> &'static str {
        match self {
            Kind::Read => "read",
            Kind::Write => "changed",
            Kind::Create => "created",
            Kind::Remove => "removed",
            Kind::RemoveTree => "removed with all in it",
            Kind::Rename => "renamed",
            Kind::Tree => "anything in it changed",
            Kind::Text => "text",
        }
    }

    pub fn is_path(self) -> bool {
        self != Kind::Text
    }
}

/// How many arguments an operand takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Arity {
    /// `NAME`
    One,
    /// `[NAME]`
    Optional,
    /// `NAME...`
    Many,
    /// `[NAME...]`
    Any,
}

impl Arity {
    pub fn required(self) -> bool {
        matches!(self, Arity::One | Arity::Many)
    }

    pub fn several(self) -> bool {
        matches!(self, Arity::Many | Arity::Any)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Operand {
    /// Without brackets or dots: `FILE`.
    pub name: String,
    pub arity: Arity,
    pub kind: Kind,
    pub text: String,
}

/// An option's argument.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Argument {
    pub name: String,
    pub kind: Kind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Opt {
    pub short: Option<char>,
    /// Without the dashes: `force`.
    pub long: String,
    pub argument: Option<Argument>,
    pub text: String,
}

impl Opt {
    /// `-f, --force` or `--all`, with `N` after an argument-taking one.
    pub fn label(&self) -> String {
        let mut s = String::new();
        if let Some(c) = self.short {
            s.push('-');
            s.push(c);
            s.push_str(", ");
        }
        s.push_str("--");
        s.push_str(&self.long);
        if let Some(a) = &self.argument {
            s.push(' ');
            s.push_str(&a.name);
        }
        s
    }
}

/// An option that changes an operand's kind: `when --recursive PATH remove-tree`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct When {
    /// Index into [`Description::options`].
    pub option: usize,
    /// Index into [`Description::operands`].
    pub operand: usize,
    pub kind: Kind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cwd {
    /// The current directory, read-only (ADR-0153 rule 1).
    Read,
    /// Nothing of the file system.
    None,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Exit {
    pub code: u8,
    pub text: String,
}

/// A setting's type (ADR-0157 item 3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SettingType {
    YesNo,
    Number {
        min: i64,
        max: i64,
    },
    Text,
    Path,
    Choice(Vec<String>),
    /// ADR-0171: items of one type, `min` to `max` of them, `unique`
    /// refusing a repeated one. The value is the items as words
    /// ([`list_text`]).
    List {
        item: alloc::boxed::Box<SettingType>,
        min: usize,
        max: usize,
        unique: bool,
    },
}

/// The most items a list may be declared to hold, and the bound when a
/// declaration gives none (ADR-0171, `[assumption]`).
pub const MAX_LIST: usize = 64;

/// A list's value as its items: words of ADR-0150's syntax, an item with a
/// space quoted; the empty text is the empty list.
pub fn list_items(value: &str) -> core::result::Result<Vec<String>, &'static str> {
    let mut items = Vec::new();
    let mut rest = value.trim_matches([' ', '\t']);
    while !rest.is_empty() {
        let (item, after) = split_value(rest)?;
        if items.len() >= MAX_LIST {
            return Err("more items than a list holds");
        }
        items.push(item);
        rest = after;
    }
    Ok(items)
}

/// Items as a list's value: each quoted only when it must be.
pub fn list_text(items: &[String]) -> String {
    let words: Vec<String> = items.iter().map(|i| quote_value(i)).collect();
    words.join(" ")
}

impl SettingType {
    /// Checks a value against the type; the refusal is a phrase for
    /// `NAME: PROBLEM` (`maybe is not yes or no`).
    pub fn check(&self, value: &str) -> core::result::Result<(), String> {
        let ok = match self {
            SettingType::YesNo => value == "yes" || value == "no",
            SettingType::Number { min, max } => {
                parse_number(value).is_some_and(|n| *min <= n && n <= *max)
            }
            SettingType::Text => true,
            SettingType::Path => value.starts_with('/'),
            SettingType::Choice(choices) => choices.iter().any(|c| c == value),
            SettingType::List {
                item,
                min,
                max,
                unique,
            } => {
                let items = list_items(value).map_err(String::from)?;
                for (i, it) in items.iter().enumerate() {
                    item.check(it)?;
                    if *unique && items[..i].contains(it) {
                        return Err(format!("{} is in the list twice", quote_value(it)));
                    }
                }
                if items.len() < *min || items.len() > *max {
                    return Err(format!(
                        "a list of {} items, not {min} to {max}",
                        items.len()
                    ));
                }
                true
            }
        };
        if ok {
            return Ok(());
        }
        let shown = if value.is_empty() { "''" } else { value };
        let mut s = String::from(shown);
        match self {
            SettingType::YesNo => s.push_str(" is not yes or no"),
            SettingType::Number { min, max } => {
                s.push_str(" is not a number from ");
                s.push_str(&min.to_string());
                s.push_str(" to ");
                s.push_str(&max.to_string());
            }
            SettingType::Path => s.push_str(" is not an absolute path"),
            SettingType::Choice(choices) => {
                s.push_str(" is not one of ");
                s.push_str(&choices.join(", "));
            }
            SettingType::Text | SettingType::List { .. } => {}
        }
        Err(s)
    }

    /// The type as a description writes it: `number 0 100000`.
    pub fn word(&self) -> String {
        match self {
            SettingType::YesNo => "yes-no".into(),
            SettingType::Number { min, max } => {
                let mut s = String::from("number ");
                s.push_str(&min.to_string());
                s.push(' ');
                s.push_str(&max.to_string());
                s
            }
            SettingType::Text => "text".into(),
            SettingType::Path => "path".into(),
            SettingType::Choice(c) => {
                let mut s = String::from("choice ");
                s.push_str(&c.join("|"));
                s
            }
            SettingType::List {
                item,
                min,
                max,
                unique,
            } => {
                let mut s = format!("list of {} {min} {max}", item.word());
                if *unique {
                    s.push_str(" unique");
                }
                s
            }
        }
    }
}

/// A decimal integer with an optional `-`, without overflow.
pub fn parse_number(text: &str) -> Option<i64> {
    let (negative, digits) = match text.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, text),
    };
    if digits.is_empty() || digits.len() > 18 || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let mut n: i64 = 0;
    for b in digits.bytes() {
        n = n * 10 + i64::from(b - b'0');
    }
    Some(if negative { -n } else { n })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Setting {
    pub name: String,
    pub ty: SettingType,
    pub default: String,
    pub text: String,
}

/// A part of the body. The first, untitled, is the description.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Section {
    pub title: Option<String>,
    pub lines: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Description {
    pub what: What,
    pub name: String,
    pub summary: String,
    pub usage: Vec<String>,
    pub operands: Vec<Operand>,
    pub options: Vec<Opt>,
    pub whens: Vec<When>,
    pub cwd: Cwd,
    /// Exit 1 means "no" (ADR-0155 rule 6).
    pub question: bool,
    pub program: Option<String>,
    pub exits: Vec<Exit>,
    pub settings: Vec<Setting>,
    pub sections: Vec<Section>,
}

impl Description {
    /// A description with only a name and a summary, for builders such as a
    /// function's (ADR-0156 item 3).
    pub fn new(what: What, name: &str, summary: &str) -> Description {
        Description {
            what,
            name: name.into(),
            summary: summary.into(),
            usage: Vec::new(),
            operands: Vec::new(),
            options: Vec::new(),
            whens: Vec::new(),
            cwd: Cwd::Read,
            question: false,
            program: None,
            exits: Vec::new(),
            settings: Vec::new(),
            sections: Vec::new(),
        }
    }

    pub fn option_by_long(&self, long: &str) -> Option<usize> {
        self.options.iter().position(|o| o.long == long)
    }

    pub fn option_by_short(&self, short: char) -> Option<usize> {
        self.options.iter().position(|o| o.short == Some(short))
    }

    pub fn operand_by_name(&self, name: &str) -> Option<usize> {
        self.operands.iter().position(|o| o.name == name)
    }

    pub fn setting(&self, name: &str) -> Option<&Setting> {
        self.settings.iter().find(|s| s.name == name)
    }
}

/// Why a description was refused, and on which line (1 is the first; 0 is
/// the file as a whole).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Error {
    pub line: usize,
    pub problem: &'static str,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.line == 0 {
            f.write_str(self.problem)
        } else {
            write!(f, "line {}: {}", self.line, self.problem)
        }
    }
}

type Result<T> = core::result::Result<T, Error>;

/// The first word of `s` and what follows it, without leading blanks.
fn next_word(s: &str) -> Option<(&str, &str)> {
    let s = s.trim_start_matches([' ', '\t']);
    if s.is_empty() {
        return None;
    }
    let end = s.find([' ', '\t']).unwrap_or(s.len());
    Some((&s[..end], s[end..].trim_start_matches([' ', '\t'])))
}

/// One value word of ADR-0150's quoting, with no expansion: a bare word,
/// `'...'` (nothing special inside), or `"..."` with `\"` and `\\`. Gives the
/// value and what follows it. Used for setting defaults here and for the
/// settings file (ADR-0157 item 2).
pub fn split_value(s: &str) -> core::result::Result<(String, &str), &'static str> {
    let s = s.trim_start_matches([' ', '\t']);
    let mut chars = s.char_indices();
    match chars.next() {
        None => Err("a value is missing"),
        Some((_, '\'')) => {
            let body = &s[1..];
            let end = body.find('\'').ok_or("a quote is not closed")?;
            let rest = &body[end + 1..];
            if !rest.is_empty() && !rest.starts_with([' ', '\t']) {
                return Err("text follows a closing quote");
            }
            Ok((body[..end].into(), rest.trim_start_matches([' ', '\t'])))
        }
        Some((_, '"')) => {
            let mut value = String::new();
            let mut escaped = false;
            for (i, c) in s.char_indices().skip(1) {
                if escaped {
                    if c != '"' && c != '\\' {
                        return Err("only \\\" and \\\\ are escapes in a value");
                    }
                    value.push(c);
                    escaped = false;
                } else if c == '\\' {
                    escaped = true;
                } else if c == '"' {
                    let rest = &s[i + 1..];
                    if !rest.is_empty() && !rest.starts_with([' ', '\t']) {
                        return Err("text follows a closing quote");
                    }
                    return Ok((value, rest.trim_start_matches([' ', '\t'])));
                } else {
                    value.push(c);
                }
            }
            Err("a quote is not closed")
        }
        Some(_) => {
            let end = s.find([' ', '\t']).unwrap_or(s.len());
            let word = &s[..end];
            if word.contains(['\'', '"', '\\']) {
                return Err("a quote inside a word");
            }
            Ok((word.into(), s[end..].trim_start_matches([' ', '\t'])))
        }
    }
}

/// Writes `value` so that [`split_value`] gives it back.
pub fn quote_value(value: &str) -> String {
    let bare = !value.is_empty()
        && !value.contains([' ', '\t', '\'', '"', '\\'])
        && !value.starts_with('#');
    if bare {
        return value.into();
    }
    let mut s = String::new();
    if !value.contains('\'') {
        s.push('\'');
        s.push_str(value);
        s.push('\'');
        return s;
    }
    s.push('"');
    for c in value.chars() {
        if c == '"' || c == '\\' {
            s.push('\\');
        }
        s.push(c);
    }
    s.push('"');
    s
}

fn is_command_name(s: &str) -> bool {
    let b = s.as_bytes();
    !b.is_empty()
        && b.len() <= MAX_NAME
        && b[0].is_ascii_lowercase()
        && b.iter()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'-')
}

fn is_upper_name(s: &str) -> bool {
    let b = s.as_bytes();
    !b.is_empty()
        && b.len() <= MAX_NAME
        && b[0].is_ascii_uppercase()
        && b.iter()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || *c == b'_' || *c == b'-')
}

fn is_long_name(s: &str) -> bool {
    let b = s.as_bytes();
    !b.is_empty()
        && b.len() <= MAX_NAME
        && b[0].is_ascii_lowercase()
        && b.iter()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'-')
}

/// `GROUP.NAME`, lower case with `-` (ADR-0157 item 2).
pub fn is_setting_name(s: &str) -> bool {
    match s.split_once('.') {
        Some((group, name)) => is_long_name(group) && is_long_name(name) && s.len() <= 48,
        None => false,
    }
}

fn is_section_title(line: &str) -> bool {
    let b = line.as_bytes();
    !b.is_empty()
        && b.len() <= 32
        && b[0].is_ascii_uppercase()
        && b.iter().all(|c| c.is_ascii_uppercase() || *c == b' ')
}

/// Body sections a command may have, in this order (mdoc's). EXIT STATUS is
/// written from the `exit` lines.
const COMMAND_SECTIONS: [&str; 3] = ["EXAMPLES", "FILES", "SEE ALSO"];

fn push<T>(list: &mut Vec<T>, item: T, line: usize) -> Result<()> {
    if list.len() >= MAX_ITEMS {
        return Err(Error {
            line,
            problem: "too many entries of this kind",
        });
    }
    list.push(item);
    Ok(())
}

fn err<T>(line: usize, problem: &'static str) -> Result<T> {
    Err(Error { line, problem })
}

fn text_of(rest: &str, line: usize) -> Result<String> {
    let t = rest.trim_end_matches([' ', '\t']);
    if t.is_empty() {
        return err(line, "the text is missing");
    }
    Ok(t.into())
}

fn parse_operand_name(word: &str) -> Option<(String, Arity)> {
    let (inner, optional) = match word.strip_prefix('[') {
        Some(rest) => (rest.strip_suffix(']')?, true),
        None => (word, false),
    };
    let (name, several) = match inner.strip_suffix("...") {
        Some(n) => (n, true),
        None => (inner, false),
    };
    if !is_upper_name(name) {
        return None;
    }
    let arity = match (optional, several) {
        (false, false) => Arity::One,
        (true, false) => Arity::Optional,
        (false, true) => Arity::Many,
        (true, true) => Arity::Any,
    };
    Some((name.into(), arity))
}

fn parse_option(rest: &str, line: usize) -> Result<Opt> {
    let Some((first, mut rest)) = next_word(rest) else {
        return err(line, "an option needs a name");
    };
    let mut short = None;
    let long_word = if let Some(l) = first.strip_prefix("--") {
        l
    } else if let Some(s) = first.strip_prefix('-') {
        let mut cs = s.chars();
        match (cs.next(), cs.next()) {
            (Some(c), None) if c.is_ascii_alphanumeric() => short = Some(c),
            _ => return err(line, "a short option is one letter after -"),
        }
        let Some((second, after)) = next_word(rest) else {
            return err(line, "an option needs a long form");
        };
        rest = after;
        match second.strip_prefix("--") {
            Some(l) => l,
            None => return err(line, "an option needs a long form"),
        }
    } else {
        return err(line, "an option starts with - or --");
    };
    if !is_long_name(long_word) {
        return err(line, "a long option is lower-case letters, digits and -");
    }
    if long_word == "help" {
        return err(line, "--help is given to every command");
    }
    // `ARG KIND` only when a kind follows the upper-case word: otherwise the
    // word starts the text (`option -d --directory PATH is a directory`).
    let mut argument = None;
    if let Some((word, after)) = next_word(rest)
        && is_upper_name(word)
        && let Some((kind, after)) = next_word(after)
        && let Some(kind) = Kind::parse(kind)
    {
        argument = Some(Argument {
            name: word.into(),
            kind,
        });
        rest = after;
    }
    Ok(Opt {
        short,
        long: long_word.into(),
        argument,
        text: text_of(rest, line)?,
    })
}

fn parse_setting(rest: &str, line: usize) -> Result<Setting> {
    let Some((name, rest)) = next_word(rest) else {
        return err(line, "a setting needs a name");
    };
    if !is_setting_name(name) {
        return err(line, "a setting's name is GROUP.NAME in lower case");
    }
    let Some((ty, rest)) = next_word(rest) else {
        return err(line, "a setting needs a type");
    };
    let (ty, rest) = if ty == "list" {
        let Some(("of", after)) = next_word(rest) else {
            return err(line, "a list is `list of TYPE`");
        };
        let Some((word, after)) = next_word(after) else {
            return err(line, "a list is `list of TYPE`");
        };
        let (item, mut rest) = item_type(word, after, line)?;
        let (mut min, mut max) = (0, MAX_LIST);
        if let Some((a, r1)) = next_word(rest)
            && let Some((b, r2)) = next_word(r1)
            && let (Some(a), Some(b)) = (parse_number(a), parse_number(b))
        {
            if a < 0 || a > b || b > MAX_LIST as i64 {
                return err(
                    line,
                    "a list's MIN and MAX are counts from 0 to 64, MIN first",
                );
            }
            (min, max) = (a as usize, b as usize);
            rest = r2;
        }
        let mut unique = false;
        if let Some(("unique", r)) = next_word(rest) {
            unique = true;
            rest = r;
        }
        let ty = SettingType::List {
            item: alloc::boxed::Box::new(item),
            min,
            max,
            unique,
        };
        (ty, rest)
    } else {
        item_type(ty, rest, line)?
    };
    let (default, rest) = split_value(rest).map_err(|problem| Error { line, problem })?;
    if ty.check(&default).is_err() {
        return err(line, "the default is not of the setting's type");
    }
    Ok(Setting {
        name: name.into(),
        ty,
        default,
        text: text_of(rest, line)?,
    })
}

/// One of the five item types (ADR-0157 rule 3), and what follows it.
fn item_type<'a>(ty: &str, rest: &'a str, line: usize) -> Result<(SettingType, &'a str)> {
    let mut rest = rest;
    let ty = match ty {
        "yes-no" => SettingType::YesNo,
        "text" => SettingType::Text,
        "path" => SettingType::Path,
        "number" => {
            let Some((min, r1)) = next_word(rest) else {
                return err(line, "a number needs MIN and MAX");
            };
            let Some((max, r2)) = next_word(r1) else {
                return err(line, "a number needs MIN and MAX");
            };
            let (Some(min), Some(max)) = (parse_number(min), parse_number(max)) else {
                return err(line, "MIN and MAX are numbers");
            };
            if min > max {
                return err(line, "MIN is larger than MAX");
            }
            rest = r2;
            SettingType::Number { min, max }
        }
        "choice" => {
            let Some((list, r)) = next_word(rest) else {
                return err(line, "a choice needs its values");
            };
            let mut choices = Vec::new();
            for c in list.split('|') {
                // A word, or a number (ADR-0169's `gui.scale`, `auto|1|2`).
                let number = !c.is_empty() && c.len() <= 9 && c.bytes().all(|b| b.is_ascii_digit());
                if !is_long_name(c) && !number {
                    return err(line, "a choice is lower-case words or numbers joined by |");
                }
                push(&mut choices, String::from(c), line)?;
            }
            rest = r;
            SettingType::Choice(choices)
        }
        _ => return err(line, "an unknown setting type"),
    };
    Ok((ty, rest))
}

fn check_text(text: &str) -> Result<()> {
    for (i, line) in text.split('\n').enumerate() {
        if line.len() > MAX_LINE {
            return err(i + 1, "the line is too long");
        }
        let bad = line
            .chars()
            .any(|c| (c.is_control() && c != '\t') || ('\u{80}'..='\u{9f}').contains(&c));
        if bad {
            return err(i + 1, "a control character");
        }
    }
    Ok(())
}

/// Parses a description.
pub fn parse(bytes: &[u8]) -> Result<Description> {
    if bytes.len() > MAX_SIZE {
        return err(0, "the description is larger than 64 KiB");
    }
    let text = match core::str::from_utf8(bytes) {
        Ok(t) => t,
        Err(e) => {
            let line = 1 + bytes[..e.valid_up_to()]
                .iter()
                .filter(|b| **b == b'\n')
                .count();
            return err(line, "not UTF-8");
        }
    };
    check_text(text)?;
    let mut d = Description::new(What::Command, "", "");
    let mut seen_name = false;
    let mut seen_summary = false;
    let mut seen_cwd = false;
    let mut seen_question = false;
    let mut seen_program = false;
    let mut whens_raw: Vec<(usize, String, String, Kind)> = Vec::new();
    let mut lines = text.split('\n').enumerate().map(|(i, l)| (i + 1, l));
    let mut body_start = None;
    for (number, line) in lines.by_ref() {
        if line == "---" {
            body_start = Some(number);
            break;
        }
        let Some((key, rest)) = next_word(line) else {
            continue; // a blank header line
        };
        if key.starts_with('#') {
            continue;
        }
        let once = |seen: &mut bool| -> Result<()> {
            if *seen {
                return err(number, "this key appears twice");
            }
            *seen = true;
            Ok(())
        };
        match key {
            "name" | "topic" => {
                once(&mut seen_name)?;
                let Some((name, after)) = next_word(rest) else {
                    return err(number, "the name is missing");
                };
                if !after.is_empty() || !is_command_name(name) {
                    return err(number, "a name is lower-case letters, digits and -");
                }
                d.name = name.into();
                d.what = if key == "name" {
                    What::Command
                } else {
                    What::Topic
                };
            }
            "summary" => {
                once(&mut seen_summary)?;
                d.summary = text_of(rest, number)?;
            }
            "usage" => {
                let u = text_of(rest, number)?;
                push(&mut d.usage, u, number)?;
            }
            "operand" => {
                let Some((name, rest)) = next_word(rest) else {
                    return err(number, "an operand needs a name");
                };
                let Some((name, arity)) = parse_operand_name(name) else {
                    return err(
                        number,
                        "an operand's name is upper case: NAME, [NAME], NAME...",
                    );
                };
                let Some((kind, rest)) = next_word(rest) else {
                    return err(number, "an operand needs a kind");
                };
                let Some(kind) = Kind::parse(kind) else {
                    return err(number, "an unknown kind");
                };
                if d.operand_by_name(&name).is_some() {
                    return err(number, "two operands have this name");
                }
                if let Some(last) = d.operands.last() {
                    if last.arity.several() {
                        return err(number, "no operand follows one that takes several");
                    }
                    if !last.arity.required() && arity.required() {
                        return err(number, "a required operand follows an optional one");
                    }
                }
                let operand = Operand {
                    name,
                    arity,
                    kind,
                    text: text_of(rest, number)?,
                };
                push(&mut d.operands, operand, number)?;
            }
            "option" => {
                let o = parse_option(rest, number)?;
                if d.option_by_long(&o.long).is_some()
                    || o.short.is_some_and(|c| d.option_by_short(c).is_some())
                {
                    return err(number, "two options have this name");
                }
                push(&mut d.options, o, number)?;
            }
            "when" => {
                let Some((option, r1)) = next_word(rest) else {
                    return err(number, "when needs OPTION OPERAND KIND");
                };
                let Some((operand, r2)) = next_word(r1) else {
                    return err(number, "when needs OPTION OPERAND KIND");
                };
                let Some((kind, r3)) = next_word(r2) else {
                    return err(number, "when needs OPTION OPERAND KIND");
                };
                if !r3.is_empty() {
                    return err(number, "when needs OPTION OPERAND KIND");
                }
                let Some(kind) = Kind::parse(kind) else {
                    return err(number, "an unknown kind");
                };
                let raw = (number, option.into(), operand.into(), kind);
                push(&mut whens_raw, raw, number)?;
            }
            "cwd" => {
                once(&mut seen_cwd)?;
                d.cwd = match rest.trim_end() {
                    "read" => Cwd::Read,
                    "none" => Cwd::None,
                    _ => return err(number, "cwd is read or none"),
                };
            }
            "question" => {
                once(&mut seen_question)?;
                if !rest.trim_end().is_empty() {
                    return err(number, "question takes nothing");
                }
                d.question = true;
            }
            "program" => {
                once(&mut seen_program)?;
                let Some((path, after)) = next_word(rest) else {
                    return err(number, "program needs a path");
                };
                if !after.is_empty() || !path.starts_with('/') {
                    return err(number, "program is one absolute path");
                }
                d.program = Some(path.into());
            }
            "exit" => {
                let Some((code, rest)) = next_word(rest) else {
                    return err(number, "exit needs a code");
                };
                let code = match parse_number(code) {
                    Some(c @ 0..=255) if !code.starts_with('-') => c as u8,
                    _ => return err(number, "an exit code is 0 to 255"),
                };
                if d.exits.iter().any(|e| e.code == code) {
                    return err(number, "this exit code appears twice");
                }
                let exit = Exit {
                    code,
                    text: text_of(rest, number)?,
                };
                push(&mut d.exits, exit, number)?;
            }
            "setting" => {
                let s = parse_setting(rest, number)?;
                if d.setting(&s.name).is_some() {
                    return err(number, "this setting appears twice");
                }
                push(&mut d.settings, s, number)?;
            }
            _ => return err(number, "an unknown key"),
        }
    }
    if !seen_name {
        return err(0, "a description needs a name or a topic");
    }
    if !seen_summary {
        return err(0, "a description needs a summary");
    }
    if d.what == What::Topic {
        let command_only = !d.usage.is_empty()
            || !d.operands.is_empty()
            || !d.options.is_empty()
            || !whens_raw.is_empty()
            || seen_cwd
            || seen_question
            || seen_program
            || !d.exits.is_empty()
            || !d.settings.is_empty();
        if command_only {
            return err(0, "a topic has only a summary and a body");
        }
    } else if d.usage.is_empty() {
        return err(0, "a command needs a usage line");
    }
    for (number, option, operand, kind) in whens_raw {
        let o = match (option.strip_prefix("--"), option.strip_prefix('-')) {
            (Some(long), _) => d.option_by_long(long),
            (None, Some(s)) if s.chars().count() == 1 => {
                d.option_by_short(s.chars().next().unwrap_or(' '))
            }
            _ => None,
        };
        let Some(o) = o else {
            return err(number, "when names an option the description lacks");
        };
        let Some(p) = d.operand_by_name(&operand) else {
            return err(number, "when names an operand the description lacks");
        };
        d.whens.push(When {
            option: o,
            operand: p,
            kind,
        });
    }
    if let Some(start) = body_start {
        let mut current = Section {
            title: None,
            lines: Vec::new(),
        };
        let mut order = 0;
        for (number, line) in lines {
            if is_section_title(line) {
                if d.what == What::Command {
                    let Some(at) = COMMAND_SECTIONS.iter().position(|t| *t == line) else {
                        return err(number, "a command's sections are EXAMPLES, FILES, SEE ALSO");
                    };
                    if at < order {
                        return err(
                            number,
                            "sections are in the order EXAMPLES, FILES, SEE ALSO",
                        );
                    }
                    order = at + 1;
                }
                if d.sections.iter().any(|s| s.title.as_deref() == Some(line)) {
                    return err(number, "this section appears twice");
                }
                finish(&mut d.sections, current, number)?;
                current = Section {
                    title: Some(line.into()),
                    lines: Vec::new(),
                };
            } else {
                current
                    .lines
                    .push(line.trim_end_matches([' ', '\t']).into());
            }
        }
        finish(&mut d.sections, current, start)?;
    }
    Ok(d)
}

/// Keeps a section without its blank lines at either end. An untitled one
/// that is empty is dropped.
fn finish(sections: &mut Vec<Section>, mut s: Section, line: usize) -> Result<()> {
    while s.lines.last().is_some_and(|l| l.is_empty()) {
        s.lines.pop();
    }
    let lead = s.lines.iter().take_while(|l| l.is_empty()).count();
    s.lines.drain(..lead);
    if s.title.is_none() && s.lines.is_empty() {
        return Ok(());
    }
    push(sections, s, line)
}

#[cfg(test)]
mod tests;

// A runaway test fails at 512 MiB instead of taking the host (docs/backlog.md).
#[cfg(test)]
zoraya_test_alloc::cap_memory!();
