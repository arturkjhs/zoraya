//! The compositor (ADR-0093, ADR-0094, ADR-0160, ADR-0164) as a loop over a
//! [`Platform`], so that it runs on the host against hostile clients.
//!
//! Clients draw into memory objects of their own and commit damage. The
//! compositor copies each committed rectangle once into a copy it owns,
//! composes the screen only from its copies and its own drawing, flushes
//! one rectangle per round to the display driver, and then replies to each
//! commit: the frame callback. It never waits for a client.
//!
//! Stage 7b (ADR-0160): every toplevel has a frame the compositor draws
//! from the tokens (title bar, title, close and minimise buttons, border,
//! rounded corners, shadow); the person moves, resizes, minimises, restores
//! and closes windows; a client may have several windows and popups; the
//! system key's chords, Alt+Tab and F12 are the compositor's, and Alt+Tab
//! opens the switcher overlay in the trusted material; the top bar is the
//! compositor's own. Sizes sent to clients are logical, at one scale for
//! the display (ADR-0164).

#![no_std]
#![forbid(unsafe_code)]

extern crate alloc;

mod draw;
pub mod draw_rooms;
pub mod fields;
pub mod rect;
pub mod rooms;
pub mod settings;
pub mod window;

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use zoraya_keymap::{Keymap, code};
use zoraya_proto_display as display;
use zoraya_proto_gui::{self as gui, status};
use zoraya_proto_input as input;
use zoraya_protocol::{Below, ErrorReply, Flags, Header, Message, NO_REPLY, decode, encode};
use zoraya_text::{Font, FontList, Text};
use zoraya_tokens::{Appearance, Scale, space};

pub use rect::Rect;
pub use rooms::{Cell, DockApp, DockGeometry, Record};
use window::Buffer;
pub use window::{Frame, Window, WindowKind, Zone};

/// Client connections at once `[assumption, ADR-0093]`.
pub const MAX_CLIENTS: usize = 8;
/// Input drivers at once.
pub const MAX_INPUTS: usize = 4;
/// Windows of all clients at once, popups included.
pub const MAX_WINDOWS: usize = 32;
/// Toplevel windows one client may have `[assumption]`.
pub const MAX_TOPLEVELS: usize = 8;
/// Where the first window's frame goes, in logical pixels from the top
/// left of the work area (below the bar), and how far each later one
/// moves: the mockups' terminal window is at (72, 70).
pub const ORIGIN: (u32, u32) = (72, 42);
pub const CASCADE: u32 = 32;
/// How long a client has to close after the person asked, before the
/// close button offers to close it anyway (ADR-0160, an assumption).
pub const CLOSE_GRACE_NS: u64 = 5_000_000_000;
/// The display width from which the scale is 2 (ADR-0164's `auto`).
pub const SCALE_TWO_FROM: u32 = 1920;
/// The density, in pixels per inch, from which a display is dense (a
/// Retina screen is about 220, a desktop monitor about 100), and the width
/// from which a dense display gets scale 2: 640 logical pixels (proposed
/// ADR-0247).
pub const DENSE_PPI: u64 = 150;
pub const DENSE_SCALE_TWO_FROM: u32 = 1280;

/// Port keys: the manager, the display, inputs from `INPUT_KEY`, clients
/// from `CLIENT_KEY`.
pub const MANAGER_KEY: u64 = 1;
pub const DISPLAY_KEY: u64 = 2;
pub const CLIENT_MANAGER_KEY: u64 = 3;
pub const INPUT_KEY: u64 = 16;
pub const CLIENT_KEY: u64 = 64;
/// How long the display driver may take to answer a flush before the
/// compositor drops it and tells the manager (ADR-0093, Decision).
pub const FLUSH_DEADLINE_NS: u64 = 2_000_000_000;
/// The keys whose chord with the system key is the compositor's (ADR-0160,
/// ADR-0163, ADR-0168, ADR-0170, ADR-0172): with the system key, every other
/// key reaches the focused client (ADR-0248, INV-0248-1). A new
/// chord goes here, and is checked against the applications' keys.
pub const TAKEN_CHORDS: [u32; 11] = [
    code::ENTER,
    code::SLASH,
    code::SPACE,
    code::C,
    code::X,
    code::V,
    code::W,
    code::M,
    code::LEFT,
    code::RIGHT,
    code::R,
];

/// How long a new sheet ignores the keyboard and the pointer's buttons, so
/// that a key the person was about to press for the window below does not
/// confirm a request timed to meet it (ADR-0245, the architect's condition).
pub const SHEET_GUARD_NS: u64 = 500_000_000;
/// How long after a press or key in its focused window a client may offer
/// the clipboard unasked (ADR-0248).
pub const GESTURE_OFFER_NS: u64 = 1_000_000_000;

/// What a new sheet ignores: everything until `until`, then the keys and
/// buttons held down when it mapped (or pressed before `until`), each until
/// its release. `until` starts again at the sheet's first commit, when it
/// can first be seen (review of the fix branch, 7).
#[derive(Debug, Clone, Copy)]
struct SheetGuard {
    sheet: Key,
    until: u64,
    keys: [u64; 8],
    buttons: u32,
    shown: bool,
}
/// How often a flush that found the driver's queue full is tried again.
pub const RETRY_NS: u64 = 10_000_000;

/// The scale `auto` gives a display `width` physical pixels wide whose
/// width is `width_mm` millimetres (0: unknown): 2 from `SCALE_TWO_FROM`
/// pixels (ADR-0164), or from `DENSE_SCALE_TWO_FROM` on a display of at
/// least `DENSE_PPI` (ADR-0247: a Retina window).
pub const fn scale_for(width: u32, width_mm: u32) -> Scale {
    // pixels / (mm / 25.4) >= ppi, in integers.
    let dense = width_mm > 0 && width as u64 * 254 >= DENSE_PPI * 10 * width_mm as u64;
    if width >= SCALE_TWO_FROM || (dense && width >= DENSE_SCALE_TWO_FROM) {
        Scale::TWO
    } else {
        Scale::ONE
    }
}

/// Where the first window's body lies on a display at `scale`: the frame
/// at `ORIGIN` below the bar, the title bar above the body. The GUI test
/// reads the terminal there.
pub fn first_body(scale: Scale) -> (i64, i64) {
    let f = Frame::at(scale);
    let s = i64::from(scale.factor());
    let bar = i64::from(space::BAR_HEIGHT.physical(scale));
    (
        i64::from(ORIGIN.0) * s + f.border,
        bar + i64::from(ORIGIN.1) * s + f.title,
    )
}

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
    /// The display driver shows the framebuffer made last: an older one may
    /// be released (ADR-0246; a window dragged larger makes one per step).
    fn framebuffer_shown(&mut self) {}
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
    /// A line for the console (the timings below); by default nothing.
    fn note(&mut self, _line: &str) {}
}

struct Client<E> {
    end: E,
    /// A commit waiting for the round's flush: its transaction and its
    /// reply's ordinal (`FrameDone` or `CommitDone`).
    commit: Option<(u32, u32)>,
    /// Events dropped since the last that reached it.
    dropped: u32,
    /// The id its next window gets (from 1).
    next_id: u32,
    /// Its launch record, from the manager only (INV-0172-1).
    record: Record,
    /// Marked by the driver manager's `AddSystemClient` (the chooser): it
    /// may create sheets and report grants (proposed ADR-0245).
    system: bool,
    /// Left presses on its bodies delivered to it: its own count, so that
    /// no client learns of the presses another got (review 7e, 4).
    presses: u32,
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
    /// The last wallpaper and its blur, made at a display's attach.
    pub wallpaper_ns: u64,
    /// The first frame composed after it: the whole screen.
    pub first_frame_ns: u64,
    /// The frames after the first, until 100 of them are reported.
    pub steady_frames: u64,
    pub steady_ns: u64,
    pub steady_max_ns: u64,
}

/// How many frames after the first are timed and reported.
const STEADY_FRAMES: u64 = 100;

/// A window, named by its client's slot and its id.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Key {
    pub client: usize,
    pub id: u32,
}

/// One room's group line in the switcher, from the compositor's records.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SwitchGroup {
    pub name: String,
    /// `None` for `home`.
    pub colour: Option<zoraya_tokens::Color>,
    pub untrusted: bool,
    /// The room's grants in ADR-0153's words.
    pub grants: String,
}

/// How long after its press a drag may name it (ADR-0170).
const DRAG_WITHIN_NS: u64 = 500_000_000;
/// How long after a press a client may name it in `Drag`, while it is still
/// held: ADR-0170's 500 ms was an assumption, and a file manager repainting
/// under TCG answers a press later than that (ADR-0248; the press
/// must still be held, so nothing is taken after the person let go).
const DRAG_NAMES_WITHIN_NS: u64 = 3_000_000_000;

/// The dragged entry's label's widest, logical pixels: 64 characters of
/// the caption face, with the material's padding.
const CARRIED_WIDTH: i64 = 520;

/// What a held pointer button is doing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Drag {
    /// Moving the window: the pointer's offset from the body's top left.
    Move { window: Key, dx: i64, dy: i64 },
    /// Resizing: the sides that move, and the body when the drag began.
    Resize {
        window: Key,
        left: bool,
        top: bool,
        right: bool,
        bottom: bool,
        start: (i64, i64),
        body: Rect,
    },
}

/// An entry being dragged (ADR-0170): from whose window, the connection
/// and its display name.
struct Carried<E> {
    source: Key,
    entry: E,
    name: String,
    /// The label's logical width, measured once: the damaged room
    /// follows it (a 64-byte name can be wider than `CARRIED_WIDTH`).
    width: i64,
}

/// The switcher overlay while open (ADR-0160): the windows in the order
/// shown, and the chosen one.
pub struct Switcher {
    pub windows: Vec<Key>,
    pub chosen: usize,
    /// Each window's group (ADR-0160, ADR-0172 point 5): an index of
    /// `groups`; windows of a group are together.
    pub group: Vec<usize>,
    pub groups: Vec<SwitchGroup>,
    /// Each window's pixels shrunk, made when it opens and when that window
    /// commits, never per frame (ADR-0160).
    pub thumbs: Vec<Vec<u32>>,
}

pub struct Compositor<P: Platform> {
    platform: P,
    manager: P::End,
    /// The panel's channel (ADR-0168): launch records, rooms, the dock.
    client_manager: Option<P::End>,
    /// The room table and the current room, from the panel (ADR-0172).
    rooms: zoraya_settings::rooms::Rooms,
    current_room: String,
    /// The dock's applications, from the panel.
    dock_apps: Vec<DockApp>,
    dock: DockGeometry,
    /// What the dock showed when last drawn.
    dock_sig: String,
    /// The application icon under the pointer: its tooltip is shown.
    hover: Option<usize>,
    /// Show-commands mode (System+?, ADR-0172 point 8).
    commands: bool,
    /// The wallpaper with the rooms' fields on it.
    fielded: Vec<u32>,
    fields_dirty: bool,
    display: Option<P::End>,
    inputs: Vec<Option<P::End>>,
    clients: Vec<Option<Client<P::End>>>,
    /// Every window, bottom of the stack first.
    windows: Vec<Window>,
    focus: Option<Key>,
    /// The display in physical pixels, its width in millimetres (0:
    /// unknown) and its scale.
    width: u32,
    height: u32,
    width_mm: u32,
    scale: Scale,
    appearance: Appearance,
    /// `gui.motion` (ADR-0169), told to clients in `Configure`.
    motion: bool,
    /// `gui.scale`: 0 is `auto` (ADR-0164); read at start, and applied at once
    /// by `ApplySettings` (ADR-0246).
    fixed_scale: u32,
    attached: bool,
    keymap: Keymap,
    /// The system key is held (either side).
    meta: u8,
    /// Where the pointer is, once a pointer has spoken.
    pointer: Option<(i64, i64)>,
    buttons: u32,
    drag: Option<Drag>,
    /// The window whose body holds the left button the compositor
    /// delivered to it: the only one that may start a drag (ADR-0170).
    body_press: Option<(Key, u32, u64)>,

    /// The entry being dragged; no client hears of the pointer meanwhile.
    carried: Option<Carried<P::End>>,
    /// The clipboard's text (ADR-0170): set only by an `Offer` answering a
    /// `Copy`, given only in a `Paste` on System+V.
    clipboard: String,
    /// The client asked to copy, and when: its `Offer` is taken then.
    copy_asked: Option<(usize, u64)>,
    /// The last press or key the compositor delivered to a client's focused
    /// window, and when: one unasked `Offer` is taken within
    /// `GESTURE_OFFER_NS` of it (ADR-0248, INV-0248-2).
    gesture: Option<(usize, u64)>,
    /// The pointer moved or a button changed this frame.
    pending_pointer: bool,
    switcher: Option<Switcher>,
    /// The screen as last composed with the switcher open: a pointer that
    /// only moves over it is drawn on this, not on a new blur.
    switcher_frame: Vec<u32>,
    /// Something but the pointer changed this round.
    scene: bool,
    /// Keys whose press reached a client: their release reaches it too,
    /// even during a chord or the switcher (review 7b, 12).
    delivered: [u64; 8],
    /// Every key down now, by code (below 512).
    held: [u64; 8],
    /// The newest sheet's guard (ADR-0245).
    sheet_guards: Vec<SheetGuard>,
    /// The enabled keyboard layouts, indices of `zoraya_keymap::LAYOUTS`,
    /// in switching order (ADR-0163: `gui.keyboard-layouts`, a setting from
    /// 7f; `us ua cz` until then).
    layouts: Vec<usize>,
    /// What changed on the screen this round.
    damage: Rect,
    /// Composed and not yet sent to the display driver.
    unflushed: Rect,
    /// When the one flush in flight was sent.
    in_flight: Option<u64>,
    /// The driver sent `Resized` while a flush was in flight: the new size
    /// is asked for once that flush is answered.
    resized: bool,
    /// Since when a flush has been waiting for room in the driver's queue.
    stuck_since: Option<u64>,
    stats: Stats,
    /// The wallpaper and its blurred copy, made when the display attaches.
    wallpaper: Vec<u32>,
    blurred: Vec<u32>,
    ui: Text<'static>,
    title_text: Text<'static>,
    mono: Text<'static>,
    message: Vec<u8>,
    handles: Vec<P::End>,
    /// Events and replies, encoded: as large as any message (a `Paste`
    /// carries the clipboard's 32 KiB).
    out: Vec<u8>,
}

fn reply<'a, M: Message<'a>>(m: &M, transaction: u32, out: &mut [u8]) -> usize {
    // Every message here is small and fixed: encoding cannot fail.
    encode(m, transaction, out).unwrap_or(0)
}

fn fonts(primary: &'static [u8]) -> FontList<'static> {
    FontList::new(
        [primary, zoraya_fonts::MONO_REGULAR]
            .into_iter()
            .filter_map(|d| Font::parse(d).ok()),
    )
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
            client_manager: None,
            rooms: zoraya_settings::rooms::Rooms::default(),
            current_room: String::new(),
            dock_apps: Vec::new(),
            dock: DockGeometry::default(),
            dock_sig: String::new(),
            hover: None,
            commands: false,
            fielded: Vec::new(),
            fields_dirty: true,
            display: None,
            inputs: vec![None; MAX_INPUTS],
            clients: (0..MAX_CLIENTS).map(|_| None).collect(),
            windows: Vec::new(),
            focus: None,
            width: 0,
            height: 0,
            width_mm: 0,
            scale: Scale::ONE,
            appearance: Appearance::Light,
            motion: true,
            fixed_scale: 0,
            attached: false,
            keymap: Keymap::new(),
            meta: 0,
            pointer: None,
            buttons: 0,
            drag: None,
            body_press: None,
            clipboard: String::new(),
            copy_asked: None,
            gesture: None,
            carried: None,
            pending_pointer: false,
            switcher: None,
            switcher_frame: Vec::new(),
            scene: false,
            delivered: [0; 8],
            held: [0; 8],
            sheet_guards: Vec::new(),
            layouts: ["us", "ua", "cz"]
                .iter()
                .filter_map(|n| zoraya_keymap::by_name(n))
                .collect(),
            damage: Rect::default(),
            unflushed: Rect::default(),
            in_flight: None,
            resized: false,
            stuck_since: None,
            stats: Stats::default(),
            wallpaper: Vec::new(),
            blurred: Vec::new(),
            ui: Text::new(fonts(zoraya_fonts::INTER_REGULAR)),
            title_text: Text::new(fonts(zoraya_fonts::INTER_MEDIUM)),
            mono: Text::new(fonts(zoraya_fonts::MONO_REGULAR)),
            message: vec![0; zoraya_protocol::MAX_WIRE_MESSAGE],
            handles: Vec::new(),
            out: vec![0; zoraya_protocol::MAX_WIRE_MESSAGE],
        };
        if let Some(d) = display {
            c.attach_display(d);
        }
        for i in inputs {
            c.attach_input(*i);
        }
        c
    }

    /// As [`Compositor::new`], with the settings read at start (ADR-0169):
    /// the scale they fix applies to the display attached here.
    pub fn with_settings(
        platform: P,
        manager: P::End,
        display: Option<P::End>,
        inputs: &[P::End],
        s: settings::GuiSettings,
    ) -> Compositor<P> {
        let mut c = Compositor::new(platform, manager, None, inputs);
        c.fixed_scale = s.scale;
        c.apply_settings(s);
        if let Some(d) = display {
            c.attach_display(d);
        }
        c
    }

    /// Applies checked settings: the theme to the frames, the wallpaper's
    /// blur and every window's `Configure`, motion likewise, the layouts
    /// and the active one, and the scale (ADR-0246), all at once.
    pub fn apply_settings(&mut self, s: settings::GuiSettings) {
        let appearance = if s.dark {
            Appearance::Dark
        } else {
            Appearance::Light
        };
        let restyle = appearance != self.appearance || s.motion != self.motion;
        self.appearance = appearance;
        self.motion = s.motion;
        self.fixed_scale = s.scale;
        let rescale = self.attached && self.chosen_scale() != self.scale;
        if (restyle || rescale) && self.attached {
            self.fields_dirty = true;
            if rescale {
                // Every toplevel at its logical size at the new scale, and
                // configured (`reconfigure`: popups and sheets close).
                let old = self.scale.factor();
                self.scale = self.chosen_scale();
                self.reconfigure(old);
            } else {
                let keys: Vec<Key> = self.windows.iter().map(Self::key_of).collect();
                for k in keys {
                    self.configure(k);
                }
            }
            // At the scale now in force: the blur's radius follows it.
            self.make_wallpaper();
            let screen = self.screen();
            self.dirty(&screen);
        }
        let layout_changed = s.layouts != self.layouts || s.layout != self.keymap.layout_index();
        self.layouts = s.layouts;
        self.keymap.set_layout(s.layout);
        if layout_changed {
            self.layout_changed();
        }
    }

    pub fn platform(&mut self) -> &mut P {
        &mut self.platform
    }

    /// The display in physical pixels.
    pub const fn size(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    pub const fn scale(&self) -> Scale {
        self.scale
    }

    pub const fn stats(&self) -> Stats {
        self.stats
    }

    /// The window that has keyboard focus.
    pub const fn focused(&self) -> Option<Key> {
        self.focus
    }

    /// The client that has keyboard focus, by index.
    pub fn appearance(&self) -> Appearance {
        self.appearance
    }

    pub fn focus(&self) -> Option<usize> {
        self.focus.map(|k| k.client)
    }

    /// Where a client's first visible window's body is on the screen.
    pub fn placement(&self, client: usize) -> Option<Rect> {
        self.windows
            .iter()
            .find(|w| w.client == client)
            .map(Window::body_rect)
    }

    /// A window, if it exists.
    pub fn window(&self, key: Key) -> Option<&Window> {
        self.windows
            .iter()
            .find(|w| w.client == key.client && w.id == key.id)
    }

    /// The windows, bottom of the stack first.
    pub fn stack(&self) -> impl Iterator<Item = Key> + '_ {
        self.windows.iter().map(|w| Key {
            client: w.client,
            id: w.id,
        })
    }

    pub fn switcher(&self) -> Option<&Switcher> {
        self.switcher.as_ref()
    }

    pub fn client_count(&self) -> usize {
        self.clients.iter().flatten().count()
    }

    /// The frame's sizes at the display's scale.
    pub fn frame(&self) -> Frame {
        Frame::at(self.scale)
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
                MANAGER_KEY => self.manager_message(false),
                CLIENT_MANAGER_KEY => self.manager_message(true),
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
        let s = self.size_and_attach(end, false);
        if s != status::OK {
            self.platform.close(end);
            return s;
        }
        self.display = Some(end);
        self.platform.watch(end, DISPLAY_KEY);
        // Everything, once: a new driver shows nothing until told.
        self.dirty(&self.screen());
        self.in_flight = None;
        self.stuck_since = None;
        status::OK
    }

    /// The driver said the display has a new size (ADR-0246), and no flush
    /// is in flight: the same `Info` and `Attach` as at attach, on the same
    /// channel, and everything drawn again.
    fn display_resized(&mut self) {
        self.resized = false;
        let Some(end) = self.display else { return };
        if self.size_and_attach(end, true) != status::OK {
            self.lose_display();
            return;
        }
        self.unflushed = Rect::default();
        self.dirty(&self.screen());
    }

    /// Asks the driver for the display's size, makes a framebuffer of it and
    /// attaches it; at a new size, the scale, the wallpaper and every window
    /// follow (`reconfigure`). `again`: the driver attached already and said
    /// the size changed; if it changed back meanwhile, nothing is attached
    /// (the driver refuses a second framebuffer of the same size), and the
    /// whole-screen flush that follows ends its wait for `Attach`. The caller
    /// closes `end` on a failure.
    fn size_and_attach(&mut self, end: P::End, again: bool) -> u32 {
        let mut answer = [0u8; 64];
        let length = reply(&display::Info {}, 1, &mut self.out);
        let message = self.out.clone();
        let Some(n) = self
            .platform
            .call(end, &message[..length], &[], &mut answer)
        else {
            return status::UNSUPPORTED;
        };
        let info = match decode::<display::InfoReply>(&answer[..n], 0) {
            Ok((_, m)) if m.status.0 == display::status::OK && m.format == display::FORMAT_XRGB => {
                m
            }
            _ => {
                return status::UNSUPPORTED;
            }
        };
        let side = |v: u32| (1..=8192).contains(&v);
        if !side(info.width) || !side(info.height) {
            return status::UNSUPPORTED;
        }
        let new_size = (info.width, info.height) != (self.width, self.height) || !self.attached;
        if again && !new_size {
            return status::OK;
        }
        let Some(frame) = self.platform.framebuffer(info.width, info.height) else {
            return status::NO_MEMORY;
        };
        if new_size {
            self.width = info.width;
            self.height = info.height;
            self.width_mm = info.width_mm;
            self.attached = true;
            let old = self.scale.factor();
            self.scale = self.chosen_scale();
            let start = self.platform.now();
            self.make_wallpaper();
            self.stats.wallpaper_ns = self.platform.now().saturating_sub(start);
            self.stats.first_frame_ns = 0;
            self.stats.steady_frames = 0;
            self.reconfigure(old);
        }
        let length = reply(&display::Attach {}, 2, &mut self.out);
        let message = self.out.clone();
        let done = self
            .platform
            .call(end, &message[..length], &[frame], &mut answer);
        if !Self::done_ok(done, &answer) {
            return status::UNSUPPORTED;
        }
        // The driver shows the new framebuffer: an older one can go.
        self.platform.framebuffer_shown();
        status::OK
    }

    /// The scale for the display's width: `gui.scale` 1 or 2 fixes it, `auto`
    /// is [`scale_for`]'s (ADR-0164, ADR-0246, ADR-0247).
    fn chosen_scale(&self) -> Scale {
        match self.fixed_scale {
            1 => Scale::ONE,
            2 => Scale::new(2).unwrap_or(Scale::ONE),
            _ => scale_for(self.width, self.width_mm),
        }
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
                if self.resized {
                    self.display_resized();
                } else {
                    // The latest frame, whatever was composed meanwhile.
                    self.send_flush();
                }
            }
            // ADR-0246: a new display size. One at a time from the driver,
            // and never while the compositor waits for `Info`'s reply.
            Incoming::Message(n)
                if handles.is_empty()
                    && decode::<display::Resized>(&self.message[..n], 0).is_ok() =>
            {
                if self.in_flight.is_some() {
                    self.resized = true;
                } else {
                    self.display_resized();
                }
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
        self.resized = false;
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
                self.meta = 0;
                self.held = [0; 8];
                self.delivered = [0; 8];
                for g in &mut self.sheet_guards {
                    g.keys = [0; 8];
                    g.buttons = 0;
                }
                self.buttons = 0;
                self.drag = None;
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
            let now = self.platform.now();
            for g in &mut self.sheet_guards {
                if value == 0 {
                    g.buttons &= !button;
                } else if value == 1 && now < g.until {
                    g.buttons |= button;
                }
            }
            if value == 0 {
                self.buttons &= !button;
                if button == gui::buttons::LEFT {
                    self.release();
                }
            } else {
                if value == 1 {
                    self.press(button);
                }
                self.buttons |= button;
            }
            self.pending_pointer = true;
            return;
        }
        if (0x110..0x200).contains(&code_) {
            return;
        }
        if (code_ as usize) < 512 {
            let (word, mask) = (code_ as usize / 64, 1u64 << (code_ % 64));
            let now = self.platform.now();
            if value == 0 {
                self.held[word] &= !mask;
                // A key held from before the sheet mapped counts again
                // once it is released.
                for g in &mut self.sheet_guards {
                    g.keys[word] &= !mask;
                }
            } else {
                self.held[word] |= mask;
                // A key pressed within the guard stays guarded until its
                // release: its repeats after the guard are not the sheet's
                // (review of the fix branch, 1).
                for g in &mut self.sheet_guards {
                    if now < g.until {
                        g.keys[word] |= mask;
                    }
                }
            }
        }
        // The system key: held, and never a client's (INV-0160-3).
        match code_ {
            code::LEFT_META => {
                self.meta = if value != 0 {
                    self.meta | 1
                } else {
                    self.meta & !1
                };
                return;
            }
            code::RIGHT_META => {
                self.meta = if value != 0 {
                    self.meta | 2
                } else {
                    self.meta & !2
                };
                return;
            }
            _ => {}
        }
        // A key the compositor takes (a chord, the switcher's keys) does
        // not touch the dead key the client may be typing (review 7c, 3).
        let dead = self.keymap.dead();
        let key = self.keymap.key(code_, value);
        let taken = self.meta != 0 || self.switcher.is_some() || code_ == code::F12;
        if taken {
            self.keymap.set_dead(dead);
        }
        let alt = key.modifiers & zoraya_keymap::ALT != 0;
        let shift = key.modifiers & zoraya_keymap::SHIFT != 0;
        let bit = (code_ as usize) < 512;
        let (word, mask) = ((code_ as usize / 64) % 8, 1u64 << (code_ % 64));
        if !key.pressed && bit && self.delivered[word] & mask != 0 {
            self.delivered[word] &= !mask;
            if let Some(focus) = self.focus {
                let m = gui::Key {
                    dropped: 0,
                    code: code_,
                    pressed: false,
                    modifiers: Flags(key.modifiers),
                    text: Below(0),
                };
                self.send_event(focus.client, m);
            }
        }
        // Escape ends a drag with nothing delivered (ADR-0170); while one
        // is carried no key reaches a client.
        if let Some(c) = self.carried.take() {
            if key.pressed && code_ == code::ESC {
                // The press that started it starts nothing again.
                self.body_press = None;
                if let Some((x, y)) = self.pointer {
                    self.dirty(&self.carried_rect(x, y));
                }
                self.platform.close(c.entry);
                self.send_event_plain(
                    c.source.client,
                    &gui::DragEndedAt {
                        window: c.source.id,
                        over: Below(0),
                        x: 0,
                        y: 0,
                        modifiers: Flags(0),
                    },
                );
            } else {
                self.carried = Some(c);
            }
            return;
        }
        // Releasing Alt chooses in the open switcher.
        if self.switcher.is_some() && !alt && !key.pressed {
            if matches!(code_, code::LEFT_ALT | code::RIGHT_ALT) {
                self.choose_in_switcher();
            }
            return;
        }
        if self.switcher.is_some() {
            if key.pressed {
                match code_ {
                    code::TAB => self.step_switcher(if shift { -1 } else { 1 }),
                    code::ESC => self.close_switcher(),
                    _ => {}
                }
            }
            return;
        }
        if self.meta != 0 {
            // A chord of the compositor's table is the compositor's alone
            // (INV-0160-3, INV-0248-1).
            if TAKEN_CHORDS.contains(&code_) {
                if key.pressed && value == 1 {
                    self.chord(code_);
                }
                return;
            }
            // Any other reaches the focused client with `SYSTEM` (proposed
            // ADR-0248: Command+Up, Command+Shift+N in the file manager);
            // its release reaches it as every delivered key's does.
            let Some(focus) = self.focus else { return };
            if !key.pressed || self.guards_key(focus, code_) {
                return;
            }
            if bit {
                self.delivered[word] |= mask;
            }
            let m = gui::Key {
                dropped: 0,
                code: code_,
                pressed: true,
                modifiers: Flags((key.modifiers & gui::modifiers::ALL) | gui::modifiers::SYSTEM),
                text: Below(0),
            };
            self.send_event(focus.client, m);
            return;
        }
        if code_ == code::TAB && alt {
            if value == 1 {
                self.open_switcher(if shift { -1 } else { 1 });
            }
            return;
        }
        // F12: the backup for Alt+Tab, the next window at once (INV-0094-1).
        if code_ == code::F12 {
            if value == 1 {
                self.next_window();
            }
            return;
        }
        let Some(focus) = self.focus else { return };
        if !key.pressed {
            // Its press did not reach a client (it began a chord), or its
            // release was sent above.
            return;
        }
        if self.guards_key(focus, code_) {
            return;
        }
        if bit {
            self.delivered[word] |= mask;
        }
        let m = gui::Key {
            dropped: 0,
            code: code_,
            pressed: key.pressed,
            modifiers: Flags(key.modifiers),
            text: Below(key.text.map_or(0, u32::from)),
        };
        self.send_event(focus.client, m);
    }

    /// A chord with the system key (ADR-0160): W closes, M minimises, Left
    /// and Right tile, Space switches the keyboard layout (Shift: back,
    /// ADR-0163); Enter, R and ? are reserved for 7d and reach no client
    /// either.
    fn chord(&mut self, code_: u32) {
        if code_ == code::ENTER {
            self.new_terminal();
            return;
        }
        if code_ == code::SLASH {
            self.commands = !self.commands;
            self.tell_commands();
            self.dock_changed();
            let s = self.screen();
            self.dirty(&s);
            return;
        }
        if code_ == code::SPACE {
            let back = self.keymap.modifiers() & zoraya_keymap::SHIFT != 0;
            let layouts = core::mem::take(&mut self.layouts);
            self.keymap.switch(&layouts, back);
            self.layouts = layouts;
            self.layout_changed();
            return;
        }
        // Copy, cut and paste (ADR-0170), to the focused window itself.
        if let (Some(f), code::C | code::X | code::V) = (self.focus, code_) {
            // Not into an untrusted room's window: its pastes wait for the
            // compositor's confirming card (ADR-0172 point 6, INV-0172-3).
            let untrusted = self
                .clients
                .get(f.client)
                .and_then(Option::as_ref)
                .is_some_and(|c| c.record.untrusted);
            if code_ == code::V {
                if !self.clipboard.is_empty() && !untrusted {
                    let text = core::mem::take(&mut self.clipboard);
                    let m = gui::Paste {
                        window: f.id,
                        text: zoraya_protocol::Data(text.as_bytes()),
                    };
                    self.send_event_plain(f.client, &m);
                    self.clipboard = text;
                }
            } else {
                let now = self.platform.now();
                self.copy_asked = Some((f.client, now));
                // A copy asked is the person's; no gesture's offer takes its
                // place (review 7h, 3).
                self.gesture = None;
                let m = gui::Copy {
                    window: f.id,
                    cut: code_ == code::X,
                };
                self.send_event_plain(f.client, &m);
            }
            return;
        }
        let Some(mut focus) = self.focus else { return };
        // A focused popup's chords are its window's; a sheet's, its owner's.
        if let Some(i) = self.index(focus) {
            match self.windows[i].kind {
                WindowKind::Popup { parent } => {
                    focus = Key {
                        client: focus.client,
                        id: parent,
                    };
                }
                WindowKind::Sheet {
                    owner_client,
                    owner_id,
                } => {
                    focus = Key {
                        client: owner_client,
                        id: owner_id,
                    };
                }
                _ => {}
            }
        }
        match code_ {
            code::W => self.ask_close(focus),
            code::M => self.set_hidden(focus, true),
            code::LEFT => self.tile(focus, false),
            code::RIGHT => self.tile(focus, true),
            _ => {}
        }
    }

    /// Marks `r` of the screen to be composed again.
    fn dirty(&mut self, r: &Rect) -> Rect {
        if !r.is_empty() {
            self.scene = true;
        }
        self.damage = self.damage.union(r);
        self.damage
    }

    fn move_pointer(&mut self, x: i64, y: i64) {
        let old = self.pointer;
        if old == Some((x, y)) {
            return;
        }
        // The pointer alone is not the scene: over the switcher it is drawn
        // on the last frame.
        if let Some((ox, oy)) = old {
            self.damage = self.damage.union(&self.pointer_rect(ox, oy));
        }
        self.pointer = Some((x, y));
        self.damage = self.damage.union(&self.pointer_rect(x, y));
        if self.carried.is_some() {
            if let Some((ox, oy)) = old {
                self.dirty(&self.carried_rect(ox, oy));
            }
            self.dirty(&self.carried_rect(x, y));
        }
        self.pending_pointer = true;
        self.drag_to(x, y);
        if self.drag.is_none() {
            self.dock_hover(x, y);
        }
    }

    /// Where the dragged entry's label is drawn, beside the pointer.
    fn carried_rect(&self, x: i64, y: i64) -> Rect {
        let s = i64::from(self.scale.factor());
        let label = self
            .carried
            .as_ref()
            .map_or(CARRIED_WIDTH, |c| c.width.max(CARRIED_WIDTH));
        // With the material's halo around the label (16 px to 34 px below
        // the pointer).
        Rect::new(x - 16 * s, y - 24 * s, (label + 64) * s, 96 * s)
    }

    /// `Offer`: the clipboard's new text, from the client just asked to
    /// copy and within 500 ms of it (ADR-0170), and only then.
    fn offer(&mut self, slot: usize, text: &[u8]) -> u32 {
        let now = self.platform.now();
        let asked = self
            .copy_asked
            .is_some_and(|(c, at)| c == slot && now.saturating_sub(at) <= DRAG_WITHIN_NS);
        // Or once, within `GESTURE_OFFER_NS` of a press or key in its
        // focused window (a menu's Copy path, ADR-0248).
        let gestured = self
            .gesture
            .is_some_and(|(c, at)| c == slot && now.saturating_sub(at) <= GESTURE_OFFER_NS);
        let Ok(text) = core::str::from_utf8(text) else {
            return status::BAD_RANGE;
        };
        if !asked && !gestured {
            return status::BAD_RANGE;
        }
        if asked {
            self.copy_asked = None;
        } else {
            self.gesture = None;
        }
        self.clipboard = String::from(text);
        status::OK
    }

    /// `Drag`: the entry is carried while the client's body press is held.
    fn start_drag(&mut self, slot: usize, d: &gui::Drag<'_>, entry: P::End) -> u32 {
        let key = Key {
            client: slot,
            id: d.window,
        };
        // One display name, drawn in the trusted material: no path, no
        // control or bidirectional character; and a connection.
        let name = core::str::from_utf8(d.name.0)
            .ok()
            .filter(|t| !t.is_empty() && !t.contains('/') && rooms::plain_text(t))
            .map(String::from);
        let channel = self.platform.kind(entry) == Kind::Channel;
        // That press, still held, named within `DRAG_NAMES_WITHIN_NS` of it.
        let now = self.platform.now();
        let held = self.body_press.is_some_and(|(k, serial, at)| {
            k == key && serial == d.serial && now.saturating_sub(at) <= DRAG_NAMES_WITHIN_NS
        }) && self.buttons & gui::buttons::LEFT != 0;
        match (held && channel && self.carried.is_none(), name) {
            (true, Some(name)) => {
                // As `draw` lays the label out: the name at the caption's
                // size, 20 px of padding, and a few for rounding at scale 2.
                let cap = zoraya_tokens::text::CAPTION.physical(Scale::ONE);
                let width = i64::from(self.title_text.measure(&name, cap)) + 20 + 4;
                self.carried = Some(Carried {
                    source: key,
                    entry,
                    name,
                    width,
                });
                if let Some((x, y)) = self.pointer {
                    self.dirty(&self.carried_rect(x, y));
                }
                status::OK
            }
            _ => {
                self.platform.close(entry);
                status::BAD_RANGE
            }
        }
    }

    /// The release: the window under the pointer gets the entry and the
    /// pill; over nothing, or over the source, nothing is delivered. The
    /// source hears only that its drag ended.
    fn drop_carried(&mut self, c: Carried<P::End>) {
        if let Some((x, y)) = self.pointer {
            self.dirty(&self.carried_rect(x, y));
        }
        let target = self.pointer.and_then(|(x, y)| {
            let (key, _) = self.under(x, y)?;
            let i = self.index(key)?;
            let w = &self.windows[i];
            // Not into an untrusted room's window: its drops wait for the
            // compositor's confirming card (ADR-0172 point 6, INV-0172-3).
            let untrusted = self
                .clients
                .get(key.client)
                .and_then(Option::as_ref)
                .is_some_and(|c| c.record.untrusted);
            let ok =
                w.framed() && w.owner().is_none() && self.sheet_of(key).is_none() && !untrusted;
            let body = w.body_rect();
            let s = i64::from(self.scale.factor());
            let at = (
                ((x - body.x).max(0) / s) as u32,
                ((y - body.y).max(0) / s) as u32,
            );
            (ok && key.client != c.source.client).then_some((key, at))
        });
        let mut entry = Some(c.entry);
        if let Some((key, (x, y))) = target
            && let Some(cl) = self.clients.get(key.client).and_then(Option::as_ref)
        {
            let m = gui::Drop {
                window: key.id,
                x,
                y,
                name: zoraya_protocol::Data(c.name.as_bytes()),
            };
            let length = reply(&m, NO_REPLY, &mut self.out);
            let end = cl.end;
            let sent = entry.take().into_iter().collect::<Vec<_>>();
            if self.platform.write(end, &self.out[..length], &sent) == Outgoing::Sent {
                // The pill in ADR-0170's words, the compositor's own.
                let words = alloc::format!("dropped: {} read", c.name);
                let _ = self.add_entry(key.client, &words);
            } else {
                entry = sent.into_iter().next();
            }
        }
        if let Some(e) = entry {
            self.platform.close(e);
        }
        // Over the source's own window: where, so that it copies or moves
        // what it holds already (ADR-0248); no `Drop` was sent.
        let own = self.pointer.and_then(|(x, y)| {
            let (key, zone) = self.under(x, y)?;
            let body = self.windows[self.index(key)?].body_rect();
            let s = i64::from(self.scale.factor());
            (key == c.source && zone == Zone::Body && body.contains(x, y))
                .then(|| (((x - body.x) / s) as u32, ((y - body.y) / s) as u32))
        });
        let modifiers = self.modifiers_now();
        self.send_event_plain(
            c.source.client,
            &gui::DragEndedAt {
                window: c.source.id,
                over: Below(u32::from(own.is_some())),
                x: own.map_or(0, |o| o.0),
                y: own.map_or(0, |o| o.1),
                modifiers: Flags(if own.is_some() { modifiers } else { 0 }),
            },
        );
    }

    /// The modifiers held, as key and pointer events carry them, the system
    /// key as `SYSTEM` (ADR-0248).
    fn modifiers_now(&self) -> u32 {
        let system = if self.meta != 0 {
            gui::modifiers::SYSTEM
        } else {
            0
        };
        (self.keymap.modifiers() & gui::modifiers::ALL) | system
    }

    fn pointer_rect(&self, x: i64, y: i64) -> Rect {
        let s = i64::from(self.scale.factor());
        Rect::new(
            x,
            y,
            draw::POINTER[0].len() as i64 * s,
            draw::POINTER.len() as i64 * s,
        )
    }

    fn index(&self, key: Key) -> Option<usize> {
        self.windows
            .iter()
            .position(|w| w.client == key.client && w.id == key.id)
    }

    fn key_of(w: &Window) -> Key {
        Key {
            client: w.client,
            id: w.id,
        }
    }

    /// The topmost visible window holding the point, and the zone.
    fn under(&self, x: i64, y: i64) -> Option<(Key, Zone)> {
        let f = self.frame();
        // The bar is the compositor's: nothing under it is reached there.
        if y < i64::from(space::BAR_HEIGHT.physical(self.scale)) {
            return None;
        }
        // So is the dock, its padding included (INV-0168-3).
        if self.dock.rect.contains(x, y) {
            return None;
        }
        self.windows
            .iter()
            .rev()
            .filter(|w| !w.hidden)
            .find_map(|w| w.zone(&f, self.scale, x, y).map(|z| (Self::key_of(w), z)))
    }

    fn press(&mut self, button: u32) {
        let Some((x, y)) = self.pointer else { return };
        // Another button during a drag raises, focuses and presses nothing.
        if self.carried.is_some() {
            return;
        }
        if self.dock.rect.contains(x, y) {
            if button == gui::buttons::LEFT
                && let Some(cell) = self.dock_cell(x, y)
            {
                self.dock_press(cell);
            }
            return;
        }
        let hit = self.under(x, y);
        // A press anywhere but on a popup closes the popups (ADR-0160).
        let on_popup = hit.is_some_and(|(k, _)| {
            self.index(k)
                .is_some_and(|i| matches!(self.windows[i].kind, WindowKind::Popup { .. }))
        });
        if !on_popup {
            let popups: Vec<Key> = self
                .windows
                .iter()
                .filter(|w| matches!(w.kind, WindowKind::Popup { .. }))
                .map(Self::key_of)
                .collect();
            for p in popups {
                self.send_event_plain(p.client, &gui::CloseRequested { window: p.id });
                self.remove_window(p);
            }
        }
        let Some((key, zone)) = hit else { return };
        self.raise(key);
        self.set_focus(Some(key));
        // A sheet's strip does nothing; a window with a sheet open can be
        // moved by its title bar and nothing else (ADR-0160).
        let is_sheet = self
            .index(key)
            .is_some_and(|i| self.windows[i].owner().is_some());
        if (is_sheet && zone != Zone::Body) || (self.sheet_of(key).is_some() && zone != Zone::Title)
        {
            return;
        }
        if button != gui::buttons::LEFT {
            return;
        }
        let Some(i) = self.index(key) else { return };
        let body = self.windows[i].body_rect();
        match zone {
            Zone::Close => self.ask_close(key),
            Zone::Minimise => self.set_hidden(key, true),
            Zone::Title => {
                self.drag = Some(Drag::Move {
                    window: key,
                    dx: x - body.x,
                    dy: y - body.y,
                });
            }
            Zone::Grip {
                left,
                top,
                right,
                bottom,
            } => {
                self.drag = Some(Drag::Resize {
                    window: key,
                    left,
                    top,
                    right,
                    bottom,
                    start: (x, y),
                    body,
                });
            }
            Zone::Body => {
                let now = self.platform.now();
                if let Some(c) = self.clients.get_mut(key.client).and_then(Option::as_mut) {
                    c.presses = c.presses.wrapping_add(1);
                    self.body_press = Some((key, c.presses, now));
                    // A press in its body, not a key, and never an untrusted
                    // room's client (review 7h, 2): its unasked offer.
                    if !c.record.untrusted {
                        self.gesture = Some((key.client, now));
                    }
                }
            }
        }
    }

    /// The pointer moves while a button holds a drag: the window follows,
    /// and its client sees nothing until the end (ADR-0160).
    fn drag_to(&mut self, x: i64, y: i64) {
        self.drag_window_to(x, y);
        if let Some(Drag::Move { window, .. } | Drag::Resize { window, .. }) = self.drag {
            self.place_sheets(window);
        }
    }

    fn drag_window_to(&mut self, x: i64, y: i64) {
        let f = self.frame();
        let bar = i64::from(space::BAR_HEIGHT.physical(self.scale));
        match self.drag {
            Some(Drag::Move { window, dx, dy }) => {
                let Some(i) = self.index(window) else { return };
                let before = self.windows[i].reach(&f);
                // The title bar stays reachable: below the bar, and a part
                // of it on the screen.
                let w = &mut self.windows[i];
                let min_x = -i64::from(w.body.0) + 2 * f.title;
                let max_x = i64::from(self.width) - 2 * f.title;
                let (ox, oy) = (w.x, w.y);
                w.x = (x - dx).clamp(min_x, max_x.max(min_x));
                w.y = (y - dy).clamp(
                    bar + f.title,
                    i64::from(self.height).max(bar + f.title + 1) - 1,
                );
                let (mx, my) = (w.x - ox, w.y - oy);
                let after = w.reach(&f);
                self.dirty(&before);
                self.dirty(&after);
                // Its popups move with it.
                for j in 0..self.windows.len() {
                    let p = &mut self.windows[j];
                    if p.client == window.client
                        && p.kind == (WindowKind::Popup { parent: window.id })
                    {
                        let before = p.reach(&f);
                        p.x += mx;
                        p.y += my;
                        let after = p.reach(&f);
                        self.dirty(&before);
                        self.dirty(&after);
                    }
                }
            }
            Some(Drag::Resize {
                window,
                left,
                top,
                right,
                bottom,
                start,
                body,
            }) => {
                let Some(i) = self.index(window) else { return };
                let before = self.windows[i].reach(&f);
                let (ddx, ddy) = (x - start.0, y - start.1);
                let s = i64::from(self.scale.factor());
                let mut r = body;
                if right {
                    r.width = (body.width + ddx).max(s);
                }
                if bottom {
                    r.height = (body.height + ddy).max(s);
                }
                if left {
                    let w = (body.width - ddx).max(s);
                    r.x = body.right() - w;
                    r.width = w;
                }
                if top {
                    let h = (body.height - ddy).max(s);
                    r.y = (body.bottom() - h).max(bar + f.title);
                    r.height = body.bottom() - r.y;
                }
                // At most the work area (review 7b, 2): a window cannot be
                // grown past the display by moving it and resizing again.
                let max_w = i64::from(self.width) - 2 * f.border;
                let max_h = i64::from(self.height) - bar - f.title - f.border;
                if r.width > max_w {
                    if left {
                        r.x += r.width - max_w;
                    }
                    r.width = max_w;
                }
                if r.height > max_h {
                    r.height = max_h;
                }
                // Whole logical pixels.
                r.width -= r.width % s;
                r.height -= r.height % s;
                let w = &mut self.windows[i];
                w.x = r.x;
                w.y = r.y;
                w.body = (r.width.max(s) as u32, r.height.max(s) as u32);
                w.chrome = None;
                let after = w.reach(&f);
                self.dirty(&before);
                self.dirty(&after);
            }
            None => {}
        }
    }

    fn release(&mut self) {
        self.body_press = None;
        if let Some(c) = self.carried.take() {
            self.drop_carried(c);
        }
        match self.drag.take() {
            Some(Drag::Resize { window, .. }) => {
                self.configure(window);
                self.fields_dirty = true;
            }
            // The fields follow a move at its end (ADR-0172 point 5(d)).
            Some(Drag::Move { .. }) => self.fields_dirty = true,
            None => {}
        }
    }

    /// After a frame: tell the window under the pointer where it is.
    fn pointer_moved_or_pressed(&mut self) {
        if !core::mem::take(&mut self.pending_pointer) {
            return;
        }
        let Some((x, y)) = self.pointer else { return };
        if self.drag.is_some() || self.carried.is_some() {
            return;
        }
        let Some((key, Zone::Body)) = self.under(x, y) else {
            return;
        };
        if self.sheet_of(key).is_some() {
            return;
        }
        let Some(i) = self.index(key) else { return };
        let w = &self.windows[i];
        let body = w.body_rect();
        // On the border of a legacy surface: not in its body (review 7b, 11).
        if !body.contains(x, y) {
            return;
        }
        let (px, py) = ((x - body.x) as u32, (y - body.y) as u32);
        if w.kind == WindowKind::Legacy {
            let m = gui::Pointer {
                dropped: 0,
                x: px,
                y: py,
                buttons: Flags(self.buttons),
            };
            self.send_event(key.client, m);
        } else {
            let s = self.scale.factor();
            let buttons = self.guarded_buttons(key);
            let m = gui::WindowPointerMods {
                dropped: 0,
                window: key.id,
                x: px / s,
                y: py / s,
                buttons: Flags(buttons),
                serial: self.clients[key.client].as_ref().map_or(0, |c| c.presses),
                // The modifiers only to the focused client: another window
                // under the pointer learns nothing of the keys (review 7h,
                // 10).
                modifiers: Flags(if self.focus.is_some_and(|f| f.client == key.client) {
                    self.modifiers_now()
                } else {
                    0
                }),
            };
            self.send_event(key.client, m);
        }
    }

    // ---- windows ----------------------------------------------------------

    /// Raises a window, with its popups above it.
    fn raise(&mut self, key: Key) {
        let Some(i) = self.index(key) else { return };
        let target = match self.windows[i].kind {
            WindowKind::Popup { parent } => Key {
                client: key.client,
                id: parent,
            },
            WindowKind::Sheet {
                owner_client,
                owner_id,
            } => Key {
                client: owner_client,
                id: owner_id,
            },
            _ => key,
        };
        let Some(t) = self.index(target) else { return };
        let rides = |w: &Window| {
            (w.client == target.client && w.kind == (WindowKind::Popup { parent: target.id }))
                || w.owner() == Some((target.client, target.id))
        };
        // Already on top with its popups and sheet: nothing changes.
        if self.windows[t + 1..].iter().all(rides) {
            return;
        }
        let mut moved: Vec<Window> = Vec::new();
        moved.push(self.windows.remove(t));
        let mut j = 0;
        while j < self.windows.len() {
            if rides(&self.windows[j]) {
                moved.push(self.windows.remove(j));
            } else {
                j += 1;
            }
        }
        let f = self.frame();
        for w in &moved {
            self.dirty(&w.reach(&f));
        }
        self.windows.extend(moved);
    }

    /// Hides or shows a window; a hidden one loses focus to the next.
    fn set_hidden(&mut self, key: Key, hidden: bool) {
        self.fields_dirty = true;
        let Some(i) = self.index(key) else { return };
        if self.windows[i].hidden == hidden || !self.windows[i].framed() {
            return;
        }
        self.windows[i].hidden = hidden;
        if hidden {
            self.close_popups_of(key);
        }
        for w in self.windows.iter_mut() {
            if w.owner() == Some((key.client, key.id)) {
                w.hidden = hidden;
            }
        }
        let f = self.frame();
        self.dirty(&self.windows[i].reach(&f));
        if self.windows[i].kind == WindowKind::Toplevel {
            self.send_event_plain(
                key.client,
                &gui::Minimised {
                    window: key.id,
                    hidden,
                },
            );
        }
        if hidden && self.focus == Some(key) {
            let next = self
                .windows
                .iter()
                .rev()
                .find(|w| !w.hidden && w.framed())
                .map(Self::key_of);
            self.set_focus(next);
        }
    }

    /// Closes the popups of `parent`, telling their client.
    fn close_popups_of(&mut self, parent: Key) {
        let popups: Vec<Key> = self
            .windows
            .iter()
            .filter(|w| {
                w.client == parent.client && w.kind == (WindowKind::Popup { parent: parent.id })
            })
            .map(Self::key_of)
            .collect();
        for p in popups {
            self.send_event_plain(p.client, &gui::CloseRequested { window: p.id });
            self.remove_window(p);
        }
    }

    /// The person asked to close: the client is told. Between five and
    /// twenty seconds later, if the client has committed nothing to the
    /// window since (a hung client), a second press ends it (ADR-0160's
    /// "close anyway"); after that, or once it commits, a press asks again
    /// (review 7b, 4).
    fn ask_close(&mut self, key: Key) {
        let now = self.platform.now();
        let Some(i) = self.index(key) else { return };
        let asked = self.windows[i].close_asked;
        if asked.is_some_and(|at| now.saturating_sub(at) >= 4 * CLOSE_GRACE_NS) {
            self.windows[i].close_asked = None;
        }
        match self.windows[i].close_asked {
            Some(at) if now.saturating_sub(at) >= CLOSE_GRACE_NS => {
                self.drop_client(key.client);
            }
            Some(_) => {}
            None => {
                self.windows[i].close_asked = Some(now);
                let f = self.frame();
                self.dirty(&self.windows[i].title_bar(&f));
                self.send_event_plain(key.client, &gui::CloseRequested { window: key.id });
            }
        }
    }

    /// System+Left and System+Right: the left or right half of the work
    /// area (ADR-0160, Option E's first step).
    fn tile(&mut self, key: Key, right: bool) {
        self.fields_dirty = true;
        let Some(i) = self.index(key) else { return };
        if self.windows[i].kind != WindowKind::Toplevel {
            return;
        }
        let f = self.frame();
        let s = i64::from(self.scale.factor());
        let bar = i64::from(space::BAR_HEIGHT.physical(self.scale));
        let half = i64::from(self.width) / 2;
        let x0 = if right { half } else { 0 };
        let mut body = Rect::new(
            x0 + f.border,
            bar + f.title,
            half - 2 * f.border,
            self.work_area().bottom() - bar - f.title - f.border,
        );
        body.width -= body.width % s;
        body.height -= body.height % s;
        let before = self.windows[i].reach(&f);
        let w = &mut self.windows[i];
        w.x = body.x;
        w.y = body.y;
        w.body = (body.width.max(s) as u32, body.height.max(s) as u32);
        w.chrome = None;
        let after = w.reach(&f);
        self.dirty(&before);
        self.dirty(&after);
        self.configure(key);
        self.place_sheets(key);
    }

    /// After the display changed size or scale (ADR-0246): popups close, and
    /// sheets too at a new scale; every toplevel keeps its logical size at the
    /// new scale, as far as the work area holds it, its frame moved back into
    /// the work area, and is configured again (ADR-0164); legacy surfaces keep
    /// their buffers (review 7b, 13) and are only moved back.
    fn reconfigure(&mut self, old_scale: u32) {
        self.fields_dirty = true;
        // Popups close, as at a press outside them (their place was the old
        // layout's); at a new scale sheets close too: no `Configure` tells
        // them the scale, and their client cancels (review 7g, 4).
        let new_scale = old_scale != self.scale.factor();
        let closing: Vec<Key> = self
            .windows
            .iter()
            .filter(|w| match w.kind {
                WindowKind::Popup { .. } => true,
                WindowKind::Sheet { .. } => new_scale,
                _ => false,
            })
            .map(Self::key_of)
            .collect();
        for p in closing {
            self.send_event_plain(p.client, &gui::CloseRequested { window: p.id });
            self.remove_window(p);
        }
        let k = self.scale.factor();
        let s = i64::from(k);
        let f = self.frame();
        let area = self.work_area();
        let (w, h) = self.largest_body();
        let keys: Vec<Key> = self
            .windows
            .iter()
            .filter(|w| w.kind == WindowKind::Toplevel)
            .map(Self::key_of)
            .collect();
        for win in &mut self.windows {
            win.chrome = None;
            let (bw, bh) = (i64::from(win.body.0), i64::from(win.body.1));
            let into = |x: i64, y: i64, bw: i64, bh: i64| {
                (
                    x.clamp(
                        area.x + f.border,
                        (area.right() - f.border - bw).max(area.x + f.border),
                    ),
                    y.clamp(
                        area.y + f.title,
                        (area.bottom() - f.border - bh).max(area.y + f.title),
                    ),
                )
            };
            match win.kind {
                // Never resized: only moved back into the work area.
                WindowKind::Legacy => {
                    (win.x, win.y) = into(win.x, win.y, bw, bh);
                    continue;
                }
                WindowKind::Toplevel => {}
                // Sheets follow their owners (`place_sheets` below).
                _ => continue,
            }
            let lw = (win.body.0 / old_scale.max(1)).max(1);
            let lh = (win.body.1 / old_scale.max(1)).max(1);
            win.body = ((lw * k).min(w), (lh * k).min(h));
            let x = win.x / i64::from(old_scale.max(1)) * s;
            let y = win.y / i64::from(old_scale.max(1)) * s;
            let (bw, bh) = (i64::from(win.body.0), i64::from(win.body.1));
            win.x = x.clamp(
                area.x + f.border,
                (area.right() - f.border - bw).max(area.x + f.border),
            );
            win.y = y.clamp(
                area.y + f.title,
                (area.bottom() - f.border - bh).max(area.y + f.title),
            );
        }
        for key in keys {
            self.configure(key);
            self.place_sheets(key);
        }
    }

    /// The largest toplevel body whose frame fits the work area, in whole
    /// logical pixels (physical).
    fn largest_body(&self) -> (u32, u32) {
        let s = i64::from(self.scale.factor());
        let f = self.frame();
        let area = self.work_area();
        let most = |v: i64| (v - v % s).max(s) as u32;
        (
            most(area.width - 2 * f.border),
            most(area.height - f.title - f.border),
        )
    }

    /// A new toplevel's body: as asked when the work area holds it; else
    /// made smaller to fit below its cascade place (so the first window's
    /// body stays at `first_body`), or, where too little is left there, the
    /// largest body the work area holds.
    fn fitting_body(&self, bw: u32, bh: u32) -> (u32, u32) {
        let (w, h) = self.largest_body();
        let s = i64::from(self.scale.factor());
        let f = self.frame();
        let area = self.work_area();
        let bar = i64::from(space::BAR_HEIGHT.physical(self.scale));
        let n = self.windows.iter().filter(|w| w.framed()).count() as i64;
        let top = bar + i64::from(ORIGIN.1 + CASCADE * n as u32) * s;
        let left = i64::from(ORIGIN.0 + CASCADE * n as u32) * s;
        let whole = |v: i64| (v - v.rem_euclid(s)).max(0) as u32;
        let below = whole(area.bottom() - top - f.title - f.border);
        let beside = whole(area.right() - left - 2 * f.border);
        // A window that fits below its cascade place keeps its size; one
        // that does not is made smaller to fit there while at least half
        // the work area is left, else it keeps what the work area holds
        // and `place` moves it up.
        let fit = |asked: u32, most: u32, room: u32| -> u32 {
            if asked <= room {
                asked
            } else if room >= most / 2 {
                room.max(s as u32)
            } else {
                asked.min(most)
            }
        };
        (fit(bw, w, beside), fit(bh, h, below))
    }

    /// Sends a toplevel its size, the scale and the appearance.
    fn configure(&mut self, key: Key) {
        let Some(i) = self.index(key) else { return };
        let w = &self.windows[i];
        if w.kind != WindowKind::Toplevel {
            return;
        }
        let s = self.scale.factor();
        let m = gui::Configure {
            window: key.id,
            width: w.body.0 / s,
            height: w.body.1 / s,
            scale: s,
            dark: self.appearance == Appearance::Dark,
            motion: self.motion,
        };
        self.send_event_plain(key.client, &m);
    }

    /// F12: the next window, hidden ones included, raised and focused.
    fn next_window(&mut self) {
        let order = self.switch_order();
        if order.is_empty() {
            return;
        }
        let next = order[1 % order.len()];
        self.activate(next);
    }

    /// The framed windows in the switcher's order: the focused first, then
    /// the stack from the top, hidden ones last.
    fn switch_order(&self) -> Vec<Key> {
        let mut v: Vec<Key> = Vec::new();
        if let Some(f) = self.focus
            && self.index(f).is_some_and(|i| self.windows[i].framed())
        {
            v.push(f);
        }
        for hidden in [false, true] {
            for w in self.windows.iter().rev() {
                let k = Self::key_of(w);
                if w.framed() && w.hidden == hidden && !v.contains(&k) {
                    v.push(k);
                }
            }
        }
        v
    }

    /// Shows, raises and focuses a window.
    fn activate(&mut self, key: Key) {
        self.set_hidden(key, false);
        self.raise(key);
        self.set_focus(Some(key));
    }

    fn open_switcher(&mut self, step: isize) {
        let order = self.switch_order();
        if order.is_empty() {
            return;
        }
        // Grouped by room: the focused window's room first, then `home`,
        // then the table's order; most recent first inside a group.
        let mut groups: Vec<SwitchGroup> = Vec::new();
        let mut keyed: Vec<(usize, usize, Key)> = Vec::new();
        for (n, key) in order.iter().enumerate() {
            let r = self.record(key.client).cloned().unwrap_or_default();
            let name = String::from(r.room_name());
            let g = match groups.iter().position(|g| g.name == name) {
                Some(g) => g,
                None => {
                    let grants = self
                        .rooms
                        .get(&name)
                        .map(|room| {
                            room.grants
                                .iter()
                                .map(|g| g.words())
                                .collect::<Vec<_>>()
                                .join("  ")
                        })
                        .unwrap_or_default();
                    groups.push(SwitchGroup {
                        colour: self.room_colour(&r),
                        untrusted: r.untrusted,
                        name,
                        grants,
                    });
                    groups.len() - 1
                }
            };
            keyed.push((g, n, *key));
        }
        let focused_group = keyed[0].0;
        let rank = |g: usize| {
            let name = groups[g].name.as_str();
            if g == focused_group {
                0
            } else if name == zoraya_settings::rooms::HOME {
                1
            } else {
                2 + self
                    .rooms
                    .rooms
                    .iter()
                    .position(|r| r.name == name)
                    .unwrap_or(zoraya_settings::rooms::MAX_ROOMS)
            }
        };
        keyed.sort_by_key(|(g, n, _)| (rank(*g), *n));
        let windows: Vec<Key> = keyed.iter().map(|k| k.2).collect();
        let group: Vec<usize> = keyed.iter().map(|k| k.0).collect();
        let chosen = (step.rem_euclid(windows.len() as isize)) as usize;
        let k = self.scale.factor();
        let thumbs = windows
            .iter()
            .map(|key| {
                self.index(*key)
                    .map(|i| draw::thumb(&self.windows[i], k))
                    .unwrap_or_default()
            })
            .collect();
        self.switcher = Some(Switcher {
            windows,
            chosen,
            group,
            groups,
            thumbs,
        });
        self.dirty(&self.screen());
    }

    fn step_switcher(&mut self, step: isize) {
        if let Some(s) = self.switcher.as_mut() {
            let n = s.windows.len() as isize;
            s.chosen = (s.chosen as isize + step).rem_euclid(n) as usize;
        }
        self.dirty(&self.screen());
    }

    fn choose_in_switcher(&mut self) {
        let Some(s) = self.switcher.take() else {
            return;
        };
        self.switcher_frame = Vec::new();
        self.dirty(&self.screen());
        if let Some(k) = s.windows.get(s.chosen).copied()
            && self.index(k).is_some()
        {
            self.activate(k);
        }
    }

    fn close_switcher(&mut self) {
        self.switcher = None;
        self.switcher_frame = Vec::new();
        self.dirty(&self.screen());
    }

    /// The bar shows the new layout, and the focused client hears of it.
    fn layout_changed(&mut self) {
        let bar = Rect::new(
            0,
            0,
            i64::from(self.width),
            i64::from(space::BAR_HEIGHT.physical(self.scale)),
        );
        self.dirty(&bar);
        if let Some(f) = self.focus {
            self.send_layout(f.client);
        }
    }

    fn send_layout(&mut self, client: usize) {
        let name = self.keymap.layout().name;
        let m = gui::Layout {
            name: zoraya_protocol::Data(name.as_bytes()),
        };
        self.send_event_plain(client, &m);
    }

    /// The active layout's short name, for the bar.
    pub fn layout_short(&self) -> &'static str {
        self.keymap.layout().short
    }

    /// The client whose press gives it one unasked offer, for a test.
    #[cfg(test)]
    fn gesture_client(&self) -> Option<usize> {
        self.gesture.map(|(c, _)| c)
    }

    fn set_focus(&mut self, key: Option<Key>) {
        // While a sheet is open its owner's focus is the sheet's: the owner
        // gets no input (ADR-0160).
        let key = key.map(|k| self.sheet_of(k).unwrap_or(k));
        if self.focus == key {
            return;
        }
        // A gesture is its window's client's while it keeps the focus
        // (review 7h, 3).
        if self
            .gesture
            .is_some_and(|(c, _)| key.is_none_or(|k| k.client != c))
        {
            self.gesture = None;
        }
        let old = self.focus;
        self.focus = key;
        // A dead key typed into one window does not compose in the next.
        self.keymap.clear_dead();
        if let Some(k) = key
            && old.is_none_or(|o| o.client != k.client)
        {
            self.send_layout(k.client);
        }
        let f = self.frame();
        for (k, focused) in [(old, false), (key, true)] {
            let Some(k) = k else { continue };
            let Some(i) = self.index(k) else { continue };
            self.dirty(&self.windows[i].reach(&f));
            if self.windows[i].kind == WindowKind::Legacy {
                self.send_event(
                    k.client,
                    gui::Focus {
                        dropped: 0,
                        focused,
                    },
                );
            } else {
                self.send_event(
                    k.client,
                    gui::WindowFocus {
                        dropped: 0,
                        window: k.id,
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
        let Some(c) = self.clients.get_mut(client).and_then(Option::as_mut) else {
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

    /// An event with no dropped count of its own: a lost one is counted
    /// for the next that carries one.
    fn send_event_plain<'b, M: Message<'b>>(&mut self, client: usize, m: &M) {
        let Some(c) = self.clients.get_mut(client).and_then(Option::as_mut) else {
            return;
        };
        let length = reply(m, NO_REPLY, &mut self.out);
        if self.platform.write(c.end, &self.out[..length], &[]) == Outgoing::Full {
            c.dropped = c.dropped.saturating_add(1);
        }
    }

    // ---- clients ----------------------------------------------------------

    /// A message on the driver manager (`init`'s, `clients` false) or the
    /// client manager (the panel's, ADR-0168). Each takes its own ordinals
    /// and no other: the drivers only from `init`, the rooms, the dock and
    /// the launch records only from the panel (or `AddClient`, a `home`
    /// client, from either).
    fn manager_message(&mut self, clients: bool) {
        let end = if clients {
            match self.client_manager {
                Some(e) => e,
                None => return,
            }
        } else {
            self.manager
        };
        let mut message = core::mem::take(&mut self.message);
        let mut handles = core::mem::take(&mut self.handles);
        handles.clear();
        let incoming = self.platform.read(end, &mut message, &mut handles);
        let Incoming::Message(n) = incoming else {
            for h in handles.drain(..) {
                self.platform.close(h);
            }
            if clients && matches!(incoming, Incoming::Closed | Incoming::Refused) {
                self.platform.close(end);
                self.client_manager = None;
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
        let none = handles.is_empty();
        let driver = !clients;
        let (s, used) = match header.ordinal {
            32 if one && decode::<gui::AddClient>(bytes, 1).is_ok() => {
                (self.add_client(handles[0], Record::home()), true)
            }
            // From either launcher: init's first terminal is `home`'s
            // (proposed ADR-0242).
            37 if one => match decode::<gui::AddRoomClient<'_>>(bytes, 1) {
                Ok((_, m)) => match Record::of(&m) {
                    Some(r) => (self.add_client(handles[0], r), true),
                    None => (status::INVALID, false),
                },
                Err(_) => (status::INVALID, false),
            },
            39 if clients && none => match decode::<gui::SetRooms<'_>>(bytes, 0) {
                Ok((_, m)) => (self.set_rooms(m.current.0, m.rooms.0), false),
                Err(_) => (status::INVALID, false),
            },
            40 if clients && none => match decode::<gui::SetDock<'_>>(bytes, 0) {
                Ok((_, m)) => (self.set_dock(m.apps.0), false),
                Err(_) => (status::INVALID, false),
            },
            43 if clients && none => match decode::<gui::ApplySettings<'_>>(bytes, 0) {
                Ok((_, m)) => {
                    fn t(d: &[u8]) -> &str {
                        // Not UTF-8: a word no declaration takes.
                        core::str::from_utf8(d).unwrap_or("\u{0}")
                    }
                    let s = settings::checked(
                        t(m.theme.0),
                        t(m.motion.0),
                        t(m.scale.0),
                        t(m.wallpaper.0),
                        t(m.layouts.0),
                        t(m.layout.0),
                    );
                    match s {
                        Some(s) => {
                            self.apply_settings(s);
                            (status::OK, false)
                        }
                        None => (status::INVALID, false),
                    }
                }
                Err(_) => (status::INVALID, false),
            },
            38 if driver && one && decode::<gui::AttachClientManager>(bytes, 1).is_ok() => {
                let h = handles[0];
                let s = if self.platform.kind(h) == Kind::Channel
                    && self.client_manager.is_none()
                    && self.platform.watch(h, CLIENT_MANAGER_KEY)
                {
                    self.client_manager = Some(h);
                    status::OK
                } else {
                    self.platform.close(h);
                    status::UNSUPPORTED
                };
                (s, true)
            }
            42 if driver && one && decode::<gui::AddSystemClient>(bytes, 1).is_ok() => {
                let h = handles[0];
                let s = self.add_client(h, Record::home());
                if s == status::OK
                    && let Some(c) = self.clients.iter_mut().flatten().find(|c| c.end == h)
                {
                    c.system = true;
                }
                (s, true)
            }
            33 if driver && one && decode::<gui::AttachDisplay>(bytes, 1).is_ok() => {
                let h = handles[0];
                let s = if self.platform.kind(h) == Kind::Channel {
                    self.attach_display(h)
                } else {
                    self.platform.close(h);
                    status::UNSUPPORTED
                };
                (s, true)
            }
            34 if driver && one && decode::<gui::AttachInput>(bytes, 1).is_ok() => {
                let h = handles[0];
                let s = if self.platform.kind(h) == Kind::Channel {
                    self.attach_input(h)
                } else {
                    self.platform.close(h);
                    status::UNSUPPORTED
                };
                (s, true)
            }
            35 if none && decode::<gui::Stats>(bytes, 0).is_ok() => {
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
                self.platform.write(end, &self.out[..length], &[]);
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
            self.platform.write(end, &self.out[..length], &[]);
        }
        self.message = message;
        self.handles = handles;
    }

    fn is_system(&self, slot: usize) -> bool {
        self.clients
            .get(slot)
            .and_then(Option::as_ref)
            .is_some_and(|c| c.system)
    }

    /// Whether the guard of a new sheet keeps key `code_` from `focus`:
    /// any key in its first `SHEET_GUARD_NS`, and a key held from before it
    /// mapped until that key's release (ADR-0245).
    fn guards_key(&mut self, focus: Key, code_: u32) -> bool {
        let Some(g) = self.sheet_guards.iter().find(|g| g.sheet == focus).copied() else {
            return false;
        };
        let held = (code_ as usize) < 512 && g.keys[code_ as usize / 64] & (1 << (code_ % 64)) != 0;
        held || self.platform.now() < g.until
    }

    /// The buttons a pointer event to `key` carries: none in a new sheet's
    /// first `SHEET_GUARD_NS`, and never one held from before (ADR-0245).
    fn guarded_buttons(&mut self, key: Key) -> u32 {
        match self.sheet_guards.iter().find(|g| g.sheet == key).copied() {
            Some(g) => {
                if self.platform.now() < g.until {
                    0
                } else {
                    self.buttons & !g.buttons
                }
            }
            _ => self.buttons,
        }
    }

    /// Tells every client the show-commands mode, so that the chooser's
    /// sheet and the file manager's menu show their `zosh` lines (ADR-0245,
    /// ADR-0248).
    fn tell_commands(&mut self) {
        let m = gui::Commands { on: self.commands };
        for slot in 0..self.clients.len() {
            self.send_event_plain(slot, &m);
        }
    }

    /// The sheet open under `owner`, if any.
    fn sheet_of(&self, owner: Key) -> Option<Key> {
        self.windows
            .iter()
            .find(|w| w.owner() == Some((owner.client, owner.id)))
            .map(Self::key_of)
    }

    /// Puts `owner`'s sheet under its title bar, centred on it: its strip
    /// starts on the line between the owner's title bar and body, where no
    /// client's pixels are (ADR-0167's third mark).
    fn place_sheets(&mut self, owner: Key) {
        let f = self.frame();
        let Some(oi) = self.index(owner) else { return };
        let ob = self.windows[oi].body_rect();
        for j in 0..self.windows.len() {
            if self.windows[j].owner() != Some((owner.client, owner.id)) {
                continue;
            }
            let before = self.windows[j].reach(&f);
            let w = &mut self.windows[j];
            w.x = ob.x + (ob.width - i64::from(w.body.0)) / 2;
            w.y = ob.y - f.border + f.title;
            let after = w.reach(&f);
            self.dirty(&before);
            self.dirty(&after);
        }
    }

    /// `CreateSheet` from a system client: a sheet under the focused (or
    /// else the topmost) window of the client the panel tagged `m.tag`,
    /// headed with what it asks and the owner's application from the
    /// compositor's own record (ADR-0167), with the keyboard.
    fn create_sheet(
        &mut self,
        slot: usize,
        m: gui::CreateSheet,
        memory: P::End,
    ) -> Result<u32, u32> {
        let refuse = |me: &mut Self, e: u32| {
            me.platform.close(memory);
            Err(e)
        };
        let owner_slot = (m.tag != 0)
            .then(|| {
                self.clients
                    .iter()
                    .position(|c| c.as_ref().is_some_and(|c| c.record.tag == m.tag))
            })
            .flatten();
        let Some(owner_slot) = owner_slot.filter(|o| *o != slot) else {
            return refuse(self, status::NO_SURFACE);
        };
        let owner = self
            .focus
            .filter(|f| f.client == owner_slot)
            .and_then(|f| self.index(f))
            .filter(|i| self.windows[*i].kind == WindowKind::Toplevel)
            .or_else(|| {
                self.windows
                    .iter()
                    .rposition(|w| w.client == owner_slot && w.kind == WindowKind::Toplevel)
            });
        let Some(oi) = owner else {
            return refuse(self, status::NO_SURFACE);
        };
        let owner = Self::key_of(&self.windows[oi]);
        if self.sheet_of(owner).is_some() {
            return refuse(self, status::EXISTS);
        }
        if self.windows.len() >= MAX_WINDOWS {
            return refuse(self, status::NO_MEMORY);
        }
        let s = self.scale.factor();
        let f = self.frame();
        let (bw, bh) = (m.width.saturating_mul(s), m.height.saturating_mul(s));
        let room = self.height.saturating_sub(f.title as u32);
        if !(1..=self.width).contains(&bw)
            || !(1..=room).contains(&bh)
            || !self.within_budget(slot, u64::from(bw) * u64::from(bh))
        {
            return refuse(self, status::BAD_BUFFER);
        }
        let (buffer, copy) = self.map_buffer(bw, bh, memory, true)?;
        let app = self.clients[owner_slot]
            .as_ref()
            .map(|c| c.record.app.clone())
            .unwrap_or_default();
        let id = self.clients[slot].as_ref().map_or(1, |c| c.next_id);
        if let Some(c) = self.clients[slot].as_mut() {
            c.next_id = c.next_id.wrapping_add(1).max(1);
        }
        // Directly above its owner and the owner's popups.
        let mut at = oi + 1;
        while at < self.windows.len()
            && self.windows[at].client == owner.client
            && self.windows[at].kind == (WindowKind::Popup { parent: owner.id })
        {
            at += 1;
        }
        let hidden = self.windows[oi].hidden;
        self.windows.insert(
            at,
            Window {
                client: slot,
                id,
                kind: WindowKind::Sheet {
                    owner_client: owner.client,
                    owner_id: owner.id,
                },
                x: 0,
                y: 0,
                body: (bw, bh),
                buffer,
                copy,
                next: None,
                title: crate::draw_rooms::sheet_heading(m.kind.0, &app),
                hidden,
                close_asked: None,
                chrome: None,
            },
        );
        self.place_sheets(owner);
        let key = Key { client: slot, id };
        let until = self.platform.now().saturating_add(SHEET_GUARD_NS);
        // One guard per sheet: a second sheet leaves the first's (review of
        // the fix branch, 5); guards of sheets that are gone go.
        let live: Vec<Key> = self.windows.iter().map(Self::key_of).collect();
        self.sheet_guards
            .retain(|g| live.contains(&g.sheet) && g.sheet != key);
        self.sheet_guards.push(SheetGuard {
            sheet: key,
            until,
            keys: self.held,
            buttons: self.buttons,
            shown: false,
        });
        if self
            .focus
            .is_some_and(|f| f == owner || f.client == owner.client)
            || self.focus.is_none()
        {
            self.set_focus(Some(key));
        }
        // The bar's middle shows the heading while the sheet is open.
        self.dirty_bar();
        Ok(id)
    }

    /// `Granted` from a system client: the entry joins the record of the
    /// client tagged `tag`, so that its frame shows it (ADR-0172 point 4).
    fn granted(&mut self, tag: u32, words: &[u8]) -> u32 {
        let Ok(words) = core::str::from_utf8(words) else {
            return status::BAD_RANGE;
        };
        if tag == 0 || words.is_empty() || !rooms::plain_text(words) {
            return status::BAD_RANGE;
        }
        let Some(slot) = self
            .clients
            .iter()
            .position(|c| c.as_ref().is_some_and(|c| c.record.tag == tag))
        else {
            return status::NO_SURFACE;
        };
        self.add_entry(slot, words)
    }

    /// An entry joins the record of client `slot` and its frames show it.
    fn add_entry(&mut self, slot: usize, words: &str) -> u32 {
        let Some(c) = self.clients[slot].as_mut() else {
            return status::NO_SURFACE;
        };
        if !c.record.entries.iter().any(|e| e == words) {
            if c.record.entries.len() >= rooms::MAX_RECORD_ENTRIES {
                return status::NO_MEMORY;
            }
            c.record.entries.push(String::from(words));
        }
        let f = self.frame();
        let mine: Vec<Rect> = self
            .windows
            .iter()
            .filter(|w| w.client == slot && w.framed())
            .map(|w| w.reach(&f))
            .collect();
        for r in mine {
            self.dirty(&r);
        }
        status::OK
    }

    /// Marks the bar to be composed again.
    fn dirty_bar(&mut self) {
        let bar = i64::from(space::BAR_HEIGHT.physical(self.scale));
        let r = Rect::new(0, 0, i64::from(self.width), bar);
        self.dirty(&r);
    }

    fn add_client(&mut self, end: P::End, record: Record) -> u32 {
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
            commit: None,
            dropped: 0,
            next_id: 1,
            record,
            system: false,
            presses: 0,
        });
        // A client that starts while the mode is on hears it.
        if self.commands {
            self.send_event_plain(slot, &gui::Commands { on: true });
        }
        status::OK
    }

    fn unmap(&mut self, b: &Buffer) {
        self.platform.unmap(b.address, b.length);
    }

    /// Removes a window: its buffers go, the screen under it is redrawn,
    /// focus moves to the top window if it had it. A toplevel takes its
    /// popups with it.
    fn remove_window(&mut self, key: Key) {
        self.fields_dirty = true;
        let Some(i) = self.index(key) else { return };
        let w = self.windows.remove(i);
        let f = self.frame();
        self.dirty(&w.reach(&f));
        if w.owner().is_some() {
            self.dirty_bar();
        }
        self.unmap(&w.buffer);
        if let Some(n) = &w.next {
            self.unmap(n);
        }
        if let Some(s) = self.switcher.as_mut() {
            if let Some(at) = s.windows.iter().position(|k| *k == key) {
                s.windows.remove(at);
                s.thumbs.remove(at);
            }
            if s.windows.is_empty() {
                self.switcher = None;
            } else {
                s.chosen = s.chosen.min(s.windows.len() - 1);
            }
        }
        if self.drag.is_some_and(|d| match d {
            Drag::Move { window, .. } | Drag::Resize { window, .. } => window == key,
        }) {
            self.drag = None;
        }
        if w.framed() {
            let popups: Vec<Key> = self
                .windows
                .iter()
                .filter(|p| {
                    p.client == key.client && p.kind == (WindowKind::Popup { parent: key.id })
                })
                .map(Self::key_of)
                .collect();
            for p in popups {
                self.remove_window(p);
            }
            // Its sheet closes: the chooser is told, and cancels.
            if let Some(sheet) = self.sheet_of(key) {
                self.send_event_plain(sheet.client, &gui::CloseRequested { window: sheet.id });
                self.remove_window(sheet);
            }
        }
        if self.focus == Some(key) {
            self.focus = None;
            let parent = match w.kind {
                WindowKind::Popup { parent } => Some(Key {
                    client: key.client,
                    id: parent,
                })
                .filter(|k| self.index(*k).is_some()),
                WindowKind::Sheet {
                    owner_client,
                    owner_id,
                } => Some(Key {
                    client: owner_client,
                    id: owner_id,
                })
                .filter(|k| self.index(*k).is_some()),
                _ => None,
            };
            let top = parent.or_else(|| {
                self.windows
                    .iter()
                    .rev()
                    .find(|w| !w.hidden && w.framed())
                    .map(Self::key_of)
            });
            self.set_focus(top);
        }
    }

    /// The pixels a client's buffers hold, all windows together.
    fn client_pixels(&self, slot: usize) -> u64 {
        self.windows
            .iter()
            .filter(|w| w.client == slot)
            .map(|w| u64::from(w.buffer.width) * u64::from(w.buffer.height))
            .sum()
    }

    /// Whether a client may have `more` pixels of buffers: at most four
    /// displays' worth, so that one client cannot take the compositor's
    /// memory (review 7b, 10).
    fn within_budget(&self, slot: usize, more: u64) -> bool {
        self.client_pixels(slot) + more <= 4 * u64::from(self.width) * u64::from(self.height)
    }

    fn drop_client(&mut self, slot: usize) {
        // A Copy asked of this client is not answered by the next one to
        // take its slot (review 7f, 11).
        if self.gesture.is_some_and(|(c, _)| c == slot) {
            self.gesture = None;
        }
        if self.copy_asked.is_some_and(|(c, _)| c == slot) {
            self.copy_asked = None;
        }
        let keys: Vec<Key> = self
            .windows
            .iter()
            .filter(|w| w.client == slot)
            .map(Self::key_of)
            .collect();
        for k in keys {
            self.remove_window(k);
        }
        if let Some(c) = self.clients[slot].take() {
            self.platform.close(c.end);
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
        let n = handles.len();
        let ready = if self.attached {
            status::OK
        } else {
            status::NO_SURFACE
        };
        match header.ordinal {
            1 if decode::<gui::Display>(bytes, n).is_ok() => {
                let m = gui::DisplayReply {
                    status: Below(ready),
                    width: self.width,
                    height: self.height,
                };
                self.answer(slot, &m, t);
            }
            2 => {
                let s = match decode::<gui::Create>(bytes, n) {
                    Ok((_, m)) => {
                        let memory = handles.remove(0);
                        self.create(slot, WindowKind::Legacy, m.width, m.height, memory)
                            .map_or_else(|e| e, |_| status::OK)
                    }
                    Err(_) => status::BAD_BUFFER,
                };
                self.answer(slot, &gui::Done { status: Below(s) }, t);
            }
            3 => {
                let s = match decode::<gui::Commit>(bytes, n) {
                    Ok((_, m)) => self.commit(slot, 0, (m.x, m.y, m.width, m.height)),
                    Err(_) => status::BAD_RANGE,
                };
                self.commit_reply(slot, s, t, 3);
            }
            4 if decode::<gui::DisplayScale>(bytes, n).is_ok() => {
                let s = self.scale.factor();
                let m = gui::DisplayScaleReply {
                    status: Below(ready),
                    width: self.width / s,
                    height: self.height / s,
                    scale: s,
                };
                self.answer(slot, &m, t);
            }
            5 => {
                let (s, window) = match decode::<gui::CreateToplevel>(bytes, n) {
                    Ok((_, m)) => {
                        let memory = handles.remove(0);
                        match self.create(slot, WindowKind::Toplevel, m.width, m.height, memory) {
                            Ok(id) => (status::OK, id),
                            Err(e) => (e, 0),
                        }
                    }
                    Err(_) => (status::BAD_BUFFER, 0),
                };
                self.answer(
                    slot,
                    &gui::Created {
                        status: Below(s),
                        window,
                    },
                    t,
                );
                if s == status::OK {
                    self.configure(Key {
                        client: slot,
                        id: window,
                    });
                }
            }
            6 => {
                let (s, window) = match decode::<gui::CreatePopup>(bytes, n) {
                    Ok((_, m)) => {
                        let memory = handles.remove(0);
                        match self.create_popup(slot, m, memory) {
                            Ok(id) => (status::OK, id),
                            Err(e) => (e, 0),
                        }
                    }
                    Err(_) => (status::BAD_BUFFER, 0),
                };
                self.answer(
                    slot,
                    &gui::PopupCreated {
                        status: Below(s),
                        window,
                    },
                    t,
                );
            }
            7 => {
                let s = match decode::<gui::CommitTo>(bytes, n) {
                    Ok((_, m)) if m.window != 0 => {
                        self.commit(slot, m.window, (m.x, m.y, m.width, m.height))
                    }
                    Ok(_) => status::NO_SURFACE,
                    Err(_) => status::BAD_RANGE,
                };
                self.commit_reply(slot, s, t, 7);
            }
            8 => {
                let s = match decode::<gui::SetBuffer>(bytes, n) {
                    Ok((_, m)) => {
                        let memory = handles.remove(0);
                        self.set_buffer(slot, m, memory)
                    }
                    Err(_) => status::BAD_BUFFER,
                };
                self.answer(slot, &gui::BufferDone { status: Below(s) }, t);
            }
            9 => {
                let s = match decode::<gui::SetTitle<'_>>(bytes, n) {
                    Ok((_, m)) => self.set_title(slot, m.window, m.text.0),
                    Err(_) => status::BAD_RANGE,
                };
                self.answer(slot, &gui::TitleDone { status: Below(s) }, t);
            }
            10 => {
                let s = match decode::<gui::Close>(bytes, n) {
                    Ok((_, m)) => {
                        let key = Key {
                            client: slot,
                            id: m.window,
                        };
                        if m.window != 0 && self.index(key).is_some() {
                            self.remove_window(key);
                            status::OK
                        } else {
                            status::NO_SURFACE
                        }
                    }
                    Err(_) => status::BAD_RANGE,
                };
                self.answer(slot, &gui::CloseDone { status: Below(s) }, t);
            }
            11 if self.is_system(slot) => {
                let (s, window) = match decode::<gui::CreateSheet>(bytes, n) {
                    Ok((_, m)) => {
                        let memory = handles.remove(0);
                        match self.create_sheet(slot, m, memory) {
                            Ok(id) => (status::OK, id),
                            Err(e) => (e, 0),
                        }
                    }
                    Err(_) => (status::BAD_BUFFER, 0),
                };
                self.answer(
                    slot,
                    &gui::SheetCreated {
                        status: Below(s),
                        window,
                    },
                    t,
                );
            }
            14 => {
                let s = match decode::<gui::Offer<'_>>(bytes, n) {
                    Ok((_, m)) => self.offer(slot, m.text.0),
                    Err(_) => status::BAD_BUFFER,
                };
                self.answer(slot, &gui::OfferDone { status: Below(s) }, t);
            }
            13 => {
                let s = match decode::<gui::Drag<'_>>(bytes, n) {
                    Ok((_, m)) => {
                        let entry = handles.remove(0);
                        self.start_drag(slot, &m, entry)
                    }
                    Err(_) => status::BAD_BUFFER,
                };
                self.answer(slot, &gui::DragDone { status: Below(s) }, t);
            }
            12 if self.is_system(slot) => {
                let s = match decode::<gui::Granted<'_>>(bytes, n) {
                    Ok((_, m)) => self.granted(m.tag, m.words.0),
                    Err(_) => status::BAD_RANGE,
                };
                self.answer(slot, &gui::GrantedDone { status: Below(s) }, t);
            }
            // Anything else, the manager's and the drivers' ordinals among
            // them, and a sheet or a grant from a client the driver manager
            // did not mark (INV-0160-7, INV-0167-4), is unknown here
            // (INV-0093-2).
            _ => self.answer(
                slot,
                &ErrorReply {
                    status: status::UNSUPPORTED,
                },
                t,
            ),
        }
    }

    /// A commit's reply: at the end of the round if it worked (the frame
    /// callback), now if not.
    fn commit_reply(&mut self, slot: usize, s: u32, t: u32, ordinal: u32) {
        if s == status::OK {
            if let Some(c) = self.clients[slot].as_mut() {
                c.commit = Some((t, ordinal));
            }
        } else if ordinal == 3 {
            self.answer(slot, &gui::FrameDone { status: Below(s) }, t);
        } else {
            self.answer(slot, &gui::CommitDone { status: Below(s) }, t);
        }
    }

    /// Maps a client's buffer of `width × height` buffer pixels.
    fn map_buffer(
        &mut self,
        width: u32,
        height: u32,
        memory: P::End,
        with_copy: bool,
    ) -> Result<(Buffer, Vec<u32>), u32> {
        if self.platform.kind(memory) != Kind::Memory {
            self.platform.close(memory);
            return Err(status::BAD_BUFFER);
        }
        let pixels = width as usize * height as usize;
        let length = (pixels as u64 * 4).next_multiple_of(4096);
        // A smaller object fails the mapping (INV-0093-3).
        let address = self.platform.map(memory, length);
        self.platform.close(memory);
        let Some(address) = address else {
            return Err(status::BAD_BUFFER);
        };
        let mut copy = Vec::new();
        if with_copy {
            if copy.try_reserve_exact(pixels).is_err() {
                self.platform.unmap(address, length);
                return Err(status::NO_MEMORY);
            }
            copy.resize(pixels, 0);
        }
        Ok((
            Buffer {
                address,
                length,
                width,
                height,
            },
            copy,
        ))
    }

    /// A legacy surface (`width × height` buffer pixels) or a toplevel
    /// (`width × height` logical), placed by the compositor.
    fn create(
        &mut self,
        slot: usize,
        kind: WindowKind,
        width: u32,
        height: u32,
        memory: P::End,
    ) -> Result<u32, u32> {
        let Some(c) = self.clients[slot].as_ref() else {
            self.platform.close(memory);
            return Err(status::UNSUPPORTED);
        };
        let mine = self.windows.iter().filter(|w| w.client == slot);
        let refuse = match kind {
            WindowKind::Legacy => mine
                .clone()
                .any(|w| w.kind == WindowKind::Legacy)
                .then_some(status::EXISTS),
            _ => (mine.filter(|w| w.kind == WindowKind::Toplevel).count() >= MAX_TOPLEVELS)
                .then_some(status::NO_MEMORY),
        };
        let id = if kind == WindowKind::Legacy {
            0
        } else {
            c.next_id
        };
        if let Some(e) = refuse.or((self.windows.len() >= MAX_WINDOWS).then_some(status::NO_MEMORY))
        {
            self.platform.close(memory);
            return Err(e);
        }
        if !self.attached {
            self.platform.close(memory);
            return Err(status::NO_SURFACE);
        }
        let s = self.scale.factor();
        let (bw, bh) = match kind {
            WindowKind::Legacy => (width, height),
            _ => (width.saturating_mul(s), height.saturating_mul(s)),
        };
        let fits = (1..=self.width).contains(&bw) && (1..=self.height).contains(&bh);
        if !fits {
            self.platform.close(memory);
            return Err(status::BAD_BUFFER);
        }
        if !self.within_budget(slot, u64::from(bw) * u64::from(bh)) {
            self.platform.close(memory);
            return Err(status::NO_MEMORY);
        }
        let (buffer, copy) = self.map_buffer(bw, bh, memory, true)?;
        // A toplevel is never larger than the work area: its body is made
        // to fit, and the `Configure` after `Created` tells the client the
        // size to draw (the 7g check: a terminal of 80 by 24 at scale 2 on
        // 1920 by 1200 ran under the dock). A legacy surface is never
        // resized (ADR-0246).
        let (bw, bh) = if kind == WindowKind::Toplevel {
            self.fitting_body(bw, bh)
        } else {
            (bw, bh)
        };
        let (x, y) = self.place(bw, bh);
        if let Some(c) = self.clients[slot].as_mut()
            && kind != WindowKind::Legacy
        {
            c.next_id = c.next_id.wrapping_add(1).max(1);
        }
        self.windows.push(Window {
            client: slot,
            id,
            kind,
            x,
            y,
            body: (bw, bh),
            buffer,
            copy,
            next: None,
            title: String::new(),
            hidden: false,
            close_asked: None,
            chrome: None,
        });
        self.fields_dirty = true;
        let key = Key { client: slot, id };
        let f = self.frame();
        if let Some(i) = self.index(key) {
            self.dirty(&self.windows[i].reach(&f));
        }
        // A new window takes focus only if none has it (ADR-0094).
        if self.focus.is_none() {
            self.set_focus(Some(key));
        }
        Ok(id)
    }

    fn create_popup(
        &mut self,
        slot: usize,
        m: gui::CreatePopup,
        memory: P::End,
    ) -> Result<u32, u32> {
        let parent = Key {
            client: slot,
            id: m.parent,
        };
        let open = self
            .windows
            .iter()
            .filter(|w| w.client == slot && matches!(w.kind, WindowKind::Popup { .. }))
            .count();
        let Some(pi) = self
            .index(parent)
            .filter(|i| self.windows[*i].kind == WindowKind::Toplevel)
        else {
            self.platform.close(memory);
            return Err(status::NO_SURFACE);
        };
        // A popup is opened by the window that has focus, or by one of its
        // popups (ADR-0160: "a popup it opens while it has focus"); an
        // unfocused client cannot put one over another's window (review
        // 7b, 1).
        let focused = self.focus.is_some_and(|f| {
            f == parent
                || (f.client == slot
                    && self.index(f).is_some_and(|i| {
                        self.windows[i].kind == (WindowKind::Popup { parent: m.parent })
                    }))
        });
        if !focused {
            self.platform.close(memory);
            return Err(status::UNSUPPORTED);
        }
        if open >= gui::MAX_POPUPS || self.windows.len() >= MAX_WINDOWS {
            self.platform.close(memory);
            return Err(status::NO_MEMORY);
        }
        let s = self.scale.factor();
        let bar = space::BAR_HEIGHT.physical(self.scale);
        let (bw, bh) = (m.width.saturating_mul(s), m.height.saturating_mul(s));
        let fits =
            (1..=self.width).contains(&bw) && (1..=self.height.saturating_sub(bar)).contains(&bh);
        if !fits || !self.within_budget(slot, u64::from(bw) * u64::from(bh)) {
            self.platform.close(memory);
            return Err(status::BAD_BUFFER);
        }
        let body = self.windows[pi].body_rect();
        let (buffer, copy) = self.map_buffer(bw, bh, memory, true)?;
        // Relative to the parent's body, drawn clipped to the work area
        // below the bar.
        let x = body.x + i64::from(m.x.min(1 << 20)) * i64::from(s);
        let y = body.y + i64::from(m.y.min(1 << 20)) * i64::from(s);
        let id = self.clients[slot].as_ref().map_or(1, |c| c.next_id);
        if let Some(c) = self.clients[slot].as_mut() {
            c.next_id = c.next_id.wrapping_add(1).max(1);
        }
        // Directly above its parent and the parent's other popups.
        let mut at = pi + 1;
        while at < self.windows.len()
            && self.windows[at].client == slot
            && self.windows[at].kind == (WindowKind::Popup { parent: m.parent })
        {
            at += 1;
        }
        self.windows.insert(
            at,
            Window {
                client: slot,
                id,
                kind: WindowKind::Popup { parent: m.parent },
                x,
                y,
                body: (bw, bh),
                buffer,
                copy,
                next: None,
                title: String::new(),
                hidden: false,
                close_asked: None,
                chrome: None,
            },
        );
        let f = self.frame();
        if let Some(i) = self.index(Key { client: slot, id }) {
            self.dirty(&self.windows[i].reach(&f));
        }
        // It takes keyboard focus while open (ADR-0160).
        self.set_focus(Some(Key { client: slot, id }));
        Ok(id)
    }

    /// A new buffer at the size of the last `Configure`; the old pixels
    /// stay until the first commit to it (INV-0160-5).
    fn set_buffer(&mut self, slot: usize, m: gui::SetBuffer, memory: P::End) -> u32 {
        let key = Key {
            client: slot,
            id: m.window,
        };
        let Some(i) = self
            .index(key)
            .filter(|i| self.windows[*i].kind == WindowKind::Toplevel)
        else {
            self.platform.close(memory);
            return status::NO_SURFACE;
        };
        let s = self.scale.factor();
        let (bw, bh) = (m.width.saturating_mul(s), m.height.saturating_mul(s));
        // Only the size the compositor configured (INV-0160-5, INV-0164-3).
        if (bw, bh) != self.windows[i].body {
            self.platform.close(memory);
            return status::BAD_BUFFER;
        }
        let old =
            u64::from(self.windows[i].buffer.width) * u64::from(self.windows[i].buffer.height);
        let more = (u64::from(bw) * u64::from(bh)).saturating_sub(old);
        if !self.within_budget(slot, more) {
            self.platform.close(memory);
            return status::NO_MEMORY;
        }
        // The copy is made at the first commit.
        match self.map_buffer(bw, bh, memory, false) {
            Ok((buffer, _)) => {
                let w = &mut self.windows[i];
                let old = w.next.replace(buffer);
                if let Some(o) = old {
                    self.platform.unmap(o.address, o.length);
                }
                status::OK
            }
            Err(e) => e,
        }
    }

    fn set_title(&mut self, slot: usize, window: u32, text: &[u8]) -> u32 {
        let key = Key {
            client: slot,
            id: window,
        };
        let Some(i) = self
            .index(key)
            .filter(|i| self.windows[*i].kind == WindowKind::Toplevel)
        else {
            return status::NO_SURFACE;
        };
        let Ok(t) = core::str::from_utf8(text) else {
            return status::BAD_RANGE;
        };
        // Control characters are shown as nothing (INV-0160-6).
        let title: String = t.chars().filter(|c| !c.is_control()).collect();
        let f = self.frame();
        let w = &mut self.windows[i];
        w.title = title;
        let bar = w.title_bar(&f);
        self.dirty(&bar);
        status::OK
    }

    /// The first window's frame at `ORIGIN` below the bar, later ones
    /// cascaded, kept on screen; returns the body's top left.
    fn place(&self, bw: u32, bh: u32) -> (i64, i64) {
        let f = self.frame();
        let s = i64::from(self.scale.factor());
        let bar = i64::from(space::BAR_HEIGHT.physical(self.scale));
        let n = self.windows.iter().filter(|w| w.framed()).count() as i64;
        let ow = i64::from(bw) + 2 * f.border;
        let oh = i64::from(bh) + f.title + f.border;
        // Kept on the screen where it fits, and never above `low`: a window
        // taller than the work area hangs below the screen's edge, its
        // title bar under the bar (review 7b, 5).
        let fit = |start: i64, side: i64, low: i64, screen: i64| -> i64 {
            start.min(screen - side).max(low)
        };
        // Within the work area, above the dock (a toplevel fits it, see
        // `create`).
        let area = self.work_area();
        let x = fit(
            i64::from(ORIGIN.0 + CASCADE * n as u32) * s,
            ow,
            0,
            area.right().min(i64::from(self.width)),
        );
        let y = fit(
            bar + i64::from(ORIGIN.1 + CASCADE * n as u32) * s,
            oh,
            bar,
            area.bottom().min(i64::from(self.height)),
        );
        (x + f.border, y + f.title)
    }

    /// Copies a committed rectangle into the window's own copy, once
    /// (INV-0093-4). The rectangle must be inside the buffer.
    fn commit(&mut self, slot: usize, id: u32, (x, y, width, height): (u32, u32, u32, u32)) -> u32 {
        let Some(i) = self.index(Key { client: slot, id }) else {
            return status::NO_SURFACE;
        };
        // A buffer given since the last commit becomes the shown one now.
        if let Some(next) = self.windows[i].next.take() {
            let pixels = next.width as usize * next.height as usize;
            let mut copy = Vec::new();
            if copy.try_reserve_exact(pixels).is_err() {
                self.platform.unmap(next.address, next.length);
                return status::NO_MEMORY;
            }
            copy.resize(pixels, 0);
            let w = &mut self.windows[i];
            let old = core::mem::replace(&mut w.buffer, next);
            w.copy = copy;
            self.platform.unmap(old.address, old.length);
            // All of it: a first commit of a part shows the rest as the
            // client drew it, not as zeros (review 7b, 7).
            let w = &mut self.windows[i];
            if !self.platform.copy_from(w.buffer.address, &mut w.copy) {
                return status::BAD_BUFFER;
            }
            let all = w.body_rect();
            self.dirty(&all);
        }
        // A sheet's first commit: it can be seen now, and its guard starts
        // again (ADR-0245).
        let now = self.platform.now();
        if let Some(g) = self
            .sheet_guards
            .iter_mut()
            .find(|g| g.sheet == Key { client: slot, id } && !g.shown)
        {
            g.shown = true;
            g.until = now.saturating_add(SHEET_GUARD_NS);
        }
        let w = &mut self.windows[i];
        let Some(r) = Rect::from_client(x, y, width, height, w.buffer.width, w.buffer.height)
        else {
            return status::BAD_RANGE;
        };
        let stride = w.buffer.width as usize;
        for row in r.y..r.bottom() {
            let at = row as usize * stride + r.x as usize;
            let out = &mut w.copy[at..at + r.width as usize];
            let address = w.buffer.address + at as u64 * 4;
            if !self.platform.copy_from(address, out) {
                return status::BAD_BUFFER;
            }
        }
        let screen = r.translate(w.x, w.y).intersect(&w.body_rect());
        // A client that commits is alive: a close press asks it again.
        w.close_asked = None;
        let key = Key { client: slot, id };
        self.dirty(&screen);
        if let Some(sw) = self.switcher.as_mut()
            && let Some(at) = sw.windows.iter().position(|k| *k == key)
        {
            let w = &self.windows[i];
            sw.thumbs[at] = draw::thumb(w, self.scale.factor());
        }
        self.stats.commits += 1;
        status::OK
    }

    // ---- composition ------------------------------------------------------

    /// The first frame after a display's attach, then 100 more: reported
    /// on the console once each (design.md's measurements).
    fn timed(&mut self, took: u64) {
        let s = &mut self.stats;
        if s.first_frame_ns == 0 {
            s.first_frame_ns = took.max(1);
            let line = alloc::format!(
                "compd: {}x{}: wallpaper and blur {} ms, first frame {} ms",
                self.width,
                self.height,
                s.wallpaper_ns / 1_000_000,
                took / 1_000_000
            );
            self.platform.note(&line);
        } else if s.steady_frames < STEADY_FRAMES {
            s.steady_frames += 1;
            s.steady_ns += took;
            s.steady_max_ns = s.steady_max_ns.max(took);
            if s.steady_frames == STEADY_FRAMES {
                let line = alloc::format!(
                    "compd: the next {STEADY_FRAMES} frames: mean {} us, longest {} us",
                    s.steady_ns / STEADY_FRAMES / 1000,
                    s.steady_max_ns / 1000
                );
                self.platform.note(&line);
            }
        }
    }

    fn finish_round(&mut self) {
        // Over the switcher, a change of the scene draws everything again
        // (its glass is a blur of the scene); the pointer alone is drawn on
        // the frame last composed.
        if self.fields_dirty {
            // The dots and icons follow the windows; the fields too.
            self.dock_changed();
            self.update_fields();
        }
        let scene = core::mem::take(&mut self.scene);
        let reuse = self.switcher.is_some() && !scene && !self.switcher_frame.is_empty();
        if self.switcher.is_some() && scene {
            self.damage = self.screen();
        }
        let damage = self.damage.intersect(&self.screen());
        self.damage = Rect::default();
        if !damage.is_empty() {
            let start = self.platform.now();
            if reuse {
                self.restore(damage);
            } else {
                self.compose(damage);
                if self.switcher.is_some() {
                    self.keep_switcher_frame();
                }
            }
            let took = self.platform.now().saturating_sub(start);
            self.stats.compose_ns += took;
            self.stats.frames += 1;
            self.timed(took);
            self.unflushed = self.unflushed.union(&damage);
        }
        // The frame callback: the commit is copied and composed. It does
        // not wait for the display driver (ADR-0093, Decision).
        for slot in 0..MAX_CLIENTS {
            let Some((t, ordinal)) = self.clients[slot].as_mut().and_then(|c| c.commit.take())
            else {
                continue;
            };
            let ok = Below(status::OK);
            if ordinal == 3 {
                self.answer(slot, &gui::FrameDone { status: ok }, t);
            } else {
                self.answer(slot, &gui::CommitDone { status: ok }, t);
            }
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
}

/// Events carry the count dropped before them.
pub trait WithDropped {
    fn set_dropped(&mut self, dropped: u32);
}

macro_rules! with_dropped {
    ($($t:ty),*) => {
        $(impl WithDropped for $t {
            fn set_dropped(&mut self, dropped: u32) {
                self.dropped = dropped;
            }
        })*
    };
}

with_dropped!(
    gui::Key,
    gui::Pointer,
    gui::Focus,
    gui::WindowFocus,
    gui::WindowPointerMods
);

#[cfg(test)]
extern crate std;

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_fields;
#[cfg(test)]
extern crate zoraya_shell;

// A runaway test fails at 512 MiB instead of taking the host (docs/backlog.md).
#[cfg(test)]
zoraya_test_alloc::cap_memory!();
