//! The panel's logic (ADR-0168 point 1, ADR-0172 point 4), without a
//! kernel, so that it is tested on the host: what a launch gives each
//! program (its *launch record*), what a press on the dock does, and the
//! text the compositor is told.
//!
//! A launch record lists every handle the program receives, by slot and
//! kind, and the grants in ADR-0153's words that the compositor draws on
//! its window. The program `panel` spawns exactly the handles a record
//! lists (INV-0168-1).

#![cfg_attr(not(test), no_std)]

extern crate alloc;

use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use zoraya_settings::rooms::{HOME, Room, Rooms};

/// What a handle in a launch is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Give {
    /// The console, to write only (INV-0097-3).
    Console,
    /// The program's own job.
    Job,
    /// The system image's directory, to find programs.
    Directory,
    /// The compositor's client channel (`term`).
    Compositor,
    /// The shell's end of the terminal's channel (`zosh`), or the
    /// terminal's end (`term`).
    Terminal,
    /// The volume's root with its rights: only the shell in a terminal
    /// holds it (ADR-0168, INV-0168-2).
    Root,
}

/// One handle of a launch: its slot and what it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Slot {
    pub slot: u32,
    pub give: Give,
}

/// One program of a launch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Program {
    pub image: &'static str,
    pub handles: Vec<Slot>,
    pub args: Vec<String>,
}

/// What the panel gives a launch, and shows of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Launch {
    /// The application as the dock names it.
    pub app: &'static str,
    /// The room (`None` is `home`).
    pub room: Option<Room>,
    /// The record's entries in ADR-0153's words, for the window's pills.
    pub entries: Vec<String>,
    /// The `zosh` line that does the same.
    pub line: String,
    pub programs: Vec<Program>,
}

/// The applications the dock lists, with the line each is launched by.
/// Stage 7d has the terminal; `files`, `edit` and `settings` come with 7e
/// and 7f.
pub const APPS: [(&str, &str); 1] = [("term", "term")];

/// A terminal launched in `room`: `term` with the compositor's channel and
/// its end of the shell's channel; `zosh` with the volume's root (the shell
/// in a terminal holds it, INV-0168-2), its end of the channel, the
/// console to write, its job and the image directory, started in the room.
/// The record shows the root and the room's grants.
pub fn terminal(room: Option<&Room>) -> Launch {
    let mut entries = vec![String::from("+w /")];
    let mut args = Vec::new();
    let mut line = String::from("term");
    if let Some(r) = room {
        entries.extend(r.grants.iter().map(|g| g.words()));
        args.push(String::from("--room"));
        args.push(r.name.clone());
        line = format!("room {} term", r.name);
    }
    Launch {
        app: "term",
        room: room.cloned(),
        entries,
        line,
        programs: vec![
            Program {
                image: "term",
                handles: vec![
                    Slot {
                        slot: 1,
                        give: Give::Console,
                    },
                    Slot {
                        slot: 5,
                        give: Give::Compositor,
                    },
                    Slot {
                        slot: 6,
                        give: Give::Terminal,
                    },
                ],
                args: Vec::new(),
            },
            Program {
                image: "zosh",
                handles: vec![
                    Slot {
                        slot: 1,
                        give: Give::Console,
                    },
                    Slot {
                        slot: 2,
                        give: Give::Job,
                    },
                    Slot {
                        slot: 3,
                        give: Give::Directory,
                    },
                    Slot {
                        slot: 5,
                        give: Give::Root,
                    },
                    Slot {
                        slot: 6,
                        give: Give::Terminal,
                    },
                ],
                args,
            },
        ],
    }
}

/// What a press on the dock asks of the panel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    Launch(Launch),
    /// The current room changed: tell the compositor.
    Entered,
    /// `+`: the rooms overview's form is not built yet (ADR-0172's
    /// overview, a later step); nothing happens.
    Nothing,
}

/// The panel's state: the room table and the current room.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Panel {
    pub rooms: Rooms,
    /// `None` is `home`.
    pub current: Option<String>,
}

/// `DockPressed`'s kinds (as `zoraya-proto-gui`'s `dock` module has them).
pub const LAUNCH: u32 = 0;
pub const ROOM: u32 = 1;
pub const NEW_ROOM: u32 = 2;

impl Panel {
    /// The table as read again from `/config/rooms`; a current room that
    /// is gone leaves for `home`. Gives whether anything changed.
    pub fn reload(&mut self, bytes: &[u8]) -> bool {
        let mut rooms = zoraya_settings::rooms::read(bytes);
        // What the compositor can be told is what the panel keeps: rooms
        // past `MAX_ROOMS_TEXT` are left out of both, and reported.
        while rooms_text(&rooms).len() > MAX_ROOMS_TEXT {
            rooms.rooms.pop();
            rooms.problems.push(zoraya_settings::Problem {
                line: 0,
                problem: String::from("the room table is longer than the compositor takes; the last rooms are left out"),
            });
        }
        let changed = rooms.rooms != self.rooms.rooms;
        self.rooms = rooms;
        let gone = self
            .current
            .as_ref()
            .is_some_and(|c| self.rooms.get(c).is_none());
        if gone {
            self.current = None;
        }
        changed || gone
    }

    pub fn room(&self) -> Option<&Room> {
        self.current.as_ref().and_then(|c| self.rooms.get(c))
    }

    /// A press on the dock (ADR-0168): an application with no window is
    /// launched in the current room; a chip enters its room (0 `home`);
    /// `+` does nothing yet.
    pub fn pressed(&mut self, kind: u32, index: u32) -> Action {
        match kind {
            LAUNCH => match APPS.get(index as usize) {
                // Nothing runs in an untrusted room until quotas exist
                // (ADR-0056, TASK-24): `room new --untrusted` is refused, and
                // so is a launch in a hand-written untrusted room.
                _ if self.room().is_some_and(|r| r.untrusted) => Action::Nothing,
                Some(("term", _)) => Action::Launch(terminal(self.room())),
                _ => Action::Nothing,
            },
            ROOM => {
                let i = index as usize;
                let next = if i == 0 {
                    None
                } else {
                    match self.rooms.rooms.get(i - 1) {
                        Some(r) => Some(r.name.clone()),
                        None => return Action::Nothing,
                    }
                };
                self.current = next;
                Action::Entered
            }
            _ => Action::Nothing,
        }
    }

    /// `SetRooms`' text: every room's line as the writer writes it (the
    /// colours the reader gave included).
    pub fn rooms_text(&self) -> String {
        rooms_text(&self.rooms)
    }

    /// `SetRooms`' current room: empty for `home`.
    pub fn current_name(&self) -> &str {
        self.current.as_deref().filter(|c| *c != HOME).unwrap_or("")
    }
}

/// The longest room table `SetRooms` carries (`zoraya-proto-gui`'s
/// `MAX_ROOMS_TEXT`; the panel checks they agree).
pub const MAX_ROOMS_TEXT: usize = 3072;

fn rooms_text(rooms: &Rooms) -> String {
    let mut s = String::new();
    for r in &rooms.rooms {
        s.push_str(&r.line());
        s.push('\n');
    }
    s
}

/// `SetDock`'s text: `NAME\tLINE` a line.
pub fn dock_text() -> String {
    let mut s = String::new();
    for (name, line) in APPS {
        s.push_str(&format!("{name}\t{line}\n"));
    }
    s
}

#[cfg(test)]
mod tests;

// A runaway test fails at 512 MiB instead of taking the host (docs/backlog.md).
#[cfg(test)]
zoraya_test_alloc::cap_memory!();
