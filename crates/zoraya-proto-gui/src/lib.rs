//! The compositor's protocols (ADR-0093, ADR-0094).
//!
//! **Client.** A client asks the display's size, creates its one surface with
//! a memory object, and commits damage; each commit's reply is its frame
//! callback. The compositor sends it events with transaction 0. No message
//! carries pixels to a client, names another client, or makes input
//! (INV-0093-2).
//!
//! **Manager.** Only `init` holds the manager channel: it adds clients and
//! attaches drivers.

#![no_std]
#![forbid(unsafe_code)]

use zoraya_protocol::{Below, Flags, protocol};

/// One of `status`.
pub type Status = Below<7>;

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
}

/// The ordinals a client may send.
pub const CLIENT_REQUESTS: [u32; 3] = [1, 2, 3];
/// The ordinals the compositor sends a client unasked.
pub const CLIENT_EVENTS: [u32; 3] = [16, 17, 18];
/// The ordinals only the manager may send.
pub const MANAGER_REQUESTS: [u32; 4] = [32, 33, 34, 35];
/// What the compositor sends the manager unasked.
pub const MANAGER_EVENTS: [u32; 1] = [36];

#[cfg(test)]
extern crate std;

#[cfg(test)]
mod tests;
