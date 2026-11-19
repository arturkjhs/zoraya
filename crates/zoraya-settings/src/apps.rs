//! `/config/apps` (ADR-0396 item 4, ADR-0397) and its log (ADR-0399 item 5):
//! one line a program installed from a package,
//! `NAME sha256:HEX [--installed DATE] WORD...`, each `WORD` one of
//! `+r PATH`, `+w PATH`, `+net TARGET[:PORTS]`, `+web`, `+own PATH` in
//! ADR-0150's word syntax with no expansion, as ADR-0397 item 1 lists them.
//! `#` comment lines and blank lines as in `/config/settings`. The reader is
//! total: a bad line is reported once and skipped (ADR-0157 rule 5); the
//! writers keep every other byte of the file (as `rooms` does).
//!
//! This module is the one writer of ADR-0397's INV-0397-1 and INV-0399-3:
//! the shell's `install` and `programs` build the file's bytes here, and the
//! settings page of ADR-0399 item 4 calls the same functions.
//!
//! The `--installed DATE` word is the date `help NAME` shows (ADR-0396 item
//! 6) and is not part of the identity (proposed ADR-0466).

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use zoraya_describe::{Description, quote_value, split_value};

use crate::rooms::{is_grant_path, is_net_word};
use crate::sha256::{self, Digest};
use crate::{MAX_SIZE, Problem, lines};

/// The record, the package directory and the log.
pub const RECORD: &str = "/config/apps";
pub const LOG: &str = "/config/apps.log";
pub const APPS_DIR: &str = "/apps";

/// Programs one record holds; the rest are reported.
pub const MAX_APPS: usize = 64;
/// Saved words of one program.
pub const MAX_WORDS: usize = 16;
/// The longest program name: the description's own limit, which the
/// package's `NAME.cmd` has to carry.
pub const MAX_NAME: usize = zoraya_describe::MAX_NAME;
/// The log's cap in bytes (ADR-0399 item 5): the oldest lines go first.
pub const LOG_CAP: usize = 64 * 1024;
/// The largest description or request file an install reads.
pub const MAX_PACKAGE_TEXT: usize = zoraya_describe::MAX_SIZE;

/// One saved word (ADR-0397 item 1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Word {
    /// `+r DIR`.
    Read(String),
    /// `+w DIR`.
    Write(String),
    /// `+net TARGET[:PORTS]`, as written after `+net`.
    Net(String),
    /// `+web`: the web rule of ADR-0324.
    Web,
    /// `+own PATH`: the program's own directory seen at `PATH`.
    Own(String),
}

impl Word {
    /// In ADR-0153's words: `+w ~/notes`, `+web`.
    pub fn words(&self) -> String {
        match self {
            Word::Read(p) => format!("+r {}", quote_value(p)),
            Word::Write(p) => format!("+w {}", quote_value(p)),
            Word::Net(t) => format!("+net {t}"),
            Word::Web => String::from("+web"),
            Word::Own(p) => format!("+own {}", quote_value(p)),
        }
    }

    /// The word's first token: `+w`.
    pub fn token(&self) -> &'static str {
        match self {
            Word::Read(_) => "+r",
            Word::Write(_) => "+w",
            Word::Net(_) => "+net",
            Word::Web => "+web",
            Word::Own(_) => "+own",
        }
    }
}

/// One installed program's record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct App {
    pub name: String,
    /// [`sha256::identity`] of its program and its description.
    pub hash: Digest,
    /// `YYYY-MM-DD`, or empty.
    pub installed: String,
    pub words: Vec<Word>,
}

impl App {
    /// The first 8 hex digits, as `help` and `programs` show it.
    pub fn short_hash(&self) -> String {
        sha256::hex(&self.hash)[..8].to_string()
    }

    /// The record's line as the writers write it.
    pub fn line(&self) -> String {
        let mut s = format!("{} sha256:{}", self.name, sha256::hex(&self.hash));
        if !self.installed.is_empty() {
            s.push_str(" --installed ");
            s.push_str(&self.installed);
        }
        for w in &self.words {
            s.push(' ');
            s.push_str(&w.words());
        }
        s
    }
}

/// A program name: a lower-case letter, then lower-case letters, digits and
/// `-`, at most [`MAX_NAME`] bytes (the form a description's `name` takes,
/// ADR-0156, which `NAME.cmd` must carry).
pub fn is_app_name(name: &str) -> bool {
    let b = name.as_bytes();
    !b.is_empty()
        && b.len() <= MAX_NAME
        && b[0].is_ascii_lowercase()
        && b.iter()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'-')
}

/// A date as `YYYY-MM-DD`.
fn is_date(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 10
        && b.iter().enumerate().all(|(i, c)| {
            if i == 4 || i == 7 {
                *c == b'-'
            } else {
                c.is_ascii_digit()
            }
        })
}

/// What the reader gives: the programs in file order and the problems.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Apps {
    pub apps: Vec<App>,
    pub problems: Vec<Problem>,
}

impl Apps {
    pub fn get(&self, name: &str) -> Option<&App> {
        self.apps.iter().find(|a| a.name == name)
    }
}

/// The words of one line, or a problem.
fn split_words(mut s: &str) -> Result<Vec<String>, &'static str> {
    let mut out = Vec::new();
    loop {
        s = s.trim_start_matches([' ', '\t']);
        if s.is_empty() || s.starts_with('#') {
            return Ok(out);
        }
        let (w, rest) = split_value(s)?;
        out.push(w);
        if out.len() > 3 + 2 * MAX_WORDS {
            return Err("too many words");
        }
        s = rest;
    }
}

/// The saved words in `tokens` (ADR-0397 item 1): `+r PATH`, `+w PATH`,
/// `+net TARGET`, `+web`, `+own PATH`. Nothing else is saved: `+listen`, a
/// file, an argument, an environment variable are refused with their text.
/// Only the syntax of each word is read here; [`check_word`] says what may
/// be saved.
pub fn parse_words(tokens: &[String]) -> Result<Vec<Word>, String> {
    let mut out: Vec<Word> = Vec::new();
    let mut i = 0;
    while i < tokens.len() {
        let t = tokens[i].as_str();
        let arg = |i: usize| -> Result<&str, String> {
            tokens
                .get(i + 1)
                .map(String::as_str)
                .ok_or_else(|| format!("{t} needs a word after it"))
        };
        let (word, used) = match t {
            "+r" | "+w" | "+own" => {
                let p = arg(i)?;
                if !is_grant_path(p) {
                    return Err(format!(
                        "{}: not a path: `~`, `~/...` or `/...`, no `.` or `..` part",
                        quote_value(p)
                    ));
                }
                let w = match t {
                    "+r" => Word::Read(p.into()),
                    "+w" => Word::Write(p.into()),
                    _ => Word::Own(p.into()),
                };
                (w, 2)
            }
            "+net" => {
                let n = arg(i)?;
                if !is_net_word(n) {
                    return Err(format!("+net {}: not a target", quote_value(n)));
                }
                (Word::Net(n.into()), 2)
            }
            "+web" => (Word::Web, 1),
            "+listen" => {
                return Err(String::from(
                    "+listen is not saved: type it on the line (ADR-0397 item 1)",
                ));
            }
            other => {
                return Err(format!(
                    "{}: not a word that can be saved; saved words are +r, +w, +net, +web and +own",
                    quote_value(other)
                ));
            }
        };
        if !out.contains(&word) {
            out.push(word);
        }
        i += used;
    }
    if out.len() > MAX_WORDS {
        return Err(format!("more than {MAX_WORDS} saved words"));
    }
    Ok(out)
}

/// What a check of a saved word needs from the shell.
pub struct Ctx<'a> {
    /// The shell's `~` (`shell.home`).
    pub home: &'a str,
    /// Whether the absolute path names something that is not a directory
    /// (a file cannot be saved, ADR-0397 item 1).
    pub is_file: &'a dyn Fn(&str) -> bool,
    /// Whether a `+net` word parses and is not `any` (the shell's
    /// `netgrant::parse_word`; ADR-0397 item 3).
    pub net: &'a dyn Fn(&str) -> Result<(), String>,
}

/// A path with `~` read as `home`.
pub fn expand(path: &str, home: &str) -> String {
    if path == "~" {
        home.into()
    } else if let Some(rest) = path.strip_prefix("~/") {
        format!("{}/{rest}", home.trim_end_matches('/'))
    } else {
        path.into()
    }
}

/// Why `abs` can never be a saved `+r` or `+w` (ADR-0397 item 3): the volume
/// root, `/config`, `/apps`, `/data` and everything below the last three
/// (the room directories, `/data/home`, another program's own directory).
fn refused_place(abs: &str) -> Option<&'static str> {
    let abs = abs.trim_end_matches('/');
    if abs.is_empty() {
        return Some("the whole volume");
    }
    let under = |dir: &str| abs == dir || abs.strip_prefix(dir).is_some_and(|r| r.starts_with('/'));
    if under("/config") {
        Some("the person's configuration")
    } else if under("/apps") {
        Some("the installed programs")
    } else if under("/data") {
        Some("programs' own directories")
    } else {
        None
    }
}

/// Whether the word is the person's home or the volume root with `+w`,
/// which the printed form calls `everything` (ADR-0397 item 3).
pub fn is_everything(w: &Word, home: &str) -> bool {
    match w {
        Word::Write(p) => expand(p, home).trim_end_matches('/') == home.trim_end_matches('/'),
        _ => false,
    }
}

/// `install` and `programs grant` refuse these (ADR-0397 item 3, INV-0397-4):
/// the places of item 3, a file, `+net any`, a `+own` that is not a plain
/// path under `~/`. A refusal names the word.
pub fn check_word(w: &Word, ctx: &Ctx<'_>) -> Result<(), String> {
    match w {
        Word::Read(p) | Word::Write(p) => {
            let abs = expand(p, ctx.home);
            if p == "~" && ctx.home.trim_end_matches('/').is_empty() {
                return Err(format!(
                    "{}: {} is the volume root here",
                    w.words(),
                    quote_value(p)
                ));
            }
            if let Some(what) = refused_place(&abs) {
                return Err(format!("{}: {what} cannot be saved", w.words()));
            }
            if (ctx.is_file)(&abs) {
                return Err(format!(
                    "{}: a file cannot be saved; type it on the line",
                    w.words()
                ));
            }
            Ok(())
        }
        Word::Net(t) => (ctx.net)(t).map_err(|why| format!("+net {t}: {why}")),
        Word::Web => Ok(()),
        Word::Own(p) => {
            if zoraya_describe::is_data_path(p) {
                Ok(())
            } else {
                Err(format!(
                    "+own {}: a path under ~/ of plain names (letters, digits and . _ -)",
                    quote_value(p)
                ))
            }
        }
    }
}

/// One line's program; `Ok(None)` for a blank or comment line.
fn parse(text: &str) -> Result<Option<App>, String> {
    let t = text.trim_matches([' ', '\t']);
    if t.is_empty() || t.starts_with('#') {
        return Ok(None);
    }
    let w = split_words(t).map_err(String::from)?;
    let name = &w[0];
    if !is_app_name(name) {
        return Err(format!(
            "{}: not a program name: a lower-case letter, then letters, digits or -",
            quote_value(name)
        ));
    }
    let Some(hash) = w
        .get(1)
        .and_then(|h| h.strip_prefix("sha256:"))
        .and_then(sha256::from_hex)
    else {
        return Err(String::from("no sha256:HEX of 64 digits after the name"));
    };
    let mut i = 2;
    let mut installed = String::new();
    if w.get(i).map(String::as_str) == Some("--installed") {
        let Some(d) = w.get(i + 1).filter(|d| is_date(d)) else {
            return Err(String::from("--installed needs a date, YYYY-MM-DD"));
        };
        installed = d.clone();
        i += 2;
    }
    let words = parse_words(&w[i..])?;
    Ok(Some(App {
        name: name.clone(),
        hash,
        installed,
        words,
    }))
}

/// Reads `/config/apps`. Total: any bytes give programs and problems; a bad
/// line, a second line of one name and the lines past [`MAX_APPS`] are
/// reported and skipped.
pub fn read(bytes: &[u8]) -> Apps {
    let mut out = Apps::default();
    let cut = bytes.len() > MAX_SIZE;
    let bytes = if cut {
        let head = &bytes[..MAX_SIZE];
        &head[..head.iter().rposition(|b| *b == b'\n').map_or(0, |i| i + 1)]
    } else {
        bytes
    };
    if cut {
        out.problems.push(Problem {
            line: 1 + bytes.iter().filter(|b| **b == b'\n').count(),
            problem: String::from("the file is longer than 256 KiB; the rest is ignored"),
        });
    }
    for (line, text) in lines(bytes) {
        let problem = match text.map_err(String::from).and_then(parse) {
            Ok(None) => continue,
            Ok(Some(app)) if out.get(&app.name).is_some() => {
                format!("{}: a second line; the first counts", app.name)
            }
            Ok(Some(_)) if out.apps.len() == MAX_APPS => {
                format!("more than {MAX_APPS} programs; the rest are ignored")
            }
            Ok(Some(app)) => {
                out.apps.push(app);
                continue;
            }
            Err(p) => p,
        };
        out.problems.push(Problem { line, problem });
    }
    out
}

/// Why a writer changed nothing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    BadName,
    TooMany,
    NoSuchApp,
    /// A saved word the record does not hold, on a revoke.
    NotSaved(String),
}

fn is_it(raw: &[u8], name: &str) -> bool {
    let raw = raw.strip_suffix(b"\r").unwrap_or(raw);
    core::str::from_utf8(raw)
        .ok()
        .and_then(|t| parse(t).ok().flatten())
        .is_some_and(|a| a.name == name)
}

/// The file with `app`'s line in place of the first line of its name, or
/// added as the last line; every other byte kept.
pub fn put(bytes: &[u8], app: &App) -> Result<Vec<u8>, Refusal> {
    if !is_app_name(&app.name) {
        return Err(Refusal::BadName);
    }
    let have = read(bytes);
    let line = app.line();
    if have.get(&app.name).is_none() {
        if have.apps.len() >= MAX_APPS || app.words.len() > MAX_WORDS {
            return Err(Refusal::TooMany);
        }
        let mut out = bytes.to_vec();
        if !out.is_empty() && !out.ends_with(b"\n") {
            out.push(b'\n');
        }
        out.extend_from_slice(line.as_bytes());
        out.push(b'\n');
        return Ok(out);
    }
    if app.words.len() > MAX_WORDS {
        return Err(Refusal::TooMany);
    }
    let parts: Vec<&[u8]> = bytes.split(|b| *b == b'\n').collect();
    let at = parts.iter().position(|p| is_it(p, &app.name));
    let mut out = Vec::with_capacity(bytes.len() + line.len());
    for (i, p) in parts.iter().enumerate() {
        if i > 0 {
            out.push(b'\n');
        }
        if Some(i) == at {
            out.extend_from_slice(line.as_bytes());
        } else {
            out.extend_from_slice(p);
        }
    }
    Ok(out)
}

/// The file without `name`'s lines; every other byte kept.
pub fn remove(bytes: &[u8], name: &str) -> Result<Vec<u8>, Refusal> {
    let parts: Vec<&[u8]> = bytes.split(|b| *b == b'\n').collect();
    if !parts.iter().any(|p| is_it(p, name)) {
        return Err(Refusal::NoSuchApp);
    }
    let kept: Vec<&[u8]> = parts.iter().filter(|p| !is_it(p, name)).copied().collect();
    Ok(kept.join(&b'\n'))
}

/// `programs revoke NAME WORD...`: the file with exactly `words` taken from
/// `name`'s line, and the words removed (INV-0399-1). A word the line does
/// not have refuses the whole revoke and changes nothing.
pub fn revoke(bytes: &[u8], name: &str, words: &[Word]) -> Result<(Vec<u8>, Vec<Word>), Refusal> {
    let mut app = read(bytes).get(name).cloned().ok_or(Refusal::NoSuchApp)?;
    for w in words {
        if !app.words.contains(w) {
            return Err(Refusal::NotSaved(w.words()));
        }
    }
    let (gone, kept): (Vec<Word>, Vec<Word>) =
        app.words.iter().cloned().partition(|w| words.contains(w));
    app.words = kept;
    Ok((put(bytes, &app)?, gone))
}

/// `programs revoke NAME --all`.
pub fn revoke_all(bytes: &[u8], name: &str) -> Result<(Vec<u8>, Vec<Word>), Refusal> {
    let mut app = read(bytes).get(name).cloned().ok_or(Refusal::NoSuchApp)?;
    let gone = core::mem::take(&mut app.words);
    Ok((put(bytes, &app)?, gone))
}

// --- The package and what `install` checks (ADR-0396 items 1, 5 and 7) ---

/// What `install` finds in a package directory, read by the shell.
pub struct Package<'a> {
    /// The name the person gave, or the directory's.
    pub name: &'a str,
    /// The first bytes of `NAME` (4 are enough), `None` for no such file.
    pub program_head: Option<&'a [u8]>,
    /// `NAME.cmd`, `None` for no such file.
    pub description: Option<&'a [u8]>,
    /// `request`, `None` for no such file.
    pub request: Option<&'a [u8]>,
}

/// Why `install` refuses a package (INV-0396-3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reject {
    BadName,
    /// An image command, a builtin, a function in the shell, a room, a name
    /// a program's own directory has, a program of `shell.path`, or an
    /// installed program a stray directory under `/apps` already holds.
    NameTaken(&'static str),
    NoProgram,
    NotAProgram,
    NoDescription,
    /// The parser's refusal, with its line.
    Description(usize, String),
    /// A key outside [`zoraya_describe::VOLUME_KEYS`], with its line.
    Key(usize, String),
    /// The description's `name` is not the package's.
    NameDiffers(String),
}

impl Reject {
    /// The line `install` prints after `zosh: install: `.
    pub fn text(&self, name: &str) -> String {
        match self {
            Reject::BadName => format!(
                "{}: not a program name: a lower-case letter, then letters, digits or -, at most {MAX_NAME}",
                quote_value(name)
            ),
            Reject::NameTaken(what) => format!("{name}: {what} has that name"),
            Reject::NoProgram => format!("{name}: the package has no program file {name}"),
            Reject::NotAProgram => {
                format!("{name}: not a program: an ELF file or a script that begins with #!")
            }
            Reject::NoDescription => format!(
                "{name}: the package has no {name}.cmd: every command of a package has a description"
            ),
            Reject::Description(line, why) => format!("{name}.cmd line {line}: {why}"),
            Reject::Key(line, key) => format!(
                "{name}.cmd line {line}: `{key}` is not allowed in an installed program's description; it gives help and completion, not access"
            ),
            Reject::NameDiffers(found) => {
                format!("{name}.cmd: its name is `{found}`, the package's is `{name}`")
            }
        }
    }
}

/// What a good package gives.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Checked {
    /// The description as the shell reads it: every kind `text`, nothing
    /// that gives authority (INV-0396-2).
    pub description: Description,
    /// What the publisher asks for, from `request`: shown, never applied.
    pub asked: Vec<Word>,
}

/// The words of a package's `request` file: one grant a line, in the syntax
/// of [`parse_words`]. A line that does not parse is left out; the file is
/// shown only as the words that did (hostile text, ADR-0337 item 2).
pub fn request_words(bytes: &[u8]) -> Vec<Word> {
    let mut out: Vec<Word> = Vec::new();
    for (_, text) in lines(&bytes[..bytes.len().min(MAX_PACKAGE_TEXT)]) {
        let Ok(text) = text else { continue };
        let Ok(tokens) = split_words(text.trim()) else {
            continue;
        };
        if tokens.is_empty() {
            continue;
        }
        if let Ok(words) = parse_words(&tokens) {
            for w in words {
                if !out.contains(&w) && out.len() < MAX_WORDS {
                    out.push(w);
                }
            }
        }
    }
    out
}

/// What `install` can check without the program's source (ADR-0396 item 7).
/// `taken` names what already has the name (an image command, a builtin, a
/// function, a room, ...), `None` when nothing does. `stray` is true when
/// `/apps/NAME` exists and the record has no line for `name`: a directory
/// no `install` made, which a new package must not take over. A name the
/// record holds is an upgrade and is the caller's to allow (it is not
/// `taken`).
pub fn check_package(
    p: &Package<'_>,
    taken: &dyn Fn(&str) -> Option<&'static str>,
    stray: bool,
) -> Result<Checked, Reject> {
    if !is_app_name(p.name) {
        return Err(Reject::BadName);
    }
    if let Some(what) = taken(p.name) {
        return Err(Reject::NameTaken(what));
    }
    if stray {
        return Err(Reject::NameTaken("another package"));
    }
    let Some(head) = p.program_head else {
        return Err(Reject::NoProgram);
    };
    if !(head.starts_with(b"\x7fELF") || head.starts_with(b"#!")) {
        return Err(Reject::NotAProgram);
    }
    let Some(text) = p.description else {
        return Err(Reject::NoDescription);
    };
    if let Some((line, key)) = zoraya_describe::volume_key_problem(text) {
        return Err(Reject::Key(line, key));
    }
    let d = zoraya_describe::parse(text)
        .map_err(|e| Reject::Description(e.line, e.problem.to_string()))?;
    if d.name != p.name || d.what != zoraya_describe::What::Command {
        return Err(Reject::NameDiffers(d.name));
    }
    Ok(Checked {
        description: zoraya_describe::for_volume(d),
        asked: p.request.map(request_words).unwrap_or_default(),
    })
}

// --- What `programs` prints (ADR-0399 item 1) ---

/// `programs` with no word: a row a program.
pub fn table(
    apps: &Apps,
    running: &dyn Fn(&str) -> usize,
    changed: &dyn Fn(&str) -> bool,
) -> String {
    if apps.apps.is_empty() {
        return String::from("no installed programs\n");
    }
    let width = apps
        .apps
        .iter()
        .map(|a| a.name.len())
        .max()
        .unwrap_or(4)
        .max(4);
    let mut s = format!(
        "{:<width$}  {:<8}  {:<10}  {:>5}  {}\n",
        "NAME", "HASH", "INSTALLED", "SAVED", "RUNNING"
    );
    for a in &apps.apps {
        let date = if a.installed.is_empty() {
            "-"
        } else {
            &a.installed
        };
        s.push_str(&format!(
            "{:<width$}  {:<8}  {:<10}  {:>5}  {:>7}{}\n",
            a.name,
            a.short_hash(),
            date,
            a.words.len(),
            running(&a.name),
            if changed(&a.name) { "  changed" } else { "" },
        ));
    }
    s
}

/// What each saved word means, as `grants` and `programs NAME` say it.
pub fn meaning(w: &Word, home: &str) -> String {
    let shown = |p: &str| {
        let abs = expand(p, home);
        if abs == home {
            String::from("~")
        } else if let Some(r) = abs.strip_prefix(&format!("{}/", home.trim_end_matches('/'))) {
            format!("~/{r}")
        } else {
            abs
        }
    };
    match w {
        Word::Read(p) => format!("{}  read", shown(p)),
        Word::Write(p) if is_everything(w, home) => {
            format!("{}  change everything in (all of your files)", shown(p))
        }
        Word::Write(p) => format!("{}  change anything in", shown(p)),
        Word::Net(t) => format!("net: {t}  reach"),
        Word::Web => String::from("net: web  public hosts, ports 80 and 443"),
        Word::Own(p) => format!("{}  its own files", shown(p)),
    }
}

/// `help NAME`'s first line (ADR-0396 item 6).
pub fn help_head(a: &App) -> String {
    let date = if a.installed.is_empty() {
        String::new()
    } else {
        format!(", installed {}", a.installed)
    };
    format!(
        "{n} is an installed program, {dir}/{n}/{n} (sha256 {h}{date})",
        n = a.name,
        dir = APPS_DIR,
        h = a.short_hash()
    )
}

// --- The log (ADR-0399 item 5, INV-0399-5) ---

/// One log line: `2026-11-15T10:02:11 install ed sha256:1a2b3c4d +own +w`.
pub fn log_line(time: &str, verb: &str, name: &str, detail: &str) -> String {
    if detail.is_empty() {
        format!("{time} {verb} {name}")
    } else {
        format!("{time} {verb} {name} {detail}")
    }
}

/// The log with `line` appended, under [`LOG_CAP`]: the oldest whole lines
/// are dropped until it fits. Every line ends with a newline.
pub fn log_append(log: &[u8], line: &str) -> Vec<u8> {
    let mut out = log.to_vec();
    if !out.is_empty() && !out.ends_with(b"\n") {
        out.push(b'\n');
    }
    let line = &line.as_bytes()[..line.len().min(1024)];
    out.extend_from_slice(line);
    out.push(b'\n');
    if out.len() > LOG_CAP {
        let drop = out.len() - LOG_CAP;
        // The first newline at or after `drop - 1` ends the last line that
        // goes; what follows is whole lines.
        let at = out[drop - 1..]
            .iter()
            .position(|b| *b == b'\n')
            .map_or(out.len(), |i| drop + i);
        out.drain(..at);
    }
    out
}

/// `programs --log [NAME]`: the lines about `name`, or all.
pub fn log_view(log: &[u8], name: Option<&str>) -> String {
    let mut s = String::new();
    for (_, text) in lines(log) {
        let Ok(text) = text else { continue };
        if text.is_empty() {
            continue;
        }
        if name.is_none_or(|n| text.split(' ').nth(2) == Some(n)) {
            s.push_str(text);
            s.push('\n');
        }
    }
    s
}

// --- Instances (ADR-0399 item 2, INV-0399-2) ---

/// One program of a job the shell started: its program name and whether it
/// is in this shell's own job tree (a shell of another room is not).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Instance {
    pub job: u32,
    pub program: String,
    pub here: bool,
}

/// The jobs `--kill` ends and the number it can only name: the instances of
/// `name` in the shell's own tree, and those of the same name elsewhere.
pub fn instances_of(all: &[Instance], name: &str) -> (Vec<u32>, usize) {
    let mut here: Vec<u32> = all
        .iter()
        .filter(|i| i.program == name && i.here)
        .map(|i| i.job)
        .collect();
    here.sort_unstable();
    here.dedup();
    let elsewhere = all.iter().filter(|i| i.program == name && !i.here).count();
    (here, elsewhere)
}
