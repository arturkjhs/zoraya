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
pub type Status = Below<9>;

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
    /// The client's account is at its bound (ADR-0262 point 2, ADR-0263):
    /// an untrusted room's surfaces or pixels. Nothing of any other account
    /// changes.
    pub const OVER_QUOTA: u32 = 8;
}

/// Modifier bits of a key event.
pub mod modifiers {
    pub const SHIFT: u32 = 1;
    pub const CTRL: u32 = 2;
    pub const ALT: u32 = 4;
    pub const CAPS: u32 = 8;
    /// The system key, held in a chord the compositor does not take
    /// (ADR-0248); a key event with it is never one of the
    /// compositor's chords.
    pub const SYSTEM: u32 = 16;
    pub const ALL: u32 = 31;
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
    /// System client only (the chooser, marked by `AddSystemClient`): a
    /// sheet of `width × height` logical pixels under the focused window of
    /// the client the panel tagged `tag` (ADR-0160, ADR-0167). `kind` is
    /// `zoraya-proto-chooser`'s `sheet`, for the heading the compositor
    /// draws above the sheet and in the bar. The reply is `SheetCreated`.
    message CreateSheet = 11, handles 1 {
        tag: u32,
        kind: Below<3>,
        width: u32,
        height: u32,
    }
    message SheetCreated = 11, handles 0 {
        status: Status,
        window: u32,
    }

    /// System client only: the person granted the client tagged `tag` an
    /// entry, in ADR-0153's words, for its frame's pills (ADR-0172 point
    /// 4). The reply is `GrantedDone`.
    message Granted<'a> = 12, handles 0 {
        tag: u32,
        words: Data<'a, MAX_LINE>,
    }
    message GrantedDone = 12, handles 0 {
        status: Status,
    }

    /// The client starts a drag of one entry (ADR-0170) from `window`,
    /// whose body holds the left button the compositor delivered to it,
    /// naming that press's `serial` within 500 ms of it: the entry's
    /// connection and its display name (one name, no `/`), which the
    /// compositor shows beside the pointer and in the target's pill as
    /// `dropped: NAME read`; the client chooses no other word of it.
    /// Refused (`BAD_RANGE`) when no press of the client's is held. The
    /// reply is `DragDone`; at the release the compositor sends the source
    /// `DragEnded` and the window under the pointer `Drop`.
    message Drag<'a> = 13, handles 1 {
        window: u32,
        serial: u32,
        name: Data<'a, MAX_DROPPED_NAME>,
    }
    message DragDone = 13, handles 0 {
        status: Status,
    }

    /// The client's answer to `Copy` (ADR-0170): what it offers, text,
    /// within 500 ms of the `Copy` the compositor sent it; at any other
    /// time refused (`BAD_RANGE`). The compositor keeps it as the
    /// clipboard; no message reads the clipboard back. The reply is
    /// `OfferDone`.
    message Offer<'a> = 14, handles 0 {
        text: Data<'a, MAX_CLIPBOARD>,
    }
    message OfferDone = 14, handles 0 {
        status: Status,
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

    /// The pointer over a window's body, in logical pixels. Superseded by
    /// `WindowPointerMods` (the compositor no longer sends it, ADR-0052).
    message WindowPointer = 23, handles 0 {
        dropped: u32,
        window: u32,
        x: u32,
        y: u32,
        buttons: Flags<{ buttons::ALL }>,
        /// How many left presses on its bodies the compositor delivered to
        /// this client, counted for it alone: what a `Drag` names
        /// (ADR-0170).
        serial: u32,
    }

    /// `WindowPointer` with the modifiers held (Shift, Ctrl, Alt, the
    /// system key), for a Shift or Command click, to the focused client
    /// only (ADR-0248). A new ordinal, not a changed layout (ADR-0052).
    message WindowPointerMods = 30, handles 0 {
        dropped: u32,
        window: u32,
        x: u32,
        y: u32,
        buttons: Flags<{ buttons::ALL }>,
        serial: u32,
        modifiers: Flags<{ modifiers::ALL }>,
    }

    /// The active keyboard layout's name (ADR-0163: `us`, `ua`, `ru`,
    /// `cz`), to the focused client when it gains focus and at every switch.
    message Layout<'a> = 24, handles 0 {
        name: Data<'a, 16>,
    }

    /// An entry dropped on `window` at `(x, y)` in its body (ADR-0170): the
    /// connection, read only, and its display name, never a path
    /// (ADR-0167). The window saw nothing of the drag before.
    message Drop<'a> = 25, handles 1 {
        window: u32,
        x: u32,
        y: u32,
        name: Data<'a, MAX_DROPPED_NAME>,
    }

    /// The source's drag ended (a drop or none: the source is not told
    /// which, ADR-0170).
    message DragEnded = 26, handles 0 {
        window: u32,
    }

    /// `DragEnded` for a drop over the source's own window, with its place
    /// in that window's body and the modifiers held (`over` 1), or `over`
    /// 0 for any other end (ADR-0248): the source holds what was dragged
    /// already. A new ordinal, not a changed layout (ADR-0052).
    message DragEndedAt = 31, handles 0 {
        window: u32,
        over: Below<2>,
        x: u32,
        y: u32,
        modifiers: Flags<{ modifiers::ALL }>,
    }

    /// The person pressed System+C (`cut` false) or System+X in `window`
    /// (ADR-0170): answer with `Offer`, and on a cut remove what was
    /// offered.
    message Copy = 27, handles 0 {
        window: u32,
        cut: bool,
    }

    /// The person pressed System+V in `window`: the clipboard's text, the
    /// only way any client learns it (ADR-0170).
    message Paste<'a> = 28, handles 0 {
        window: u32,
        text: Data<'a, MAX_CLIPBOARD>,
    }

    /// To every client: the show-commands mode (System+?, ADR-0172 point 8)
    /// is on or off, so that the chooser's sheet (ADR-0245) and the file
    /// manager's menu (ADR-0248) show their `zosh` lines.
    message Commands = 29, handles 0 {
        on: bool,
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

    /// Client manager only (the panel, ADR-0169): the desktop's settings
    /// after the settings window changed them, in the words of their
    /// declarations in the compositor's description (`dark`, `yes`,
    /// `auto`, `''`, `us ua`, `ua`). The compositor checks each again and
    /// applies them all, or none (`INVALID`). The reply is `ManagerDone`.
    message ApplySettings<'a> = 43, handles 0 {
        theme: Data<'a, 16>,
        motion: Data<'a, 16>,
        scale: Data<'a, 16>,
        wallpaper: Data<'a, MAX_LINE>,
        layouts: Data<'a, 64>,
        layout: Data<'a, 16>,
    }

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
        /// The panel's tag for this client, which it also gives the
        /// chooser with the application's chooser channel, so that a sheet
        /// finds its owner (ADR-0160, proposed ADR-0245); 0 for none.
        tag: u32,
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

    /// Driver manager: a client whose windows are system windows and who
    /// may create sheets and report grants (the chooser, ADR-0167). The
    /// reply is `ManagerDone`.
    message AddSystemClient = 42, handles 1 {}

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

    /// Client manager (the panel, ADR-0267 point 3): the usage of the rooms
    /// that run, one reading a line in `zoraya_settings::usage`'s text, once
    /// a second at most. The compositor checks every line again and takes
    /// all or none (`INVALID`); it draws the pills at 90% and the hover card
    /// from them, and from nothing a client sends (INV-0172-1). The reply is
    /// `ManagerDone`.
    message SetUsage<'a> = 44, handles 0 {
        readings: Data<'a, MAX_USAGE_TEXT>,
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
pub const CLIENT_REQUESTS: [u32; 14] = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14];
/// The client requests only a system client may send (proposed ADR-0245).
pub const SYSTEM_REQUESTS: [u32; 2] = [11, 12];
/// The ordinals the compositor sends a client unasked.
pub const CLIENT_EVENTS: [u32; 16] = [
    16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31,
];
/// The ordinals only the manager may send.
pub const MANAGER_REQUESTS: [u32; 6] = [32, 33, 34, 35, 38, 42];
/// The ordinals only a manager may send besides `MANAGER_REQUESTS`:
/// `AddRoomClient` from either, the rooms and the dock from the client
/// manager only (ADR-0168).
pub const CLIENT_MANAGER_REQUESTS: [u32; 5] = [37, 39, 40, 43, 44];
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
/// The most bytes the clipboard holds (ADR-0170, an assumption).
pub const MAX_CLIPBOARD: usize = 32 * 1024;
/// The longest display name a drag carries.
pub const MAX_DROPPED_NAME: usize = 64;
pub const MAX_ROOMS_TEXT: usize = 3072;
pub const MAX_DOCK_TEXT: usize = 1024;
/// The longest `SetUsage` text: 33 readings of about 110 bytes.
pub const MAX_USAGE_TEXT: usize = 4096;

#[cfg(test)]
extern crate std;

#[cfg(test)]
mod tests;

// A runaway test fails at 512 MiB instead of taking the host (docs/backlog.md).
#[cfg(test)]
zoraya_test_alloc::cap_memory!();
