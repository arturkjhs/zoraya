//! The compositor's protocols (ADR-0093, ADR-0094).
//!
//! **Client.** A client asks the display's size, creates its one surface with
//! a memory object, and commits damage; each commit's reply is its frame
//! callback. The compositor sends it events with transaction 0. No message
//! carries pixels to a client, names another client, or makes input
//! (INV-0093-2).
//!
//! **Windows** (ADR-0160, ADR-0164; stage 7b). A client learns the logical
//! display size and the scale (`DisplayScale`), creates any number of
//! toplevel windows (`CreateToplevel`, a memory object of the logical size
//! times the scale) and popups, commits damage to each (`CommitTo`), and
//! gets a new buffer to a window after a `Configure` (`SetBuffer`). The
//! compositor frames every toplevel; nothing a client sends sets a window's
//! position, stacking, decoration or focus (INV-0160-2). `Create` and
//! `Commit` stay for one surface per connection until every client moves.
//!
//! **Manager.** Only `init` holds the manager channel: it adds clients and
//! attaches drivers.

#![no_std]
#![forbid(unsafe_code)]

use zoraya_protocol::{Below, Data, Flags, protocol};

/// One of `status`.
pub type Status = Below<8>;

pub mod status {
    pub const OK: u32 = 0;
    /// A damage rectangle outside the surface, or empty.
    pub const BAD_RANGE: u32 = 1;
    /// Not a memory object, smaller than the surface, or a size over the
    /// display's.
    pub const BAD_BUFFER: u32 = 2;
    /// The compositor could not allocate its copy.
    pub const NO_MEMORY: u32 = 3;
    /// `Commit` before `Create`.
    pub const NO_SURFACE: u32 = 4;
    /// A second `Create`, or a driver attached twice.
    pub const EXISTS: u32 = 5;
    /// The compositor does not know the ordinal here.
    pub const UNSUPPORTED: u32 = 6;
    /// A manager's record, room table or dock list that does not parse.
    pub const INVALID: u32 = 7;
}

/// Modifier bits of a key event.
pub mod modifiers {
    pub const SHIFT: u32 = 1;
    pub const CTRL: u32 = 2;
    pub const ALT: u32 = 4;
    pub const CAPS: u32 = 8;
    pub const ALL: u32 = 15;
}

/// Pointer buttons.
pub mod buttons {
    pub const LEFT: u32 = 1;
    pub const RIGHT: u32 = 2;
    pub const MIDDLE: u32 = 4;
    pub const ALL: u32 = 7;
}

/// The longest window title, in bytes of UTF-8 (ADR-0160).
pub const MAX_TITLE: usize = 256;

/// Popups a client may have open at once (ADR-0160).
pub const MAX_POPUPS: usize = 4;

/// One past the last Unicode scalar value: `text` is below it, and 0 is no
/// text. A receiver still converts it with `char::from_u32`.
pub const TEXT_LIMIT: u32 = 0x11_0000;

protocol! {
    /// The display's size.
    message Display = 1, handles 0 {}
    message DisplayReply = 1, handles 0 {
        status: Status,
        width: u32,
        height: u32,
    }

    /// The client's surface: its memory object holds `width × height`
    /// pixels, `0x00RRGGBB` little-endian, row by row.
    message Create = 2, handles 1 {
        width: u32,
        height: u32,
    }
    message Done = 2, handles 0 {
        status: Status,
    }

    /// Shows a rectangle of the surface. The reply, `FrameDone`, comes after
    /// the compositor has copied it: the frame callback.
    message Commit = 3, handles 0 {
        x: u32,
        y: u32,
        width: u32,
        height: u32,
    }
    message FrameDone = 3, handles 0 {
        status: Status,
    }

    /// A key while this client has focus. `text` is the character it
    /// produces under the layout, or 0.
    message Key = 16, handles 0 {
        dropped: u32,
        code: u32,
        pressed: bool,
        modifiers: Flags<{ modifiers::ALL }>,
        text: Below<TEXT_LIMIT>,
    }

    /// The pointer over this client's surface, in its coordinates.
    message Pointer = 17, handles 0 {
        dropped: u32,
        x: u32,
        y: u32,
        buttons: Flags<{ buttons::ALL }>,
    }

    /// This client gained or lost keyboard focus.
    message Focus = 18, handles 0 {
        dropped: u32,
        focused: bool,
    }

    /// The display's logical size and its scale (ADR-0164): a buffer is the
    /// logical size times the scale in each direction.
    message DisplayScale = 4, handles 0 {}
    message DisplayScaleReply = 4, handles 0 {
        status: Status,
        width: u32,
        height: u32,
        scale: u32,
    }

    /// A toplevel window of `width × height` logical pixels; its memory
    /// object holds that times the scale, `0x00RRGGBB` row by row. The
    /// compositor places, frames and stacks it; `Configure` follows.
    message CreateToplevel = 5, handles 1 {
        width: u32,
        height: u32,
    }
    message Created = 5, handles 0 {
        status: Status,
        window: u32,
    }

    /// A popup (a menu, a completion list): no frame, at `(x, y)` logical
    /// pixels from the top left of the parent's body, clipped to the
    /// display, closed when the person presses outside it.
    message CreatePopup = 6, handles 1 {
        parent: u32,
        x: u32,
        y: u32,
        width: u32,
        height: u32,
    }
    message PopupCreated = 6, handles 0 {
        status: Status,
        window: u32,
    }

    /// Shows a rectangle (buffer pixels) of a window; the reply is the
    /// frame callback.
    message CommitTo = 7, handles 0 {
        window: u32,
        x: u32,
        y: u32,
        width: u32,
        height: u32,
    }
    message CommitDone = 7, handles 0 {
        status: Status,
    }

    /// A new buffer for a window at the logical size of its last
    /// `Configure`; the old pixels stay until the first commit to it.
    message SetBuffer = 8, handles 1 {
        window: u32,
        width: u32,
        height: u32,
    }
    message BufferDone = 8, handles 0 {
        status: Status,
    }

    /// The window's title, UTF-8, drawn by the compositor in the frame.
    message SetTitle<'a> = 9, handles 0 {
        window: u32,
        text: Data<'a, MAX_TITLE>,
    }
    message TitleDone = 9, handles 0 {
        status: Status,
    }

    /// The client closes its own window.
    message Close = 10, handles 0 {
        window: u32,
    }
    message CloseDone = 10, handles 0 {
        status: Status,
    }

    /// How a window is to be drawn: its logical size, the scale and the
    /// appearance (ADR-0160; ADR-0164 and ADR-0169 add no field). Sent at
    /// creation and whenever any of it changes.
    message Configure = 19, handles 0 {
        window: u32,
        width: u32,
        height: u32,
        scale: u32,
        dark: bool,
        motion: bool,
    }

    /// The person asked to close the window (its button or System+W).
    message CloseRequested = 20, handles 0 {
        window: u32,
    }

    /// The window was hidden or shown again.
    message Minimised = 21, handles 0 {
        window: u32,
        hidden: bool,
    }

    /// A window of this client gained or lost keyboard focus; keys go to
    /// the client's focused window.
    message WindowFocus = 22, handles 0 {
        dropped: u32,
        window: u32,
        focused: bool,
    }

    /// The pointer over a window's body, in logical pixels.
    message WindowPointer = 23, handles 0 {
        dropped: u32,
        window: u32,
        x: u32,
        y: u32,
        buttons: Flags<{ buttons::ALL }>,
    }

    /// The active keyboard layout's name (ADR-0163: `us`, `ua`, `ru`,
    /// `cz`), to the focused client when it gains focus and at every switch.
    message Layout<'a> = 24, handles 0 {
        name: Data<'a, 16>,
    }

    /// Manager: a new client, with the compositor's end of its channel.
    message AddClient = 32, handles 1 {}
    message ManagerDone = 32, handles 0 {
        status: Status,
    }

    /// Manager: a display driver's channel. The reply is `ManagerDone`.
    message AttachDisplay = 33, handles 1 {}

    /// Manager: an input driver's channel. The reply is `ManagerDone`.
    message AttachInput = 34, handles 1 {}

    /// Manager: what composition has cost so far.
    message Stats = 35, handles 0 {}
    message StatsReply = 35, handles 0 {
        frames: u64,
        commits: u64,
        compose_ns: u64,
        flush_ns: u64,
        /// Flushes a display driver answered.
        flushes: u64,
        /// Display drivers dropped for missing their deadline.
        hangs: u64,
    }

    /// Compositor to manager, unasked: the display driver missed its
    /// deadline and was dropped (ADR-0093, Decision).
    message DisplayHung = 36, handles 0 {}

    /// Manager (ADR-0168, ADR-0172 point 4): a new client launched in a
    /// room, with the compositor's end of its channel and its launch
    /// record, which the compositor draws and no client message changes
    /// (INV-0172-1). From the panel, and from `init` for its first
    /// terminal (proposed ADR-0242). The reply is `ManagerDone`.
    message AddRoomClient<'a> = 37, handles 1 {
        /// 1 for an untrusted room.
        untrusted: u32,
        /// The room's colour slot, 1 to 4; 0 for `home` and untrusted.
        colour: u32,
        /// The room's name; empty for `home`.
        room: Data<'a, 24>,
        /// The application, as the dock names it (`term`, `edit`).
        app: Data<'a, 16>,
        /// The record's entries in ADR-0153's words, one a line.
        entries: Data<'a, MAX_ENTRIES>,
        /// The `zosh` line that made the record (`room project term`).
        line: Data<'a, MAX_LINE>,
    }

    /// Driver manager: the client manager's channel, for the panel
    /// (ADR-0168). The reply is `ManagerDone`.
    message AttachClientManager = 38, handles 1 {}

    /// Client manager: the room table as `/config/rooms` writes it, and
    /// the current room (empty for `home`), for the dock's chips and the
    /// fields (ADR-0172). The reply is `ManagerDone`.
    message SetRooms<'a> = 39, handles 0 {
        current: Data<'a, 24>,
        rooms: Data<'a, MAX_ROOMS_TEXT>,
    }

    /// Client manager: the dock's applications, one a line, `NAME` a tab
    /// and the line that launches it (`edit\tedit FILE`). The reply is
    /// `ManagerDone`.
    message SetDock<'a> = 40, handles 0 {
        apps: Data<'a, MAX_DOCK_TEXT>,
    }

    /// Compositor to the client manager, unasked: the person pressed the
    /// dock (ADR-0168). `kind` is one of [`dock`]; `index` the application
    /// in `SetDock`'s order or the room (0 `home`, then the table's order).
    message DockPressed = 41, handles 0 {
        kind: u32,
        index: u32,
    }
}

/// The ordinals a client may send.
pub const CLIENT_REQUESTS: [u32; 10] = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10];
/// The ordinals the compositor sends a client unasked.
pub const CLIENT_EVENTS: [u32; 9] = [16, 17, 18, 19, 20, 21, 22, 23, 24];
/// The ordinals only the manager may send.
pub const MANAGER_REQUESTS: [u32; 5] = [32, 33, 34, 35, 38];
/// The ordinals only a manager may send besides `MANAGER_REQUESTS`:
/// `AddRoomClient` from either, the rooms and the dock from the client
/// manager only (ADR-0168).
pub const CLIENT_MANAGER_REQUESTS: [u32; 3] = [37, 39, 40];
/// What the compositor sends the manager unasked.
pub const MANAGER_EVENTS: [u32; 2] = [36, 41];

/// `DockPressed`'s kinds.
pub mod dock {
    /// An application's icon with no window, or System+Enter's terminal.
    pub const LAUNCH: u32 = 0;
    /// A room's chip: enter the room.
    pub const ROOM: u32 = 1;
    /// The `+` chip: a new room.
    pub const NEW_ROOM: u32 = 2;
}

/// The longest launch record's entries, room table and dock list.
pub const MAX_ENTRIES: usize = 1024;
pub const MAX_LINE: usize = 256;
pub const MAX_ROOMS_TEXT: usize = 3072;
pub const MAX_DOCK_TEXT: usize = 1024;

#[cfg(test)]
extern crate std;

#[cfg(test)]
mod tests;

// A runaway test fails at 512 MiB instead of taking the host (docs/backlog.md).
#[cfg(test)]
zoraya_test_alloc::cap_memory!();
