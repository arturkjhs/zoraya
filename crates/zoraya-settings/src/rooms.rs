//! `/config/rooms` (ADR-0172 point 1, ADR-0266 point 8): one room a line,
//! `NAME [--untrusted] [--colour N] [--mem SIZE] [--cpu PCT] [--procs N]
//! [--disk SIZE] [--conns N] [--tmp SIZE] GRANT...`, each `GRANT` the two words
//! `+r PATH` or `+w PATH` as ADR-0153 rule 3 writes them, in ADR-0150's word
//! syntax with no expansion. `#` comment lines and blank lines as in
//! `/config/settings`. The reader is total: a bad line is reported once and
//! skipped (ADR-0157 rule 5). `home` has no line: it is the session's own.

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use zoraya_describe::{quote_value, split_value};

use crate::{MAX_SIZE, Problem, lines};

/// The trusted colour slots, `room.1` to `room.4` (ADR-0172 point 7).
pub const SLOTS: u8 = 4;
/// Rooms read from one file; the rest are reported.
pub const MAX_ROOMS: usize = 32;
/// Grants of one room.
pub const MAX_GRANTS: usize = 16;
/// The longest room name, in bytes.
pub const MAX_NAME: usize = 24;
/// The longest path of a grant, in bytes.
pub const MAX_PATH: usize = 200;

/// The room every session has, with no line and no colour.
pub const HOME: &str = "home";

/// `+r PATH` or `+w PATH`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Grant {
    pub write: bool,
    /// `~`, `~/...` or `/...`, with no `.` or `..` component.
    pub path: String,
}

impl Grant {
    /// The grant in ADR-0153's words: `+w ~/project`.
    pub fn words(&self) -> String {
        format!(
            "{} {}",
            if self.write { "+w" } else { "+r" },
            quote_value(&self.path)
        )
    }
}

/// A room's six limit words (ADR-0266 point 1). `None` is a word the line
/// does not carry: no limit of that kind for a trusted room, a word to be
/// filled by [`Limits::fill_defaults`] for an untrusted one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Limits {
    /// `--mem`, bytes.
    pub mem: Option<u64>,
    /// `--cpu`, percent of the machine, 1 to 100.
    pub cpu: Option<u32>,
    /// `--procs`.
    pub procs: Option<u32>,
    /// `--disk`, bytes, on each `+w` directory of the room.
    pub disk: Option<u64>,
    /// `--conns`.
    pub conns: Option<u32>,
    /// `--tmp`, bytes (untrusted rooms only).
    pub tmp: Option<u64>,
}

/// The six words, in the order of the line.
pub const LIMIT_WORDS: [&str; 6] = ["--mem", "--cpu", "--procs", "--disk", "--conns", "--tmp"];

const KIB: u64 = 1 << 10;
const MIB: u64 = 1 << 20;
const GIB: u64 = 1 << 30;

/// `512M`, `2G`, `64K`: a positive number and a 1024-based unit.
pub fn parse_size(text: &str) -> Option<u64> {
    let unit = match text.as_bytes().last()? {
        b'K' => KIB,
        b'M' => MIB,
        b'G' => GIB,
        _ => return None,
    };
    let digits = &text[..text.len() - 1];
    if digits.is_empty() || digits.len() > 12 || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let n: u64 = digits.parse().ok()?;
    if n == 0 {
        return None;
    }
    n.checked_mul(unit)
}

/// The size in the largest unit that holds it exactly: `2G`, `89M`, `5000K`.
pub fn format_size(bytes: u64) -> String {
    if bytes != 0 && bytes.is_multiple_of(GIB) {
        format!("{}G", bytes / GIB)
    } else if bytes != 0 && bytes.is_multiple_of(MIB) {
        format!("{}M", bytes / MIB)
    } else {
        format!("{}K", bytes.div_ceil(KIB))
    }
}

/// What a room may not pass: its parent job's ceilings (ADR-0266 point 5).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ceilings {
    pub mem: u64,
    pub procs: u32,
    /// Percent.
    pub cpu: u32,
    /// For the message: `the untrusted rooms'` or `the session's`.
    pub whose: &'static str,
}

/// The numbers the defaults and the ceilings come from (ADR-0266 points 3
/// and 4), as the room keeper reads them from the session job and the
/// volume.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Machine {
    /// The session job's memory ceiling.
    pub session_mem: u64,
    /// The session job's process ceiling.
    pub session_procs: u32,
    /// Free space of the volume, bytes.
    pub free_disk: u64,
}

impl Machine {
    /// The untrusted job: half of the session's memory, 96 processes, 50%.
    pub fn untrusted(&self) -> Ceilings {
        Ceilings {
            mem: self.session_mem / 2,
            procs: UNTRUSTED_PROCS.min(self.session_procs),
            cpu: 50,
            whose: "the untrusted rooms'",
        }
    }

    pub fn session(&self) -> Ceilings {
        Ceilings {
            mem: self.session_mem,
            procs: self.session_procs,
            cpu: 100,
            whose: "the session's",
        }
    }

    /// The ceilings a room of this trust may not pass.
    pub fn ceilings(&self, untrusted: bool) -> Ceilings {
        if untrusted {
            self.untrusted()
        } else {
            self.session()
        }
    }
}

/// The untrusted job's process ceiling (ADR-0266 point 4).
pub const UNTRUSTED_PROCS: u32 = 96;

impl Limits {
    /// The words as the line writes them, those that are set, in order:
    /// `--mem 89M --cpu 25%`. Empty when none is.
    pub fn words(&self) -> String {
        let mut out = String::new();
        let mut put = |w: &str, v: String| {
            if !out.is_empty() {
                out.push(' ');
            }
            out.push_str(w);
            out.push(' ');
            out.push_str(&v);
        };
        if let Some(v) = self.mem {
            put("--mem", format_size(v));
        }
        if let Some(v) = self.cpu {
            put("--cpu", format!("{v}%"));
        }
        if let Some(v) = self.procs {
            put("--procs", format!("{v}"));
        }
        if let Some(v) = self.disk {
            put("--disk", format_size(v));
        }
        if let Some(v) = self.conns {
            put("--conns", format!("{v}"));
        }
        if let Some(v) = self.tmp {
            put("--tmp", format_size(v));
        }
        out
    }

    /// True when a word is set that only a job can enforce (`--mem`, `--cpu`,
    /// `--procs`, `--conns`, `--tmp`): a room with one is not entered without
    /// the launch job of the room keeper. `--disk` is the volume's.
    pub fn needs_job(&self) -> bool {
        self.mem.is_some()
            || self.cpu.is_some()
            || self.procs.is_some()
            || self.conns.is_some()
            || self.tmp.is_some()
    }

    /// True when all six words are set (an untrusted room's line after any
    /// `room new` or `room set`, INV-0266-1).
    pub fn is_complete(&self) -> bool {
        self.mem.is_some()
            && self.cpu.is_some()
            && self.procs.is_some()
            && self.disk.is_some()
            && self.conns.is_some()
            && self.tmp.is_some()
    }

    /// Fills each word an untrusted room's line lacks (ADR-0266 point 3).
    /// `[assumption, every value]` until measured.
    pub fn fill_defaults(&mut self, m: &Machine) {
        let mem = *self.mem.get_or_insert_with(|| {
            let whole = (m.session_mem / 4 / MIB).max(1);
            whole * MIB
        });
        self.cpu.get_or_insert(25);
        self.procs.get_or_insert(32);
        self.disk.get_or_insert_with(|| {
            let eighth = (m.free_disk / 8 / KIB).max(1) * KIB;
            eighth.min(GIB)
        });
        self.conns.get_or_insert(16);
        self.tmp
            .get_or_insert_with(|| (16 * MIB).min(((mem / 4) / MIB).max(1) * MIB));
    }

    /// Sets one word from its text (`--mem 512M`). `none` removes a word of
    /// a trusted room; an untrusted room refuses it (ADR-0266 points 3, 6).
    pub fn set(&mut self, word: &str, value: &str, untrusted: bool) -> Result<(), String> {
        if value == "none" {
            if untrusted {
                return Err(format!("{word} none: an untrusted room always has all six"));
            }
            match word {
                "--mem" => self.mem = None,
                "--cpu" => self.cpu = None,
                "--procs" => self.procs = None,
                "--disk" => self.disk = None,
                "--conns" => self.conns = None,
                "--tmp" => self.tmp = None,
                _ => return Err(format!("{word}: not a limit word")),
            }
            return Ok(());
        }
        let size = |v: &str| {
            parse_size(v).ok_or_else(|| format!("{word} {v}: a size is a number and K, M or G"))
        };
        let count = |v: &str| {
            v.parse::<u32>()
                .ok()
                .filter(|n| (1..=1_000_000).contains(n))
                .ok_or_else(|| format!("{word} {v}: a number from 1"))
        };
        match word {
            "--mem" => self.mem = Some(size(value)?),
            "--disk" => self.disk = Some(size(value)?),
            "--tmp" if untrusted => self.tmp = Some(size(value)?),
            "--tmp" => return Err(String::from("--tmp is for untrusted rooms only")),
            "--procs" => self.procs = Some(count(value)?),
            "--conns" => self.conns = Some(count(value)?),
            "--cpu" => {
                let n = value
                    .strip_suffix('%')
                    .and_then(|d| d.parse::<u32>().ok())
                    .filter(|n| (1..=100).contains(n))
                    .ok_or_else(|| format!("{word} {value}: 1% to 100%"))?;
                self.cpu = Some(n);
            }
            _ => return Err(format!("{word}: not a limit word")),
        }
        Ok(())
    }

    /// Refuses a limit above the parent's (ADR-0266 point 5): the text is
    /// `--mem 512M: above the untrusted rooms' 178M`.
    pub fn check_within(&self, c: &Ceilings) -> Result<(), String> {
        if let Some(v) = self.mem.filter(|v| *v > c.mem) {
            return Err(format!(
                "--mem {}: above {} {}",
                format_size(v),
                c.whose,
                format_size(c.mem)
            ));
        }
        if let Some(v) = self.procs.filter(|v| *v > c.procs) {
            return Err(format!("--procs {v}: above {} {}", c.whose, c.procs));
        }
        if let Some(v) = self.cpu.filter(|v| *v > c.cpu) {
            return Err(format!("--cpu {v}%: above {} {}%", c.whose, c.cpu));
        }
        Ok(())
    }
}

/// One room's record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Room {
    pub name: String,
    pub untrusted: bool,
    /// 1 to [`SLOTS`] for a trusted room; 0 for an untrusted one, whose
    /// colour is `room.untrusted` and no other room's.
    pub colour: u8,
    /// The limit words of the line (ADR-0266).
    pub limits: Limits,
    pub grants: Vec<Grant>,
}

impl Room {
    /// The room's line as the writer writes it.
    pub fn line(&self) -> String {
        let mut s = String::from(self.name.as_str());
        if self.untrusted {
            s.push_str(" --untrusted");
        } else {
            s.push_str(&format!(" --colour {}", self.colour));
        }
        let limits = self.limits.words();
        if !limits.is_empty() {
            s.push(' ');
            s.push_str(&limits);
        }
        for g in &self.grants {
            s.push(' ');
            s.push_str(&g.words());
        }
        s
    }
}

/// A room name: a lower-case ASCII letter, then letters, digits and `-`, at
/// most [`MAX_NAME`] bytes; never `home`.
pub fn is_room_name(name: &str) -> bool {
    let b = name.as_bytes();
    !b.is_empty()
        && b.len() <= MAX_NAME
        && b[0].is_ascii_lowercase()
        && b.iter()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'-')
        && name != HOME
}

/// A grant's path: `~`, `~/...` or absolute, no empty, `.` or `..`
/// component past the first, no control character.
pub fn is_grant_path(path: &str) -> bool {
    if path.is_empty() || path.len() > MAX_PATH || path.chars().any(char::is_control) {
        return false;
    }
    let rest = if path == "~" {
        return true;
    } else if let Some(r) = path.strip_prefix("~/") {
        r
    } else if let Some(r) = path.strip_prefix('/') {
        if r.is_empty() {
            return true;
        }
        r
    } else {
        return false;
    };
    rest.split('/')
        .all(|c| !c.is_empty() && c != "." && c != "..")
}

/// What the reader gives: the rooms in file order and the problems.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Rooms {
    pub rooms: Vec<Room>,
    pub problems: Vec<Problem>,
}

impl Rooms {
    pub fn get(&self, name: &str) -> Option<&Room> {
        self.rooms.iter().find(|r| r.name == name)
    }

    /// The lowest trusted slot no room has; when all are taken, the slot
    /// the fewest rooms have, the lowest of those (open question of
    /// ADR-0172: more than four rooms repeat a colour).
    pub fn free_colour(&self) -> u8 {
        let mut count = [0usize; SLOTS as usize];
        for r in self.rooms.iter().filter(|r| !r.untrusted) {
            count[usize::from(r.colour - 1)] += 1;
        }
        let least = *count.iter().min().unwrap_or(&0);
        let i = count.iter().position(|c| *c == least).unwrap_or(0);
        i as u8 + 1
    }
}

/// The words of one line, or a problem.
fn words(mut s: &str) -> Result<Vec<String>, &'static str> {
    let mut out = Vec::new();
    loop {
        s = s.trim_start_matches([' ', '\t']);
        if s.is_empty() || s.starts_with('#') {
            return Ok(out);
        }
        let (w, rest) = split_value(s)?;
        out.push(w);
        if out.len() > 4 + 2 * (MAX_GRANTS + LIMIT_WORDS.len()) {
            return Err("too many grants");
        }
        s = rest;
    }
}

/// One line's room; the colour of a trusted line without `--colour` is 0
/// here and given by [`read`].
fn parse(text: &str) -> Result<Option<Room>, String> {
    let t = text.trim_matches([' ', '\t']);
    if t.is_empty() || t.starts_with('#') {
        return Ok(None);
    }
    let w = words(t).map_err(String::from)?;
    let name = &w[0];
    if !is_room_name(name) {
        return Err(format!(
            "{}: not a room name: a lower-case letter, then letters, digits or -",
            quote_value(name)
        ));
    }
    let mut room = Room {
        name: name.clone(),
        untrusted: false,
        colour: 0,
        limits: Limits::default(),
        grants: Vec::new(),
    };
    let mut i = 1;
    let mut colour = None;
    let mut seen = 0u8;
    while i < w.len() {
        match w[i].as_str() {
            "--untrusted" if !room.untrusted => room.untrusted = true,
            "--colour" if colour.is_none() => {
                let n = w
                    .get(i + 1)
                    .ok_or(format!("{name}: --colour needs 1 to {SLOTS}"))?;
                match n.parse::<u8>() {
                    Ok(n) if (1..=SLOTS).contains(&n) => colour = Some(n),
                    _ => return Err(format!("{name}: --colour needs 1 to {SLOTS}")),
                }
                i += 1;
            }
            word if LIMIT_WORDS.contains(&word) => {
                let bit = 1u8 << LIMIT_WORDS.iter().position(|l| *l == word).unwrap_or(0);
                let word = String::from(word);
                if seen & bit != 0 {
                    return Err(format!("{name}: {word} twice"));
                }
                seen |= bit;
                let value = w
                    .get(i + 1)
                    .ok_or(format!("{name}: {word} needs a value"))?;
                // `none` is not a value in a line (a word is absent instead),
                // and `--tmp` is checked against the trust below, since
                // `--untrusted` may come after it.
                room.limits
                    .set(&word, value, true)
                    .map_err(|p| format!("{name}: {p}"))?;
                i += 1;
            }
            right @ ("+r" | "+w") => {
                let path = w
                    .get(i + 1)
                    .ok_or(format!("{name}: {right} needs a path"))?;
                if !is_grant_path(path) {
                    return Err(format!(
                        "{name}: {}: a grant's path starts with ~ or /, with no . or ..",
                        quote_value(path)
                    ));
                }
                if room.grants.len() == MAX_GRANTS {
                    return Err(format!("{name}: more than {MAX_GRANTS} grants"));
                }
                room.grants.push(Grant {
                    write: right == "+w",
                    path: path.clone(),
                });
                i += 1;
            }
            other => {
                return Err(format!(
                    "{name}: {}: not --untrusted, --colour N, a limit word, +r PATH or +w PATH",
                    quote_value(other)
                ));
            }
        }
        i += 1;
    }
    if room.untrusted && colour.is_some() {
        return Err(format!(
            "{name}: an untrusted room has the reserved colour, not --colour"
        ));
    }
    if !room.untrusted && room.limits.tmp.is_some() {
        return Err(format!("{name}: --tmp is for untrusted rooms only"));
    }
    room.colour = colour.unwrap_or(0);
    Ok(Some(room))
}

/// Reads `/config/rooms`. Total: any bytes give rooms and problems; a bad
/// line, a second line of one name and the lines past [`MAX_ROOMS`] are
/// reported and skipped. A trusted room without `--colour` takes the lowest
/// free slot after every line with one.
pub fn read(bytes: &[u8]) -> Rooms {
    let mut out = Rooms::default();
    // Past MAX_SIZE the file is cut at the last whole line before it, and
    // that is reported: a cut line could read as a wider grant.
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
            Ok(Some(room)) if out.get(&room.name).is_some() => {
                format!("{}: a second line; the first counts", room.name)
            }
            Ok(Some(_)) if out.rooms.len() == MAX_ROOMS => {
                format!("more than {MAX_ROOMS} rooms; the rest are ignored")
            }
            Ok(Some(room)) => {
                out.rooms.push(room);
                continue;
            }
            Err(p) => p,
        };
        out.problems.push(Problem { line, problem });
    }
    let unset: Vec<usize> = (0..out.rooms.len())
        .filter(|i| !out.rooms[*i].untrusted && out.rooms[*i].colour == 0)
        .collect();
    for i in unset {
        // Rooms still at 0 do not count in `free_colour`.
        let mut set = out.clone();
        set.rooms.retain(|r| r.untrusted || r.colour != 0);
        out.rooms[i].colour = set.free_colour();
    }
    out
}

/// Why the writer changed nothing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RoomRefusal {
    BadName,
    Exists,
    NoSuchRoom,
    TooMany,
    BadGrant,
}

/// The file with `room` added as its last line; every other byte kept.
pub fn add(bytes: &[u8], room: &Room) -> Result<Vec<u8>, RoomRefusal> {
    let have = read(bytes);
    if !is_room_name(&room.name) {
        return Err(RoomRefusal::BadName);
    }
    if have.get(&room.name).is_some() {
        return Err(RoomRefusal::Exists);
    }
    if have.rooms.len() >= MAX_ROOMS || room.grants.len() > MAX_GRANTS {
        return Err(RoomRefusal::TooMany);
    }
    if room.grants.iter().any(|g| !is_grant_path(&g.path)) {
        return Err(RoomRefusal::BadGrant);
    }
    let mut out = bytes.to_vec();
    if !out.is_empty() && !out.ends_with(b"\n") {
        out.push(b'\n');
    }
    out.extend_from_slice(room.line().as_bytes());
    out.push(b'\n');
    Ok(out)
}

/// The file without `name`'s lines (every one: a second line of a name
/// would count once the first is gone); every other byte kept.
pub fn remove(bytes: &[u8], name: &str) -> Result<Vec<u8>, RoomRefusal> {
    let parts: Vec<&[u8]> = bytes.split(|b| *b == b'\n').collect();
    let is_it = |raw: &&[u8]| {
        let raw = raw.strip_suffix(b"\r").unwrap_or(raw);
        core::str::from_utf8(raw)
            .ok()
            .and_then(|t| parse(t).ok().flatten())
            .is_some_and(|r| r.name == name)
    };
    if !parts.iter().any(is_it) {
        return Err(RoomRefusal::NoSuchRoom);
    }
    let kept: Vec<&[u8]> = parts.iter().filter(|p| !is_it(p)).copied().collect();
    Ok(kept.join(&b'\n'))
}

/// The file with the first line of `room.name` replaced by `room`'s line
/// (`room set`, and the rewrite of an untrusted line that lacked a word,
/// ADR-0266 points 6 and 8); every other byte kept.
pub fn replace(bytes: &[u8], room: &Room) -> Result<Vec<u8>, RoomRefusal> {
    if room.grants.len() > MAX_GRANTS
        || room.grants.iter().any(|g| !is_grant_path(&g.path))
        || !is_room_name(&room.name)
    {
        return Err(RoomRefusal::BadGrant);
    }
    let parts: Vec<&[u8]> = bytes.split(|b| *b == b'\n').collect();
    let at = parts.iter().position(|raw| {
        let raw = raw.strip_suffix(b"\r").unwrap_or(raw);
        core::str::from_utf8(raw)
            .ok()
            .and_then(|t| parse(t).ok().flatten())
            .is_some_and(|r| r.name == room.name)
    });
    let Some(at) = at else {
        return Err(RoomRefusal::NoSuchRoom);
    };
    let line = room.line();
    let mut out = Vec::with_capacity(bytes.len() + line.len());
    for (i, p) in parts.iter().enumerate() {
        if i > 0 {
            out.push(b'\n');
        }
        if i == at {
            out.extend_from_slice(line.as_bytes());
        } else {
            out.extend_from_slice(p);
        }
    }
    Ok(out)
}

/// Which limit a room refused, for the shell's hint (ADR-0263 point 3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refused {
    Memory,
    Processes,
}

/// The refusal counters of a room, before and after a line: `(memory,
/// processes)` refusals. At most one hint per line (INV-0263-4): the memory
/// field if it rose, else the processes field, however many times either
/// was refused.
pub fn refused_field(before: (u64, u64), after: (u64, u64)) -> Option<Refused> {
    if after.0 > before.0 {
        Some(Refused::Memory)
    } else if after.1 > before.1 {
        Some(Refused::Processes)
    } else {
        None
    }
}

/// The hint text for a refused field of `room`, whose limit is `limit`
/// (bytes or processes): it names the field and the room, and the line that
/// raises it twofold. `manage` is false in a room's own shell, which holds
/// no `Set` (ADR-0266 point 6) and so says where to run it.
pub fn quota_hint(room: &str, field: Refused, limit: u64, manage: bool) -> String {
    let (what, word, now, raised) = match field {
        Refused::Memory => (
            "memory",
            "--mem",
            format_size(limit),
            format_size(limit.saturating_mul(2)),
        ),
        Refused::Processes => (
            "processes",
            "--procs",
            format!("{limit}"),
            format!("{}", limit.saturating_mul(2)),
        ),
    };
    let from = if manage { "" } else { "from home: " };
    format!(
        "hint: room {room} is at its {what} limit of {now}; {from}room set {room} {word} {raised}\n"
    )
}
