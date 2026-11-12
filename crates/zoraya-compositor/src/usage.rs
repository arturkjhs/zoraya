//! Rooms' bounds and use on the compositor's side (ADR-0262 point 2,
//! ADR-0267 point 3, Q6b): the account of an untrusted room, the readings
//! the panel sends once a second, the pills at 90% and the hover card. The
//! readings change only through the client manager's channel (INV-0172-1);
//! nothing a client sends reaches them.

use alloc::string::String;
use alloc::vec::Vec;

use zoraya_proto_gui::status;
use zoraya_settings::usage::{self, Reading};
use zoraya_tokens::{Scale, space, text};

use crate::{Compositor, Key, Platform, Rect};

/// Surfaces (windows and popups) one untrusted room may have (ADR-0262
/// point 2's table: 3).
pub const ROOM_SURFACES: usize = 3;
/// Display areas of buffers one untrusted room may hold (2).
pub const ROOM_AREAS: u64 = 2;
/// Surfaces all untrusted rooms together may have (4 accounts: 12), which
/// leaves `home` and the trusted rooms at least `MAX_WINDOWS - 12` = 20.
pub const UNTRUSTED_SURFACES: usize = 12;
/// Display areas of all untrusted rooms together (8).
pub const UNTRUSTED_AREAS: u64 = 8;
/// Untrusted rooms with surfaces at once (the keeper's four, ADR-0269).
pub const UNTRUSTED_ACCOUNTS: usize = 4;

/// An untrusted room's account in the compositor, as `top` would list it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Account {
    pub room: String,
    pub surfaces: usize,
    pub pixels: u64,
    /// Requests of this account refused at a bound since the room's first
    /// surface.
    pub refused: u32,
}

impl<P: Platform> Compositor<P> {
    /// The untrusted room whose account a client's surfaces are in; none for
    /// `home`, a trusted room and a system client (they share the rest).
    fn account_room(&self, slot: usize) -> Option<&str> {
        let r = &self.clients.get(slot)?.as_ref()?.record;
        (r.untrusted && !r.room.is_empty()).then_some(r.room.as_str())
    }

    /// The surfaces and pixels of the clients of untrusted room `room`.
    fn account_use(&self, room: &str) -> (usize, u64) {
        let mut surfaces = 0;
        let mut pixels = 0;
        for w in &self.windows {
            if self.account_room(w.client) == Some(room) {
                surfaces += 1;
                pixels += u64::from(w.buffer.width) * u64::from(w.buffer.height);
            }
        }
        (surfaces, pixels)
    }

    /// Whether the account of `slot`'s room may take `more_surfaces` and
    /// `more_pixels` (ADR-0262: its own bound, the untrusted rooms'
    /// together, and four accounts). A client that is not in an untrusted
    /// room is not asked: the global bound and its own are all it has. A
    /// refusal is counted for the room and changes nothing else
    /// (INV-0262-1).
    pub(crate) fn within_account(
        &mut self,
        slot: usize,
        more_surfaces: usize,
        more_pixels: u64,
    ) -> bool {
        let Some(room) = self.account_room(slot).map(String::from) else {
            return true;
        };
        let area = u64::from(self.width) * u64::from(self.height);
        let (surfaces, pixels) = self.account_use(&room);
        let mut all_surfaces = 0;
        let mut all_pixels = 0;
        let mut rooms: Vec<&str> = Vec::new();
        for w in &self.windows {
            if let Some(r) = self.account_room(w.client) {
                all_surfaces += 1;
                all_pixels += u64::from(w.buffer.width) * u64::from(w.buffer.height);
                if !rooms.contains(&r) {
                    rooms.push(r);
                }
            }
        }
        let new_account = !rooms.contains(&room.as_str());
        let ok = surfaces + more_surfaces <= ROOM_SURFACES
            && pixels + more_pixels <= ROOM_AREAS * area
            && all_surfaces + more_surfaces <= UNTRUSTED_SURFACES
            && all_pixels + more_pixels <= UNTRUSTED_AREAS * area
            && (!new_account || rooms.len() < UNTRUSTED_ACCOUNTS);
        if !ok {
            match self.refusals.iter_mut().find(|(r, _)| *r == room) {
                Some((_, n)) => *n = n.saturating_add(1),
                None => self.refusals.push((room, 1)),
            }
        }
        ok
    }

    /// The accounts of the untrusted rooms that have surfaces or refusals.
    pub fn accounts(&self) -> Vec<Account> {
        let mut out: Vec<Account> = Vec::new();
        for w in &self.windows {
            if let Some(r) = self.account_room(w.client)
                && !out.iter().any(|a| a.room == r)
            {
                let (surfaces, pixels) = self.account_use(r);
                out.push(Account {
                    room: r.into(),
                    surfaces,
                    pixels,
                    refused: 0,
                });
            }
        }
        for (room, n) in &self.refusals {
            match out.iter_mut().find(|a| a.room == *room) {
                Some(a) => a.refused = *n,
                None => out.push(Account {
                    room: room.clone(),
                    refused: *n,
                    ..Account::default()
                }),
            }
        }
        out
    }

    /// `SetUsage`: all lines or none.
    pub(crate) fn set_usage(&mut self, text: &[u8]) -> u32 {
        let Some(readings) = core::str::from_utf8(text).ok().and_then(usage::from_wire) else {
            return status::INVALID;
        };
        if readings == self.usage {
            return status::OK;
        }
        // Only the windows whose pills changed are drawn again, and the
        // card's window if its lines did: a calm desktop costs nothing.
        let card_room = self
            .card
            .and_then(|k| self.index(k))
            .and_then(|i| self.record(self.windows[i].client))
            .map(|r| r.room.clone());
        let changed = |room: &str, new: &[Reading], old: &[Reading]| {
            let find = |v: &[Reading]| v.iter().find(|r| r.name == room).cloned();
            let (a, b) = (find(old), find(new));
            if card_room.as_deref() == Some(room) {
                return a != b;
            }
            a.map(|r| r.pills()) != b.map(|r| r.pills())
        };
        let mut rects = Vec::new();
        let f = self.frame();
        for w in &self.windows {
            let Some(r) = self.record(w.client) else {
                continue;
            };
            if !r.room.is_empty() && changed(&r.room, &readings, &self.usage) {
                rects.push(w.reach(&f));
            }
        }
        self.usage = readings;
        // The card may now be taller or shorter: its new rectangle too.
        if let Some(k) = self.card
            && let Some(c) = self.card_rect(k)
        {
            rects.push(c);
        }
        for r in rects {
            self.dirty(&r);
        }
        status::OK
    }

    pub fn usage(&self) -> &[Reading] {
        &self.usage
    }

    /// The reading of a room (`home` included).
    pub fn reading(&self, room: &str) -> Option<&Reading> {
        self.usage.iter().find(|r| r.name == room)
    }

    /// The pills of usage at 90% or more for a window's room.
    pub(crate) fn usage_pills(&self, room: &str) -> Vec<String> {
        self.reading(room).map(Reading::pills).unwrap_or_default()
    }

    /// The window whose card is shown, if any.
    pub fn card(&self) -> Option<Key> {
        self.card
    }

    /// Where the hover card of a framed window is drawn, with its lines:
    /// under its title bar at the right, physical pixels.
    pub(crate) fn card_geometry(&mut self, key: Key) -> Option<(Rect, Vec<String>)> {
        let i = self.index(key)?;
        let lines = self.card_lines(key)?;
        let k = self.scale.factor();
        let cap = text::CAPTION.physical(Scale::ONE) * k;
        let si = k as i32;
        let mut tw = 0;
        for l in &lines {
            tw = tw.max(self.mono.measure(l, cap));
        }
        let f = self.frame();
        let o = self.windows[i].outer(&f);
        let width = i64::from(tw + 20 * si);
        let height = i64::from((8 + 15 * lines.len() as i32) * si);
        let x = (o.right() - width - 8 * i64::from(k)).max(o.x);
        let y = o.y + f.title + 4 * i64::from(k);
        Some((Rect::new(x, y, width, height), lines))
    }

    /// The rectangle a redraw of the card must cover: the card and the
    /// halo of its material.
    pub(crate) fn card_rect(&mut self, key: Key) -> Option<Rect> {
        let (r, _) = self.card_geometry(key)?;
        let halo = i64::from(space::DOCK_PADDING.physical(self.scale)) * 2;
        Some(Rect::new(
            r.x - halo,
            r.y - halo,
            r.width + 2 * halo,
            r.height + 2 * halo,
        ))
    }

    /// The text of a window's card: the room, its grants as the record has
    /// them, then the reading's lines (`mem 58M of 64M`, ... `limits ...`).
    /// None for a window without a room.
    pub fn card_lines(&self, key: Key) -> Option<Vec<String>> {
        let i = self.index(key)?;
        let r = self.record(self.windows[i].client)?;
        if r.room.is_empty() {
            return None;
        }
        let mut lines = Vec::new();
        lines.push(if r.untrusted {
            alloc::format!("{}  untrusted", r.room)
        } else {
            r.room.clone()
        });
        lines.extend(r.entries.iter().cloned());
        if let Some(reading) = self.reading(&r.room) {
            lines.extend(reading.card_lines());
        }
        Some(lines)
    }

    /// The pointer moved or a window changed: the card follows the title bar
    /// under the pointer, and only a framed window of a room has one.
    pub(crate) fn card_hover(&mut self, x: i64, y: i64) {
        let key = match self.under(x, y) {
            Some((k, crate::Zone::Title)) => self
                .index(k)
                .filter(|i| self.windows[*i].framed() && self.windows[*i].owner().is_none())
                .and_then(|_| self.card_lines(k).map(|_| k)),
            _ => None,
        };
        if key == self.card {
            return;
        }
        if let Some(old) = self.card.take()
            && let Some(r) = self.card_rect(old)
        {
            self.dirty(&r);
        }
        self.card = key;
        if let Some(r) = key.and_then(|k| self.card_rect(k)) {
            self.dirty(&r);
        }
    }
}
