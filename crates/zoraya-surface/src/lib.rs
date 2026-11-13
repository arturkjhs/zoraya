//! `zoraya-surface` (proposed ADR-0317): the client side of one raw pixel
//! window, for a program that paints its own pixels (NetSurf's `libnsfb`
//! surface). Unlike `zoraya-app` there is no widget tree: the program gets
//! the buffer, says what changed ([`Surface::damage`]), asks for it to be
//! shown ([`Surface::commit`]) and reads what the person did
//! ([`Surface::event`]).
//!
//! It runs on `zoraya-app`'s [`Platform`] (the compositor's channel and
//! memory objects) and sends only the client requests of `zoraya-proto-gui`
//! (INV-0317-2): create a toplevel, set its title and buffer, commit a
//! rectangle, close. It never asks for focus, position or stacking, which
//! the protocol does not have (INV-0165-2).
//!
//! Sizes are in physical pixels, as the buffer is; the compositor speaks
//! logical pixels, which are physical pixels divided by the display's scale
//! (ADR-0164): [`Surface::open`] takes a logical size, [`Surface::size`]
//! gives the buffer's.

#![cfg_attr(not(test), no_std)]

extern crate alloc;

use alloc::collections::VecDeque;
use alloc::vec;
use alloc::vec::Vec;

use zoraya_platform::{Platform, Poll};
use zoraya_proto_gui::{self as gui, buttons, status};
use zoraya_protocol::{Data, Header, Message, decode, encode};

/// What the person did, or what the compositor decided, for the program.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Event {
    /// A key went down or up: the compositor's code (Linux's key numbers),
    /// the code point it types (0 for none) and the modifier bits of
    /// `zoraya-proto-gui`.
    Key {
        code: u32,
        text: u32,
        pressed: bool,
        modifiers: u32,
    },
    /// The pointer moved, to a physical pixel of the buffer.
    Move { x: u32, y: u32 },
    /// A button of `zoraya_proto_gui::buttons` went down or up at a pixel.
    Button {
        x: u32,
        y: u32,
        button: u32,
        pressed: bool,
    },
    /// The window has the keyboard, or lost it.
    Focus(bool),
    /// The buffer has a new size, in physical pixels: read it again.
    Resize { width: u32, height: u32 },
    /// The person asked to close the window (ADR-0160).
    Close,
    /// The compositor is gone.
    Gone,
}

/// A rectangle of the buffer, in physical pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Rect {
    x0: u32,
    y0: u32,
    x1: u32,
    y1: u32,
}

impl Rect {
    fn union(self, o: Rect) -> Rect {
        Rect {
            x0: self.x0.min(o.x0),
            y0: self.y0.min(o.y0),
            x1: self.x1.max(o.x1),
            y1: self.y1.max(o.y1),
        }
    }
}

/// A toplevel window and its buffer.
pub struct Surface<P: Platform> {
    platform: P,
    window: u32,
    /// The buffer's number for [`Platform::pixels`], and its size in
    /// physical pixels.
    buffer: usize,
    size: (u32, u32),
    scale: u32,
    /// A commit waits for its reply; damage meanwhile waits for it.
    in_flight: bool,
    pending: Option<Rect>,
    /// Messages that came while a reply was awaited, and the events made
    /// from them.
    queued: VecDeque<Vec<u8>>,
    events: VecDeque<Event>,
    next_transaction: u32,
    buttons: u32,
    pointer: (u32, u32),
}

fn message<'a, M: Message<'a>>(m: &M, transaction: u32) -> Vec<u8> {
    let mut out = vec![0u8; 512];
    let n = encode(m, transaction, &mut out).unwrap_or(0);
    out.truncate(n);
    out
}

impl<P: Platform> Surface<P> {
    /// Connects and opens a toplevel of `width × height` logical pixels
    /// titled `title`. `None` if the compositor refuses or is gone.
    pub fn open(platform: P, title: &str, width: u32, height: u32) -> Option<Surface<P>> {
        if width == 0 || height == 0 {
            return None;
        }
        let mut s = Surface {
            platform,
            window: 0,
            buffer: 0,
            size: (0, 0),
            scale: 1,
            in_flight: false,
            pending: None,
            queued: VecDeque::new(),
            events: VecDeque::new(),
            next_transaction: 1,
            buttons: 0,
            pointer: (0, 0),
        };
        let reply = s.call(&gui::DisplayScale {}, None)?;
        let (_, d) = decode::<gui::DisplayScaleReply>(&reply, 0).ok()?;
        if d.status.0 != status::OK {
            return None;
        }
        s.scale = d.scale.clamp(1, 4);
        let (pw, ph) = (width.checked_mul(s.scale)?, height.checked_mul(s.scale)?);
        let (memory, buffer) = s.platform.buffer(pw as usize * ph as usize)?;
        let reply = s.call(&gui::CreateToplevel { width, height }, Some(memory));
        let created = reply
            .as_deref()
            .and_then(|r| decode::<gui::Created>(r, 0).ok())
            .map(|(_, c)| c);
        let Some(c) = created.filter(|c| c.status.0 == status::OK) else {
            s.platform.free(buffer);
            return None;
        };
        s.window = c.window;
        s.buffer = buffer;
        s.size = (pw, ph);
        s.set_title(title);
        Some(s)
    }

    /// Sends a request and waits for its reply (the same ordinal and
    /// transaction); what comes meanwhile is kept for [`Surface::event`].
    fn call<'a, M: Message<'a>>(&mut self, m: &M, handle: Option<P::End>) -> Option<Vec<u8>> {
        let t = self.transaction();
        if !self.platform.send(&message(m, t), handle) {
            return None;
        }
        let mut bytes = vec![0u8; zoraya_protocol::MAX_WIRE_MESSAGE];
        loop {
            let n = self.platform.receive(&mut bytes)?;
            let got = &bytes[..n];
            match Header::decode(got) {
                Ok((h, _)) if h.ordinal == M::ORDINAL && h.transaction == t => {
                    return Some(got.to_vec());
                }
                _ => self.queued.push_back(got.to_vec()),
            }
        }
    }

    fn transaction(&mut self) -> u32 {
        let t = self.next_transaction;
        self.next_transaction = t.wrapping_add(1).max(1);
        t
    }

    /// A new title; the frame draws it (ADR-0160).
    pub fn set_title(&mut self, title: &str) -> bool {
        let n = title.len().min(gui::MAX_TITLE);
        // A cut inside a character would not be text: cut before it.
        let mut n = n;
        while !title.is_char_boundary(n) {
            n -= 1;
        }
        let m = gui::SetTitle {
            window: self.window,
            text: Data(&title.as_bytes()[..n]),
        };
        self.call(&m, None).and_then(|r| {
            decode::<gui::TitleDone>(&r, 0)
                .ok()
                .map(|(_, d)| d.status.0)
        }) == Some(status::OK)
    }

    /// The buffer's size in physical pixels; its stride is the width.
    pub fn size(&self) -> (u32, u32) {
        self.size
    }

    /// The display's scale: physical pixels per logical pixel.
    pub fn scale(&self) -> u32 {
        self.scale
    }

    /// The buffer: `width × height` pixels, row after row, `0x00RRGGBB`.
    pub fn pixels(&mut self) -> &mut [u32] {
        let n = self.size.0 as usize * self.size.1 as usize;
        let p = self.platform.pixels(self.buffer);
        let n = n.min(p.len());
        &mut p[..n]
    }

    /// Says that the rectangle changed. It is clipped to the buffer
    /// (INV-0317-2); one outside it, or empty, changes nothing. Nothing is
    /// shown until [`Surface::commit`].
    pub fn damage(&mut self, x: i64, y: i64, width: i64, height: i64) {
        let (w, h) = (i64::from(self.size.0), i64::from(self.size.1));
        let x0 = x.clamp(0, w);
        let y0 = y.clamp(0, h);
        let x1 = x.saturating_add(width).clamp(0, w);
        let y1 = y.saturating_add(height).clamp(0, h);
        if x0 >= x1 || y0 >= y1 {
            return;
        }
        let r = Rect {
            x0: x0 as u32,
            y0: y0 as u32,
            x1: x1 as u32,
            y1: y1 as u32,
        };
        self.pending = Some(self.pending.map_or(r, |p| p.union(r)));
    }

    /// Shows the damage, unless the last frame is not yet shown (ADR-0093:
    /// one frame in flight): it goes when the compositor's reply comes.
    pub fn commit(&mut self) {
        let Some(r) = self.pending else { return };
        if self.in_flight {
            return;
        }
        let t = self.transaction();
        let m = gui::CommitTo {
            window: self.window,
            x: r.x0,
            y: r.y0,
            width: r.x1 - r.x0,
            height: r.y1 - r.y0,
        };
        if self.platform.send(&message(&m, t), None) {
            self.in_flight = true;
            self.pending = None;
        } else {
            self.events.push_back(Event::Gone);
        }
    }

    /// The next event; with `block`, waits for one. `None`: nothing there.
    pub fn event(&mut self, block: bool) -> Option<Event> {
        let mut bytes = vec![0u8; zoraya_protocol::MAX_WIRE_MESSAGE];
        loop {
            if let Some(e) = self.events.pop_front() {
                return Some(e);
            }
            let got = match self.queued.pop_front() {
                Some(b) => b,
                None if block => match self.platform.receive(&mut bytes) {
                    Some(n) => bytes[..n].to_vec(),
                    None => return Some(Event::Gone),
                },
                None => match self.platform.poll(&mut bytes) {
                    Poll::Message(n) => bytes[..n].to_vec(),
                    Poll::Empty => return None,
                    Poll::Gone => return Some(Event::Gone),
                },
            };
            self.handle(&got);
        }
    }

    fn handle(&mut self, got: &[u8]) {
        let Ok((h, _)) = Header::decode(got) else {
            return;
        };
        match h.ordinal {
            7 => {
                self.in_flight = false;
                self.commit();
            }
            19 => {
                if let Ok((_, c)) = decode::<gui::Configure>(got, 0) {
                    self.configure(c);
                }
            }
            20 => {
                if let Ok((_, c)) = decode::<gui::CloseRequested>(got, 0)
                    && c.window == self.window
                {
                    self.events.push_back(Event::Close);
                }
            }
            22 => {
                if let Ok((_, f)) = decode::<gui::WindowFocus>(got, 0)
                    && f.window == self.window
                {
                    self.events.push_back(Event::Focus(f.focused));
                }
            }
            16 => {
                if let Ok((_, k)) = decode::<gui::Key>(got, 0) {
                    self.events.push_back(Event::Key {
                        code: k.code,
                        text: k.text.0,
                        pressed: k.pressed,
                        modifiers: k.modifiers.0,
                    });
                }
            }
            30 => {
                if let Ok((_, p)) = decode::<gui::WindowPointerMods>(got, 0)
                    && p.window == self.window
                {
                    self.pointer_event(p.x, p.y, p.buttons.0);
                }
            }
            _ => {}
        }
    }

    /// The pointer message of the compositor is a state, not a change: the
    /// events are what differs from the last one.
    fn pointer_event(&mut self, lx: u32, ly: u32, now: u32) {
        let k = self.scale;
        let x = lx.saturating_mul(k).min(self.size.0.saturating_sub(1));
        let y = ly.saturating_mul(k).min(self.size.1.saturating_sub(1));
        if (x, y) != self.pointer {
            self.pointer = (x, y);
            self.events.push_back(Event::Move { x, y });
        }
        let changed = (now ^ self.buttons) & buttons::ALL;
        for button in [buttons::LEFT, buttons::MIDDLE, buttons::RIGHT] {
            if changed & button != 0 {
                self.events.push_back(Event::Button {
                    x,
                    y,
                    button,
                    pressed: now & button != 0,
                });
            }
        }
        self.buttons = now;
    }

    /// A new size from the compositor (ADR-0160): a new buffer at it,
    /// swapped in when the compositor has taken it.
    fn configure(&mut self, c: gui::Configure) {
        if c.window != self.window || c.width == 0 || c.height == 0 {
            return;
        }
        let k = c.scale.clamp(1, 4);
        let (Some(pw), Some(ph)) = (c.width.checked_mul(k), c.height.checked_mul(k)) else {
            return;
        };
        if (pw, ph) == self.size {
            return;
        }
        let Some((memory, buffer)) = self.platform.buffer(pw as usize * ph as usize) else {
            return;
        };
        let m = gui::SetBuffer {
            window: self.window,
            width: c.width,
            height: c.height,
        };
        let ok = self.call(&m, Some(memory)).and_then(|r| {
            decode::<gui::BufferDone>(&r, 0)
                .ok()
                .map(|(_, d)| d.status.0)
        }) == Some(status::OK);
        if !ok {
            self.platform.free(buffer);
            return;
        }
        let old = core::mem::replace(&mut self.buffer, buffer);
        self.platform.free(old);
        self.scale = k;
        self.size = (pw, ph);
        // The new buffer is blank: all of it is to be shown once painted.
        self.pending = None;
        self.events.push_back(Event::Resize {
            width: pw,
            height: ph,
        });
    }

    /// Closes the window.
    pub fn close(mut self) {
        let w = self.window;
        self.call(&gui::Close { window: w }, None);
        self.platform.free(self.buffer);
    }
}

#[cfg(test)]
mod tests;

// A runaway test fails at 512 MiB instead of taking the host (docs/backlog.md).
#[cfg(test)]
zoraya_test_alloc::cap_memory!();
