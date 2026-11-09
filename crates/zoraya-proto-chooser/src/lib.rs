//! The protocol of the chooser (ADR-0167, proposed ADR-0245): an
//! application asks to open a file, to save one or to pick a directory; the
//! chooser shows its own sheet, and only when the person confirms there it
//! answers with one connection to what the person picked, its display name
//! and its kind in ADR-0153's words. No message carries a path
//! (INV-0167-1): titles, suggested names and display names are text the
//! chooser shows or the application shows, never resolved by anyone.
//!
//! The panel registers each application's channel with the chooser, with
//! the tag it gave the application's compositor client (`AddRoomClient`),
//! so that the compositor can hang the sheet under the right window without
//! any client naming another (ADR-0160).

#![no_std]
#![forbid(unsafe_code)]

use zoraya_protocol::{Below, Data, protocol};

/// The longest title an application may give; shown inside the chooser's
/// own sheet, under the heading the compositor draws.
pub const MAX_TITLE: usize = 128;
/// The longest list of filters: extensions without the dot, separated by
/// spaces (`txt md`); empty for every file.
pub const MAX_FILTERS: usize = 128;
/// The longest display name, suggested name or application name.
pub const MAX_NAME: usize = 255;
/// The longest grant words (`+r ~/project/plán.txt edit`).
pub const MAX_WORDS: usize = 512;

/// What an entry gives, in ADR-0153's kinds (the subset the chooser makes).
pub mod kind {
    /// `Open` and `PickDirectory` without write: read only.
    pub const READ: u32 = 0;
    /// `Save`: create or replace one name in the parent directory.
    pub const CREATE: u32 = 1;
    /// `PickDirectory` with write: change anything in the directory.
    pub const TREE: u32 = 2;
    pub const COUNT: u32 = 3;
}

/// Why nothing was chosen.
pub mod status {
    /// The person cancelled (Cancel, Escape or the owner went away).
    pub const CANCELLED: u32 = 0;
    /// A request is already open on this channel (one at a time).
    pub const BUSY: u32 = 1;
    /// The request was malformed.
    pub const INVALID: u32 = 2;
    /// The chooser could not list or open what was needed.
    pub const FAILED: u32 = 3;
    pub const COUNT: u32 = 4;
}

/// What the sheet asks the person to do, for the heading the compositor
/// draws: "Open a file for edit", "Save a file for edit", "Choose a folder
/// for edit".
pub mod sheet {
    pub const OPEN: u32 = 0;
    pub const SAVE: u32 = 1;
    pub const PICK: u32 = 2;
    pub const COUNT: u32 = 3;
}

protocol! {
    /// Application to chooser: choose a file to read.
    message Open<'a> = 1, handles 0 {
        title: Data<'a, MAX_TITLE>,
        filters: Data<'a, MAX_FILTERS>,
    }

    /// Application to chooser: choose a name to write, in a folder.
    message Save<'a> = 2, handles 0 {
        title: Data<'a, MAX_TITLE>,
        suggested: Data<'a, MAX_NAME>,
        filters: Data<'a, MAX_FILTERS>,
    }

    /// Application to chooser: choose a folder; `write` 1 asks for a `tree`
    /// entry, which the sheet says in words before the person confirms.
    message PickDirectory<'a> = 3, handles 0 {
        write: Below<2>,
        title: Data<'a, MAX_TITLE>,
    }

    /// Chooser to application, the answer to the request with the same
    /// transaction, after the person confirmed: one file or directory
    /// connection (for `Save`, the parent directory narrowed to `name`),
    /// the name to show, the kind and the grant's words as the frame and
    /// the shell would say them.
    message Chosen<'a> = 8, handles 1 {
        kind: Below<3>,
        name: Data<'a, MAX_NAME>,
        words: Data<'a, MAX_WORDS>,
    }

    /// Chooser to application, the answer when nothing was chosen.
    message Cancelled = 9, handles 0 {
        status: Below<4>,
    }

    /// Panel to chooser: the chooser's end of a new application's channel,
    /// the tag of its compositor client and its name for the heading.
    message AddApplication<'a> = 16, handles 1 {
        tag: u32,
        name: Data<'a, MAX_NAME>,
    }

    message Added = 16, handles 0 {
        status: Below<4>,
    }

    /// The file manager to the panel: open this file with the application
    /// for its name (ADR-0168 point 4); the connection reads it, and
    /// `words` name it as the record shows it (`+r ~/notes.txt`).
    message OpenWith<'a> = 32, handles 1 {
        name: Data<'a, MAX_NAME>,
        words: Data<'a, MAX_WORDS>,
    }

    message Opening = 32, handles 0 {
        status: Below<4>,
    }

    /// The settings window to the panel (ADR-0169): `/config/settings`
    /// changed; the panel reads it and tells the compositor
    /// (`ApplySettings`). `SettingsApplied`'s status is the compositor's
    /// answer (`status::CANCELLED` for applied, as `Added` uses it;
    /// `INVALID` when it refused the values).
    message SettingsChanged = 33, handles 0 {}

    message SettingsApplied = 33, handles 0 {
        status: Below<4>,
    }
}

/// The application requests' ordinals.
pub const APPLICATION_REQUESTS: [u32; 3] = [1, 2, 3];
/// The chooser's answers to an application.
pub const ANSWERS: [u32; 2] = [8, 9];
/// The panel's requests.
pub const PANEL_REQUESTS: [u32; 1] = [16];
/// The file manager's request to the panel.
pub const LAUNCHER_REQUESTS: [u32; 2] = [32, 33];

#[cfg(test)]
extern crate std;

#[cfg(test)]
mod tests;

// A runaway test fails at 512 MiB instead of taking the host (docs/backlog.md).
#[cfg(test)]
zoraya_test_alloc::cap_memory!();
