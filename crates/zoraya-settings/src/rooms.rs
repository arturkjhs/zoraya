//! `/config/rooms` (ADR-0172 point 1): one room a line,
//! `NAME [--untrusted] [--colour N] GRANT...`, each `GRANT` the two words
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

/// One room's record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Room {
    pub name: String,
    pub untrusted: bool,
    /// 1 to [`SLOTS`] for a trusted room; 0 for an untrusted one, whose
    /// colour is `room.untrusted` and no other room's.
    pub colour: u8,
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
        if out.len() > 4 + 2 * MAX_GRANTS {
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
        grants: Vec::new(),
    };
    let mut i = 1;
    let mut colour = None;
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
                    "{name}: {}: not --untrusted, --colour N, +r PATH or +w PATH",
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
