//! The compositor (ADR-0093, ADR-0094) as a loop over a [`Platform`], so
//! that it runs on the host against hostile clients.
//!
//! Clients draw into memory objects of their own and commit damage. The
//! compositor copies each committed rectangle once into a copy it owns,
//! composes the screen only from its copies, flushes one rectangle per round
//! to the display driver, and then replies to each commit: the frame
//! callback. It never waits for a client. Keys go to the focused surface,
//! pointer events to the surface under the pointer, and F12 is its own.

#![no_std]
#![forbid(unsafe_code)]

extern crate alloc;

pub mod rect;

use alloc::vec;
use alloc::vec::Vec;

use zoraya_keymap::{Keymap, code};
use zoraya_proto_display as display;
use zoraya_proto_gui::{self as gui, status};
use zoraya_proto_input as input;
use zoraya_protocol::{Below, ErrorReply, Flags, Header, Message, NO_REPLY, decode, encode};

pub use rect::Rect;

/// Client connections at once `[assumption, ADR-0093]`.
pub const MAX_CLIENTS: usize = 8;
/// Input drivers at once.
pub const MAX_INPUTS: usize = 4;
/// Where the first window goes, and how far each later one moves.
pub const ORIGIN: (i64, i64) = (16, 16);
pub const CASCADE: i64 = 32;
pub const BACKGROUND: u32 = 0x0020_3040;
pub const BORDER: u32 = 0x0060_6060;
pub const BORDER_FOCUSED: u32 = 0x0040_a0ff;

/// Port keys: the manager, the display, inputs from `INPUT_KEY`, clients
/// from `CLIENT_KEY`.
pub const MANAGER_KEY: u64 = 1;
pub const DISPLAY_KEY: u64 = 2;
pub const INPUT_KEY: u64 = 16;
pub const CLIENT_KEY: u64 = 64;
/// How long the display driver may take to answer a flush before the
/// compositor drops it and tells the manager (ADR-0093, Decision).
pub const FLUSH_DEADLINE_NS: u64 = 2_000_000_000;
/// How often a flush that found the driver's queue full is tried again.
pub const RETRY_NS: u64 = 10_000_000;

/// The pointer: 1 is white, 2 is black, 0 is clear.
const POINTER: [[u8; 8]; 11] = [
    [2, 0, 0, 0, 0, 0, 0, 0],
    [2, 2, 0, 0, 0, 0, 0, 0],
    [2, 1, 2, 0, 0, 0, 0, 0],
    [2, 1, 1, 2, 0, 0, 0, 0],
    [2, 1, 1, 1, 2, 0, 0, 0],
    [2, 1, 1, 1, 1, 2, 0, 0],
    [2, 1, 1, 1, 1, 1, 2, 0],
    [2, 1, 1, 1, 1, 1, 1, 2],
    [2, 1, 1, 2, 2, 2, 2, 2],
    [2, 1, 2, 0, 0, 0, 0, 0],
    [2, 2, 0, 0, 0, 0, 0, 0],
];

/// What came on a channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Incoming {
    /// A message of this many bytes, with its handles.
    Message(usize),
    Empty,
    Closed,
    /// Too large, or too many handles: close the connection (ADR-0050).
    Refused,
}

/// What a write did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outgoing {
    Sent,
    /// The peer's queue is full.
    Full,
    Closed,
}

/// What an end is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Channel,
    Memory,
    Other,
}

/// Everything the compositor needs from the system.
pub trait Platform {
    type End: Copy + Eq + core::fmt::Debug;
    fn read(&mut self, end: Self::End, bytes: &mut [u8], handles: &mut Vec<Self::End>) -> Incoming;
    fn write(&mut self, end: Self::End, bytes: &[u8], handles: &[Self::End]) -> Outgoing;
    fn close(&mut self, end: Self::End);
    fn kind(&mut self, end: Self::End) -> Kind;
    /// Wakes the loop with `key` while `end` is readable or closed.
    fn watch(&mut self, end: Self::End, key: u64) -> bool;
    /// Maps `length` bytes of a memory object read-only.
    fn map(&mut self, memory: Self::End, length: u64) -> Option<u64>;
    fn unmap(&mut self, address: u64, length: u64);
    /// Copies pixels out of a mapping, once.
    fn copy_from(&mut self, address: u64, out: &mut [u32]) -> bool;
    /// Makes the framebuffer of `width × height`, and gives a handle to it
    /// that can be sent to the display driver. [`Platform::pixels`] is then
    /// that framebuffer.
    fn framebuffer(&mut self, width: u32, height: u32) -> Option<Self::End>;
    fn pixels(&mut self) -> &mut [u32];
    /// Sends a request to a driver and waits, with a deadline, for its
    /// reply. None if the driver is gone or late. Used only to attach a
    /// display driver (`Info`, `Attach`); flushes do not wait.
    fn call(
        &mut self,
        end: Self::End,
        message: &[u8],
        handles: &[Self::End],
        reply: &mut [u8],
    ) -> Option<usize>;
    /// Blocks until something is ready, or until `deadline` on the clock of
    /// [`Platform::now`], and gives the keys.
    fn wait(&mut self, ready: &mut Vec<u64>, deadline: Option<u64>);
    fn now(&mut self) -> u64;
}

struct Surface {
    width: u32,
    height: u32,
    /// Where the surface's top left is on the screen.
    x: i64,
    y: i64,
    address: u64,
    length: u64,
    /// The compositor's own copy: what it composes from.
    copy: Vec<u32>,
}

impl Surface {
    fn area(&self) -> Rect {
        Rect::new(
            self.x,
            self.y,
            i64::from(self.width),
            i64::from(self.height),
        )
    }

    /// The area with its one-pixel frame.
    fn frame(&self) -> Rect {
        Rect::new(
            self.x - 1,
            self.y - 1,
            i64::from(self.width) + 2,
            i64::from(self.height) + 2,
        )
    }
}

struct Client<E> {
    end: E,
    surface: Option<Surface>,
    /// A commit waiting for the round's flush: its transaction.
    commit: Option<u32>,
    /// Events dropped since the last that reached it.
    dropped: u32,
    /// The order surfaces were created in, for F12.
    created: u64,
}

/// What composition has cost.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Stats {
    pub frames: u64,
    pub commits: u64,
    pub compose_ns: u64,
    pub flush_ns: u64,
    /// Flushes a display driver answered.
    pub flushes: u64,
    /// Display drivers dropped for missing their deadline.
    pub hangs: u64,
}

pub struct Compositor<P: Platform> {
    platform: P,
    manager: P::End,
    display: Option<P::End>,
    inputs: Vec<Option<P::End>>,
    clients: Vec<Option<Client<P::End>>>,
    /// Surfaces, bottom first, by client index.
    order: Vec<usize>,
    focus: Option<usize>,
    width: u32,
    height: u32,
    attached: bool,
    keymap: Keymap,
    /// Where the pointer is, once a pointer has spoken.
    pointer: Option<(i64, i64)>,
    buttons: u32,
    /// The pointer moved or a button changed this frame.
    pending_pointer: bool,
    /// What changed on the screen this round.
    damage: Rect,
    /// Composed and not yet sent to the display driver.
    unflushed: Rect,
    /// When the one flush in flight was sent.
    in_flight: Option<u64>,
    /// Since when a flush has been waiting for room in the driver's queue.
    stuck_since: Option<u64>,
    created: u64,
    stats: Stats,
    message: Vec<u8>,
    handles: Vec<P::End>,
    out: [u8; 128],
}

fn reply<'a, M: Message<'a>>(m: &M, transaction: u32, out: &mut [u8]) -> usize {
    // Every message here is small and fixed: encoding cannot fail.
    encode(m, transaction, out).unwrap_or(0)
}

impl<P: Platform> Compositor<P> {
    /// A compositor with its manager channel, and the drivers it starts
    /// with. It asks the display for its size and attaches a framebuffer.
    pub fn new(
        mut platform: P,
        manager: P::End,
        display: Option<P::End>,
        inputs: &[P::End],
    ) -> Compositor<P> {
        platform.watch(manager, MANAGER_KEY);
        let mut c = Compositor {
            platform,
            manager,
            display: None,
            inputs: vec![None; MAX_INPUTS],
            clients: (0..MAX_CLIENTS).map(|_| None).collect(),
            order: Vec::new(),
            focus: None,
            width: 0,
            height: 0,
            attached: false,
            keymap: Keymap::new(),
            pointer: None,
            buttons: 0,
            pending_pointer: false,
            damage: Rect::default(),
            unflushed: Rect::default(),
            in_flight: None,
            stuck_since: None,
            created: 0,
            stats: Stats::default(),
            message: vec![0; 4096],
            handles: Vec::new(),
            out: [0; 128],
        };
        if let Some(d) = display {
            c.attach_display(d);
        }
        for i in inputs {
            c.attach_input(*i);
        }
        c
    }

    pub fn platform(&mut self) -> &mut P {
        &mut self.platform
    }

    pub const fn size(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    pub const fn stats(&self) -> Stats {
        self.stats
    }

    /// The client that has keyboard focus, by index.
    pub const fn focus(&self) -> Option<usize> {
        self.focus
    }

    /// Where a client's surface is on the screen, if it has one.
    pub fn placement(&self, client: usize) -> Option<Rect> {
        let c = self.clients.get(client)?.as_ref()?;
        c.surface.as_ref().map(Surface::area)
    }

    pub fn client_count(&self) -> usize {
        self.clients.iter().flatten().count()
    }

    /// Runs forever.
    pub fn run(&mut self) -> ! {
        loop {
            self.step();
        }
    }

    /// One round: wait, take one message from each ready connection, compose
    /// and flush what changed, reply to the round's commits.
    pub fn step(&mut self) {
        let mut ready = Vec::new();
        // The flush in flight has its deadline; a flush waiting for room in
        // the driver's queue is tried again every 10 ms, under the same one.
        let deadline = match (self.in_flight, self.stuck_since) {
            (Some(sent), _) => Some(sent + FLUSH_DEADLINE_NS),
            (None, Some(_)) => Some(self.platform.now() + RETRY_NS),
            (None, None) => None,
        };
        self.platform.wait(&mut ready, deadline);
        for key in ready {
            match key {
                MANAGER_KEY => self.manager_message(),
                DISPLAY_KEY => self.display_message(),
                k if (INPUT_KEY..INPUT_KEY + MAX_INPUTS as u64).contains(&k) => {
                    self.input_message((k - INPUT_KEY) as usize);
                }
                k if (CLIENT_KEY..CLIENT_KEY + MAX_CLIENTS as u64).contains(&k) => {
                    self.client_message((k - CLIENT_KEY) as usize);
                }
                _ => {}
            }
        }
        let waiting = self.in_flight.or(self.stuck_since);
        if let Some(since) = waiting
            && self.platform.now() >= since + FLUSH_DEADLINE_NS
        {
            self.display_hung();
        }
        self.finish_round();
    }

    /// The display driver missed its deadline: drop it, and tell the
    /// manager, which restarts it (ADR-0093, Decision).
    fn display_hung(&mut self) {
        self.stats.hangs += 1;
        self.lose_display();
        let length = reply(&gui::DisplayHung {}, NO_REPLY, &mut self.out);
        // If the manager's queue is full the report is lost, and the
        // compositor goes on with no display until the manager attaches one.
        let _ = self.platform.write(self.manager, &self.out[..length], &[]);
    }

    // ---- drivers ----------------------------------------------------------

    fn attach_display(&mut self, end: P::End) -> u32 {
        if self.display.is_some() {
            self.platform.close(end);
            return status::EXISTS;
        }
        let mut answer = [0u8; 64];
        let length = reply(&display::Info {}, 1, &mut self.out);
        let message = self.out;
        let Some(n) = self
            .platform
            .call(end, &message[..length], &[], &mut answer)
        else {
            self.platform.close(end);
            return status::UNSUPPORTED;
        };
        let info = match decode::<display::InfoReply>(&answer[..n], 0) {
            Ok((_, m)) if m.status.0 == display::status::OK && m.format == display::FORMAT_XRGB => {
                m
            }
            _ => {
                self.platform.close(end);
                return status::UNSUPPORTED;
            }
        };
        let side = |v: u32| (1..=8192).contains(&v);
        if !side(info.width) || !side(info.height) {
            self.platform.close(end);
            return status::UNSUPPORTED;
        }
        if (info.width, info.height) != (self.width, self.height) || !self.attached {
            let Some(frame) = self.platform.framebuffer(info.width, info.height) else {
                self.platform.close(end);
                return status::NO_MEMORY;
            };
            self.width = info.width;
            self.height = info.height;
            self.attached = true;
            let length = reply(&display::Attach {}, 2, &mut self.out);
            let message = self.out;
            let done = self
                .platform
                .call(end, &message[..length], &[frame], &mut answer);
            if !Self::done_ok(done, &answer) {
                self.platform.close(end);
                return status::UNSUPPORTED;
            }
        } else {
            // A new driver for the same display: the same framebuffer.
            let Some(frame) = self.platform.framebuffer(info.width, info.height) else {
                self.platform.close(end);
                return status::NO_MEMORY;
            };
            let length = reply(&display::Attach {}, 2, &mut self.out);
            let message = self.out;
            let done = self
                .platform
                .call(end, &message[..length], &[frame], &mut answer);
            if !Self::done_ok(done, &answer) {
                self.platform.close(end);
                return status::UNSUPPORTED;
            }
        }
        self.display = Some(end);
        self.platform.watch(end, DISPLAY_KEY);
        // Everything, once: a new driver shows nothing until told.
        self.damage = self.damage.union(&self.screen());
        self.in_flight = None;
        self.stuck_since = None;
        status::OK
    }

    fn done_ok(n: Option<usize>, answer: &[u8]) -> bool {
        n.and_then(|n| decode::<display::Done>(&answer[..n], 0).ok())
            .is_some_and(|(_, d)| d.status.0 == display::status::OK)
    }

    fn screen(&self) -> Rect {
        Rect::new(0, 0, i64::from(self.width), i64::from(self.height))
    }

    /// The display driver answers the flush in flight; anything else
    /// readable is a broken driver or its end.
    fn display_message(&mut self) {
        let Some(end) = self.display else { return };
        let mut handles = Vec::new();
        match self.platform.read(end, &mut self.message, &mut handles) {
            Incoming::Empty => {}
            Incoming::Message(n)
                if handles.is_empty()
                    && self.in_flight.is_some()
                    && decode::<display::Done>(&self.message[..n], 0).is_ok() =>
            {
                let sent = self.in_flight.take().unwrap_or(0);
                self.stats.flushes += 1;
                self.stats.flush_ns += self.platform.now().saturating_sub(sent);
                // The latest frame, whatever was composed meanwhile.
                self.send_flush();
            }
            _ => {
                for h in handles {
                    self.platform.close(h);
                }
                self.lose_display();
            }
        }
    }

    fn lose_display(&mut self) {
        self.in_flight = None;
        self.stuck_since = None;
        if let Some(end) = self.display.take() {
            self.platform.close(end);
        }
    }

    fn attach_input(&mut self, end: P::End) -> u32 {
        let Some(slot) = self.inputs.iter().position(Option::is_none) else {
            self.platform.close(end);
            return status::NO_MEMORY;
        };
        self.inputs[slot] = Some(end);
        self.platform.watch(end, INPUT_KEY + slot as u64);
        status::OK
    }

    fn input_message(&mut self, slot: usize) {
        let Some(end) = self.inputs[slot] else { return };
        self.handles.clear();
        let mut message = core::mem::take(&mut self.message);
        let mut handles = core::mem::take(&mut self.handles);
        let incoming = self.platform.read(end, &mut message, &mut handles);
        for h in handles.drain(..) {
            self.platform.close(h);
        }
        self.handles = handles;
        match incoming {
            Incoming::Message(n) => {
                if let Ok((_, frame)) = decode::<input::Frame<'_>>(&message[..n], 0) {
                    for event in frame.events.iter() {
                        if event.is_valid() {
                            self.input_event(event);
                        }
                    }
                    self.pointer_moved_or_pressed();
                }
            }
            Incoming::Empty => {}
            Incoming::Closed | Incoming::Refused => {
                self.platform.close(end);
                self.inputs[slot] = None;
                // Keys it held down will never be released (review R10).
                self.keymap = Keymap::new();
                self.buttons = 0;
            }
        }
        self.message = message;
    }

    // ---- input routing ----------------------------------------------------

    fn input_event(&mut self, e: input::Event) {
        match e.kind.0 {
            input::kind::AXIS => {
                let span = |side: u32| i64::from(side.max(1) - 1);
                let v = i64::from(e.value);
                let (mut x, mut y) = self.pointer.unwrap_or((0, 0));
                if e.code == 0 {
                    x = v * span(self.width) / i64::from(input::AXIS_MAX);
                } else {
                    y = v * span(self.height) / i64::from(input::AXIS_MAX);
                }
                self.move_pointer(x, y);
            }
            _ => self.key_event(e.code, e.value),
        }
    }

    fn key_event(&mut self, code_: u32, value: u32) {
        let button = match code_ {
            code::BTN_LEFT => gui::buttons::LEFT,
            code::BTN_RIGHT => gui::buttons::RIGHT,
            code::BTN_MIDDLE => gui::buttons::MIDDLE,
            _ => 0,
        };
        if button != 0 {
            if value == 0 {
                self.buttons &= !button;
            } else {
                if value == 1 {
                    self.click();
                }
                self.buttons |= button;
            }
            self.pending_pointer = true;
            return;
        }
        if (0x110..0x200).contains(&code_) {
            return;
        }
        let key = self.keymap.key(code_, value);
        // F12 is the compositor's: no client sees it (INV-0094-1).
        if code_ == code::F12 {
            if value == 1 {
                self.next_focus();
            }
            return;
        }
        let Some(client) = self.focus else { return };
        let m = gui::Key {
            dropped: 0,
            code: code_,
            pressed: key.pressed,
            modifiers: Flags(key.modifiers),
            text: Below(key.text.map_or(0, u32::from)),
        };
        self.send_event(client, m);
    }

    fn move_pointer(&mut self, x: i64, y: i64) {
        let old = self.pointer;
        if old == Some((x, y)) {
            return;
        }
        if let Some((ox, oy)) = old {
            self.damage = self.damage.union(&pointer_rect(ox, oy));
        }
        self.pointer = Some((x, y));
        self.damage = self.damage.union(&pointer_rect(x, y));
        self.pending_pointer = true;
    }

    /// After a frame: tell the surface under the pointer where it is.
    fn pointer_moved_or_pressed(&mut self) {
        if !core::mem::take(&mut self.pending_pointer) {
            return;
        }
        let Some((x, y)) = self.pointer else { return };
        let Some(client) = self.under(x, y) else {
            return;
        };
        let Some(area) = self.clients[client]
            .as_ref()
            .and_then(|c| c.surface.as_ref())
            .map(Surface::area)
        else {
            return;
        };
        let m = gui::Pointer {
            dropped: 0,
            x: (x - area.x) as u32,
            y: (y - area.y) as u32,
            buttons: Flags(self.buttons),
        };
        self.send_event(client, m);
    }

    /// The topmost client whose surface holds the point.
    fn under(&self, x: i64, y: i64) -> Option<usize> {
        self.order.iter().rev().copied().find(|i| {
            self.clients[*i]
                .as_ref()
                .and_then(|c| c.surface.as_ref())
                .is_some_and(|s| s.area().contains(x, y))
        })
    }

    fn click(&mut self) {
        let Some((x, y)) = self.pointer else { return };
        if let Some(client) = self.under(x, y) {
            self.raise(client);
            self.set_focus(Some(client));
        }
    }

    fn raise(&mut self, client: usize) {
        if let Some(at) = self.order.iter().position(|i| *i == client)
            && at + 1 != self.order.len()
        {
            self.order.remove(at);
            self.order.push(client);
            if let Some(frame) = self.frame_of(client) {
                self.damage = self.damage.union(&frame);
            }
        }
    }

    fn frame_of(&self, client: usize) -> Option<Rect> {
        self.clients[client]
            .as_ref()?
            .surface
            .as_ref()
            .map(Surface::frame)
    }

    /// F12: the next surface in creation order after the focused one.
    fn next_focus(&mut self) {
        let mut surfaces: Vec<(u64, usize)> = self
            .clients
            .iter()
            .enumerate()
            .filter_map(|(i, c)| {
                c.as_ref()
                    .filter(|c| c.surface.is_some())
                    .map(|c| (c.created, i))
            })
            .collect();
        if surfaces.is_empty() {
            return;
        }
        surfaces.sort_unstable();
        let next = match self
            .focus
            .and_then(|f| surfaces.iter().position(|(_, i)| *i == f))
        {
            Some(at) => surfaces[(at + 1) % surfaces.len()].1,
            None => surfaces[0].1,
        };
        self.raise(next);
        self.set_focus(Some(next));
    }

    fn set_focus(&mut self, client: Option<usize>) {
        if self.focus == client {
            return;
        }
        let old = self.focus;
        self.focus = client;
        for (c, focused) in [(old, false), (client, true)] {
            if let Some(c) = c {
                if let Some(frame) = self.frame_of(c) {
                    self.damage = self.damage.union(&frame);
                }
                self.send_event(
                    c,
                    gui::Focus {
                        dropped: 0,
                        focused,
                    },
                );
            }
        }
    }

    /// Sends an event; a full queue drops it and counts it for that client
    /// alone (INV-0094-3).
    fn send_event<M>(&mut self, client: usize, mut m: M)
    where
        M: for<'a> Message<'a> + WithDropped,
    {
        let Some(c) = self.clients[client].as_mut() else {
            return;
        };
        m.set_dropped(c.dropped);
        let length = reply(&m, NO_REPLY, &mut self.out);
        match self.platform.write(c.end, &self.out[..length], &[]) {
            Outgoing::Sent => c.dropped = 0,
            Outgoing::Full => c.dropped = c.dropped.saturating_add(1),
            // Its close arrives as a message of its own.
            Outgoing::Closed => {}
        }
    }

    // ---- clients ----------------------------------------------------------

    fn manager_message(&mut self) {
        let mut message = core::mem::take(&mut self.message);
        let mut handles = core::mem::take(&mut self.handles);
        handles.clear();
        let incoming = self.platform.read(self.manager, &mut message, &mut handles);
        let Incoming::Message(n) = incoming else {
            for h in handles.drain(..) {
                self.platform.close(h);
            }
            self.message = message;
            self.handles = handles;
            return;
        };
        let bytes = &message[..n];
        let (header, _) = Header::decode(bytes).unwrap_or((
            Header {
                ordinal: u32::MAX,
                transaction: 0,
            },
            &[],
        ));
        let one = handles.len() == 1;
        let (s, used) = match header.ordinal {
            32 if one && decode::<gui::AddClient>(bytes, 1).is_ok() => {
                (self.add_client(handles[0]), true)
            }
            33 if one && decode::<gui::AttachDisplay>(bytes, 1).is_ok() => {
                let h = handles[0];
                let s = if self.platform.kind(h) == Kind::Channel {
                    self.attach_display(h)
                } else {
                    self.platform.close(h);
                    status::UNSUPPORTED
                };
                (s, true)
            }
            34 if one && decode::<gui::AttachInput>(bytes, 1).is_ok() => {
                let h = handles[0];
                let s = if self.platform.kind(h) == Kind::Channel {
                    self.attach_input(h)
                } else {
                    self.platform.close(h);
                    status::UNSUPPORTED
                };
                (s, true)
            }
            35 if handles.is_empty() && decode::<gui::Stats>(bytes, 0).is_ok() => {
                let s = self.stats;
                let m = gui::StatsReply {
                    frames: s.frames,
                    commits: s.commits,
                    compose_ns: s.compose_ns,
                    flush_ns: s.flush_ns,
                    flushes: s.flushes,
                    hangs: s.hangs,
                };
                let length = reply(&m, header.transaction, &mut self.out);
                self.platform.write(self.manager, &self.out[..length], &[]);
                self.message = message;
                self.handles = handles;
                return;
            }
            _ => (status::UNSUPPORTED, false),
        };
        if used {
            // The one handle went to whatever took it.
            handles.clear();
        } else {
            for h in handles.drain(..) {
                self.platform.close(h);
            }
        }
        if header.transaction != NO_REPLY {
            let m = gui::ManagerDone { status: Below(s) };
            let length = reply(&m, header.transaction, &mut self.out);
            self.platform.write(self.manager, &self.out[..length], &[]);
        }
        self.message = message;
        self.handles = handles;
    }

    fn add_client(&mut self, end: P::End) -> u32 {
        if self.platform.kind(end) != Kind::Channel {
            self.platform.close(end);
            return status::UNSUPPORTED;
        }
        let Some(slot) = self.clients.iter().position(Option::is_none) else {
            self.platform.close(end);
            return status::NO_MEMORY;
        };
        if !self.platform.watch(end, CLIENT_KEY + slot as u64) {
            self.platform.close(end);
            return status::NO_MEMORY;
        }
        self.clients[slot] = Some(Client {
            end,
            surface: None,
            commit: None,
            dropped: 0,
            created: 0,
        });
        status::OK
    }

    fn drop_client(&mut self, slot: usize) {
        let Some(c) = self.clients[slot].take() else {
            return;
        };
        if let Some(s) = c.surface {
            self.platform.unmap(s.address, s.length);
            self.damage = self.damage.union(&s.frame());
        }
        self.platform.close(c.end);
        self.order.retain(|i| *i != slot);
        if self.focus == Some(slot) {
            self.focus = None;
            let top = self.order.last().copied();
            self.set_focus(top);
        }
    }

    fn client_message(&mut self, slot: usize) {
        let Some(c) = self.clients[slot].as_ref() else {
            return;
        };
        // One commit at a time: its reply comes at the end of the round.
        if c.commit.is_some() {
            return;
        }
        let end = c.end;
        let mut message = core::mem::take(&mut self.message);
        let mut handles = core::mem::take(&mut self.handles);
        handles.clear();
        let incoming = self.platform.read(end, &mut message, &mut handles);
        match incoming {
            Incoming::Message(n) => self.request(slot, &message[..n], &mut handles),
            Incoming::Empty => {}
            Incoming::Closed | Incoming::Refused => self.drop_client(slot),
        }
        for h in handles.drain(..) {
            self.platform.close(h);
        }
        self.message = message;
        self.handles = handles;
    }

    fn answer<'a, M: Message<'a>>(&mut self, slot: usize, m: &M, transaction: u32) {
        let Some(c) = self.clients[slot].as_mut() else {
            return;
        };
        if transaction == NO_REPLY {
            return;
        }
        let length = reply(m, transaction, &mut self.out);
        // A client whose queue is full loses the reply and stalls only
        // itself (INV-0093-5). It is not an event, so it is not counted as
        // a dropped one (review R10).
        let _ = self.platform.write(c.end, &self.out[..length], &[]);
    }

    fn request(&mut self, slot: usize, bytes: &[u8], handles: &mut Vec<P::End>) {
        let Ok((header, _)) = Header::decode(bytes) else {
            self.drop_client(slot);
            return;
        };
        let t = header.transaction;
        let done = |s: u32| gui::Done { status: Below(s) };
        match header.ordinal {
            1 if decode::<gui::Display>(bytes, handles.len()).is_ok() => {
                let s = if self.attached {
                    status::OK
                } else {
                    status::NO_SURFACE
                };
                let m = gui::DisplayReply {
                    status: Below(s),
                    width: self.width,
                    height: self.height,
                };
                self.answer(slot, &m, t);
            }
            2 => match decode::<gui::Create>(bytes, handles.len()) {
                Ok((_, m)) => {
                    let memory = handles.remove(0);
                    let s = self.create(slot, m.width, m.height, memory);
                    self.answer(slot, &done(s), t);
                }
                Err(_) => self.answer(slot, &done(status::BAD_BUFFER), t),
            },
            3 => match decode::<gui::Commit>(bytes, handles.len()) {
                Ok((_, m)) => {
                    let s = self.commit(slot, m);
                    if s == status::OK {
                        if let Some(c) = self.clients[slot].as_mut() {
                            c.commit = Some(t);
                        }
                    } else {
                        let m = gui::FrameDone { status: Below(s) };
                        self.answer(slot, &m, t);
                    }
                }
                Err(_) => {
                    let m = gui::FrameDone {
                        status: Below(status::BAD_RANGE),
                    };
                    self.answer(slot, &m, t);
                }
            },
            // Anything else, the manager's and the drivers' ordinals among
            // them, is unknown here (INV-0093-2).
            _ => self.answer(
                slot,
                &ErrorReply {
                    status: status::UNSUPPORTED,
                },
                t,
            ),
        }
    }

    fn create(&mut self, slot: usize, width: u32, height: u32, memory: P::End) -> u32 {
        let Some(c) = self.clients[slot].as_ref() else {
            return status::UNSUPPORTED;
        };
        if c.surface.is_some() {
            self.platform.close(memory);
            return status::EXISTS;
        }
        if !self.attached {
            self.platform.close(memory);
            return status::NO_SURFACE;
        }
        let fits = (1..=self.width).contains(&width) && (1..=self.height).contains(&height);
        if !fits || self.platform.kind(memory) != Kind::Memory {
            self.platform.close(memory);
            return status::BAD_BUFFER;
        }
        let pixels = width as usize * height as usize;
        let length = (pixels as u64 * 4).next_multiple_of(4096);
        // A smaller object fails the mapping (INV-0093-3).
        let address = self.platform.map(memory, length);
        self.platform.close(memory);
        let Some(address) = address else {
            return status::BAD_BUFFER;
        };
        let mut copy = Vec::new();
        if copy.try_reserve_exact(pixels).is_err() {
            self.platform.unmap(address, length);
            return status::NO_MEMORY;
        }
        copy.resize(pixels, 0);
        let (x, y) = self.place(width, height);
        let created = self.created;
        self.created += 1;
        if let Some(c) = self.clients[slot].as_mut() {
            c.surface = Some(Surface {
                width,
                height,
                x,
                y,
                address,
                length,
                copy,
            });
            c.created = created;
        }
        self.order.push(slot);
        if let Some(frame) = self.frame_of(slot) {
            self.damage = self.damage.union(&frame);
        }
        // A new surface takes focus only if none has it (ADR-0094).
        if self.focus.is_none() {
            self.set_focus(Some(slot));
        }
        status::OK
    }

    /// The first window at `ORIGIN`, later ones cascaded, kept on screen.
    fn place(&self, width: u32, height: u32) -> (i64, i64) {
        let n = self.order.len() as i64;
        let fit = |start: i64, side: u32, screen: u32| -> i64 {
            let room = i64::from(screen) - i64::from(side) - 1;
            if room < 1 {
                1.min(room.max(0))
            } else {
                start.min(room)
            }
        };
        let x = fit(ORIGIN.0 + CASCADE * n, width, self.width);
        let y = fit(ORIGIN.1 + CASCADE * n, height, self.height);
        (x, y)
    }

    /// Copies a committed rectangle into the client's own copy, once
    /// (INV-0093-4). The rectangle must be inside the surface.
    fn commit(&mut self, slot: usize, m: gui::Commit) -> u32 {
        let Some(c) = self.clients[slot].as_mut() else {
            return status::UNSUPPORTED;
        };
        let Some(s) = c.surface.as_mut() else {
            return status::NO_SURFACE;
        };
        let Some(r) = Rect::from_client(m.x, m.y, m.width, m.height, s.width, s.height) else {
            return status::BAD_RANGE;
        };
        let stride = s.width as usize;
        for row in r.y..r.bottom() {
            let at = row as usize * stride + r.x as usize;
            let out = &mut s.copy[at..at + r.width as usize];
            let address = s.address + at as u64 * 4;
            if !self.platform.copy_from(address, out) {
                return status::BAD_BUFFER;
            }
        }
        let screen = r.translate(s.x, s.y);
        self.damage = self.damage.union(&screen);
        self.stats.commits += 1;
        status::OK
    }

    // ---- composition ------------------------------------------------------

    fn finish_round(&mut self) {
        let damage = self.damage.intersect(&self.screen());
        self.damage = Rect::default();
        if !damage.is_empty() {
            let start = self.platform.now();
            self.compose(damage);
            self.stats.compose_ns += self.platform.now().saturating_sub(start);
            self.stats.frames += 1;
            self.unflushed = self.unflushed.union(&damage);
        }
        // The frame callback: the commit is copied and composed. It does
        // not wait for the display driver (ADR-0093, Decision).
        for slot in 0..MAX_CLIENTS {
            let Some(t) = self.clients[slot].as_mut().and_then(|c| c.commit.take()) else {
                continue;
            };
            let m = gui::FrameDone {
                status: Below(status::OK),
            };
            self.answer(slot, &m, t);
        }
        self.send_flush();
    }

    /// Sends what is composed and not yet shown, if no flush is in flight:
    /// one at a time, and frames composed meanwhile go together in the next.
    fn send_flush(&mut self) {
        let Some(end) = self.display else { return };
        if self.in_flight.is_some() || self.unflushed.is_empty() {
            return;
        }
        let r = self.unflushed;
        let m = display::Flush {
            x: r.x as u32,
            y: r.y as u32,
            width: r.width as u32,
            height: r.height as u32,
        };
        let length = reply(&m, 3, &mut self.out);
        match self.platform.write(end, &self.out[..length], &[]) {
            Outgoing::Sent => {
                self.unflushed = Rect::default();
                self.in_flight = Some(self.platform.now());
                self.stuck_since = None;
            }
            // Tried again at the end of the next round, which comes within
            // `RETRY_NS`; a driver that never makes room is hung.
            Outgoing::Full => {
                if self.stuck_since.is_none() {
                    self.stuck_since = Some(self.platform.now());
                }
            }
            Outgoing::Closed => self.lose_display(),
        }
    }

    /// Recomposes `r` of the screen from the copies: background, windows
    /// bottom to top with their frames, then the pointer.
    fn compose(&mut self, r: Rect) {
        let width = self.width as usize;
        let height = self.height as usize;
        let focus = self.focus;
        let Compositor {
            platform,
            clients,
            order,
            pointer,
            ..
        } = self;
        let pixels = platform.pixels();
        if pixels.len() < width * height {
            return;
        }
        for y in r.y..r.bottom() {
            let row = y as usize * width;
            pixels[row + r.x as usize..row + r.right() as usize].fill(BACKGROUND);
        }
        for &i in order.iter() {
            let Some(s) = clients[i].as_ref().and_then(|c| c.surface.as_ref()) else {
                continue;
            };
            let border = if focus == Some(i) {
                BORDER_FOCUSED
            } else {
                BORDER
            };
            let frame = s.frame().intersect(&r);
            for y in frame.y..frame.bottom() {
                let row = y as usize * width;
                pixels[row + frame.x as usize..row + frame.right() as usize].fill(border);
            }
            let area = s.area().intersect(&r);
            for y in area.y..area.bottom() {
                let row = y as usize * width;
                let sy = (y - s.y) as usize;
                let sx = (area.x - s.x) as usize;
                let from = sy * s.width as usize + sx;
                let count = area.width as usize;
                pixels[row + area.x as usize..row + area.x as usize + count]
                    .copy_from_slice(&s.copy[from..from + count]);
            }
        }
        if let Some((px, py)) = *pointer {
            for (dy, line) in POINTER.iter().enumerate() {
                for (dx, dot) in line.iter().enumerate() {
                    let (x, y) = (px + dx as i64, py + dy as i64);
                    if *dot == 0 || !r.contains(x, y) {
                        continue;
                    }
                    pixels[y as usize * width + x as usize] =
                        if *dot == 1 { 0x00ff_ffff } else { 0 };
                }
            }
        }
    }
}

fn pointer_rect(x: i64, y: i64) -> Rect {
    Rect::new(x, y, POINTER[0].len() as i64, POINTER.len() as i64)
}

/// Events carry the count dropped before them.
pub trait WithDropped {
    fn set_dropped(&mut self, dropped: u32);
}

impl WithDropped for gui::Key {
    fn set_dropped(&mut self, dropped: u32) {
        self.dropped = dropped;
    }
}

impl WithDropped for gui::Pointer {
    fn set_dropped(&mut self, dropped: u32) {
        self.dropped = dropped;
    }
}

impl WithDropped for gui::Focus {
    fn set_dropped(&mut self, dropped: u32) {
        self.dropped = dropped;
    }
}

#[cfg(test)]
extern crate std;

#[cfg(test)]
mod tests;

// A runaway test fails at 512 MiB instead of taking the host (docs/backlog.md).
#[cfg(test)]
zoraya_test_alloc::cap_memory!();
