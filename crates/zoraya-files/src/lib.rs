//! The file manager's logic and its view (stage 7h): [`model`] holds the
//! folder shown, the selection and what each gesture asks for, as [`Act`]s
//! the program carries out through `zoraya-fsclient` and the panel;
//! [`view`] is the window, a `zoraya-ui` widget drawn as the approved
//! mockups draw it (`docs/research/desktop/mockups/clear-field/files*.png`).
//! No I/O here.

#![no_std]
#![forbid(unsafe_code)]

extern crate alloc;

use alloc::format;
use alloc::string::String;

pub mod format;
pub mod model;
pub mod view;

pub use model::{Act, Column, Item, MENU, MenuItem, Model, Place};

/// `name` in `dir`.
pub fn join(dir: &str, name: &str) -> String {
    if dir == "/" {
        format!("/{name}")
    } else {
        format!("{dir}/{name}")
    }
}

/// The folder above `dir`; `/` has none.
pub fn parent(dir: &str) -> Option<String> {
    if dir == "/" {
        return None;
    }
    Some(match dir.rsplit_once('/') {
        Some(("", _)) | None => String::from("/"),
        Some((p, _)) => String::from(p),
    })
}

/// The sidebar's places from the window's holds, as the panel words them
/// (`+r /`, `+w ~`, `+w ~/project`): Home (what `~` allows, else read
/// through the volume), each of the room's other grants, and `/system`.
pub fn places(holds: &[(String, bool)], home: &str) -> alloc::vec::Vec<Place> {
    let expand = |p: &str| -> String {
        match p.strip_prefix('~') {
            Some(r) if r.is_empty() || r.starts_with('/') => format!("{home}{r}"),
            _ => String::from(p),
        }
    };
    let mut out = alloc::vec::Vec::new();
    let home_write = holds.iter().any(|(p, w)| *w && expand(p) == home);
    out.push(Place {
        label: String::from("Home"),
        path: String::from(home),
        write: home_write,
        room: false,
        system: false,
    });
    for (p, w) in holds {
        let abs = expand(p);
        if abs == "/" || abs == home {
            continue;
        }
        let label = abs.rsplit('/').next().unwrap_or(&abs);
        out.push(Place {
            label: String::from(label),
            path: abs.clone(),
            write: *w,
            room: true,
            system: false,
        });
    }
    // The volume read whole: the image's folder is a place too.
    if holds.iter().any(|(p, _)| p == "/") {
        out.push(Place {
            label: String::from("/system"),
            path: String::from("/system"),
            write: false,
            room: false,
            system: true,
        });
        // And the volume itself, read only, for the longest match.
        out.push(Place {
            label: String::from("/"),
            path: String::from("/"),
            write: false,
            room: false,
            system: true,
        });
    }
    out
}

#[cfg(test)]
extern crate std;

#[cfg(test)]
mod tests;

#[cfg(test)]
mod tests_view;

// A runaway test fails at 512 MiB instead of taking the host (docs/backlog.md).
#[cfg(test)]
zoraya_test_alloc::cap_memory!();
