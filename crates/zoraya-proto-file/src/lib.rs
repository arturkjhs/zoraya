//! The file protocol between clients and a filesystem service (ADR-0055). A
//! connection is a channel, and each open object has one. A write under
//! `MAX_INLINE` travels in the message; above it, an attached memory object.

#![no_std]
#![forbid(unsafe_code)]

use zoraya_protocol::{Below, Data, Flags, Name, Path, Seq, Text, protocol};

/// Largest read or write in a message (ADR-0055).
pub const MAX_INLINE: usize = 60 * 1024;
/// Longest path in one request.
pub const MAX_PATH: usize = 4096;
/// Names in one `read_directory` reply.
pub const MAX_ENTRIES: usize = 64;

/// One of `status`.
pub type Code = Below<16>;
/// One of `kind`.
pub type Kind = Below<3>;

/// What an object is.
pub mod kind {
    pub const FILE: u32 = 0;
    pub const DIRECTORY: u32 = 1;
    pub const LINK: u32 = 2;
}

/// The status of every reply.
pub mod status {
    pub const OK: u32 = 0;
    pub const NOT_FOUND: u32 = 1;
    pub const EXISTS: u32 = 2;
    pub const NO_SPACE: u32 = 3;
    pub const NOT_EMPTY: u32 = 4;
    pub const INVALID_NAME: u32 = 5;
    pub const NOT_DIRECTORY: u32 = 6;
    pub const IS_DIRECTORY: u32 = 7;
    /// `OpenReply` then holds the rest of the path and the link's target.
    pub const IS_LINK: u32 = 8;
    pub const INVALID_ARGUMENT: u32 = 9;
    pub const CORRUPT: u32 = 10;
    pub const STORAGE_FAILED: u32 = 11;
    pub const ACCESS_DENIED: u32 = 12;
    /// The service's table is full (ADR-0055).
    pub const TOO_MANY_OPEN: u32 = 13;
    /// The service's mappings are all in use.
    pub const LIMIT_REACHED: u32 = 14;
    /// The service does not know the ordinal.
    pub const UNSUPPORTED: u32 = 15;
}

/// A connection's rights (ADR-0055).
pub mod rights {
    pub const READ: u32 = 1 << 0;
    pub const WRITE: u32 = 1 << 1;
    pub const CREATE: u32 = 1 << 2;
    pub const REMOVE: u32 = 1 << 3;
    pub const RENAME: u32 = 1 << 4;
    pub const PUBLISH: u32 = 1 << 5;
    pub const SYNC: u32 = 1 << 6;
    pub const CLONE: u32 = 1 << 7;
    pub const ALL: u32 = (1 << 8) - 1;
}

/// `open`'s flags.
pub mod open_flags {
    pub const CREATE: u32 = 1 << 0;
    pub const EXCLUSIVE: u32 = 1 << 1;
    pub const TRUNCATE: u32 = 1 << 2;
    pub const DIRECTORY: u32 = 1 << 3;
    /// The link itself, not `IS_LINK`.
    pub const NO_FOLLOW: u32 = 1 << 4;
    /// A new file with no name, for `publish` (ADR-0010 rule 7).
    pub const UNNAMED: u32 = 1 << 5;
    pub const ALL: u32 = (1 << 6) - 1;
}

/// `write`'s flags.
pub mod write_flags {
    /// To the end of the file, atomically (ADR-0010 rule 2).
    pub const APPEND: u32 = 1 << 0;
    /// The reply waits for the commit that covers it.
    pub const DURABLE: u32 = 1 << 1;
    pub const ALL: u32 = (1 << 2) - 1;
}

/// `rename`'s mode (ADR-0010 rule 2).
pub mod rename_mode {
    pub const REPLACE: u32 = 0;
    pub const NO_REPLACE: u32 = 1;
    pub const EXCHANGE: u32 = 2;
    pub const COUNT: u32 = 3;
}

/// `publish`'s mode (ADR-0010 rule 7).
pub mod publish_mode {
    pub const REPLACE: u32 = 0;
    pub const NO_REPLACE: u32 = 1;
    pub const COUNT: u32 = 2;
}

/// `seek`'s origin (ADR-0103 item 2): POSIX's `SEEK_SET`, `SEEK_CUR` and
/// `SEEK_END`.
pub mod whence {
    pub const START: u32 = 0;
    pub const CURRENT: u32 = 1;
    pub const END: u32 = 2;
    pub const COUNT: u32 = 3;
}

/// What an open description holds besides its position (ADR-0103 item 1):
/// what `open_with_mode` starts with, `set_flags` sets and `get_flags` gives.
pub mod description_flags {
    /// Every write at the current position goes to the end (ADR-0010 rule 2).
    pub const APPEND: u32 = 1 << 0;
    pub const ALL: u32 = (1 << 1) - 1;
}

/// The reasons a supervisor hears (ADR-0080).
pub mod failure {
    pub const DEVICE_WRITE: u32 = 0;
    pub const DEVICE_FLUSH: u32 = 1;
    pub const METADATA_READ: u32 = 2;
    pub const METADATA_CORRUPT: u32 = 3;
    pub const INTERNAL: u32 = 4;
    pub const DEVICE_LOST: u32 = 5;
    pub const COUNT: u32 = 6;
}

pub type Rights = Flags<{ rights::ALL }>;

protocol! {
    /// Opens `path` under this directory on the given end.
    message Open<'a> = 1, handles 1 {
        flags: Flags<{ open_flags::ALL }>,
        rights: Rights,
        path: Path<'a, MAX_PATH, false>,
    }
    message OpenReply<'a> = 1, handles 0 {
        status: Code,
        /// With `IS_LINK`: the path after the link.
        remaining: Path<'a, MAX_PATH, true>,
        /// With `IS_LINK`: where it points.
        target: Text<'a, MAX_PATH>,
    }

    /// A new connection to this object with equal or fewer rights.
    message Clone = 2, handles 1 {
        rights: Rights,
    }

    /// A new connection to this directory with equal or fewer rights,
    /// through which requests may name only `name` (ADR-0055's note,
    /// ADR-0153 rule 2): an operand that creates, replaces, removes or
    /// renames one name. It cannot list the directory. Answered by `Done`.
    message Narrow<'a> = 36, handles 1 {
        rights: Rights,
        name: Name<'a>,
    }

    message Read = 3, handles 0 {
        offset: u64,
        length: u32,
    }
    message ReadReply<'a> = 3, handles 0 {
        status: Code,
        data: Data<'a, MAX_INLINE>,
    }

    message Write<'a> = 4, handles 0 {
        offset: u64,
        flags: Flags<{ write_flags::ALL }>,
        data: Data<'a, MAX_INLINE>,
    }

    /// Done, with a count of bytes, for every request with no data to give.
    message Done = 4, handles 0 {
        status: Code,
        count: u64,
    }

    /// Attaches a memory object for large transfers, one per connection.
    message Attach = 5, handles 1 {
        /// The object's size, which the mapping checks.
        size: u64,
    }
    message Detach = 6, handles 0 {}

    message ReadBuffer = 7, handles 0 {
        offset: u64,
        buffer_offset: u64,
        length: u32,
    }

    message WriteBuffer = 8, handles 0 {
        offset: u64,
        buffer_offset: u64,
        length: u32,
        flags: Flags<{ write_flags::ALL }>,
    }

    message GetAttributes = 9, handles 0 {}
    message Attributes = 9, handles 0 {
        status: Code,
        size: u64,
        modified: u64,
        changed: u64,
        mode: u32,
        kind: Kind,
    }

    message ReadDirectory = 10, handles 0 {
        /// 0 for the start.
        cookie: u64,
    }
    record Entry<'a> {
        /// Where to go on from this name. Opaque to the client.
        next: u64,
        kind: Kind,
        name: Name<'a>,
    }
    message ReadDirectoryReply<'a> = 10, handles 0 {
        status: Code,
        entries: Seq<'a, Entry<'a>, MAX_ENTRIES>,
    }

    /// `old` here to `new` in the directory of the token.
    message Rename<'a> = 11, handles 1 {
        mode: Below<{ rename_mode::COUNT }>,
        old: Name<'a>,
        new: Name<'a>,
    }

    /// Gives this unnamed file `name` in the directory of the token.
    message Publish<'a> = 12, handles 1 {
        mode: Below<{ publish_mode::COUNT }>,
        name: Name<'a>,
    }

    message Remove<'a> = 13, handles 0 {
        name: Name<'a>,
    }

    message MakeDirectory<'a> = 14, handles 0 {
        mode: u32,
        name: Name<'a>,
    }

    message Sync = 15, handles 0 {}

    /// The volume's state: `STORAGE_FAILED` if it has failed. `Done`'s
    /// count is the most bytes of blocks in use since the mount (proposed
    /// ADR-0211).
    message Status = 16, handles 0 {}

    /// The count of corrupt reads (ADR-0012 rule 3), in `Done`'s count.
    message CorruptReads = 17, handles 0 {}

    /// A token naming this connection in a `Rename` or `Publish`.
    message GetToken = 18, handles 0 {}
    message TokenReply = 18, handles 1 {
        status: Code,
    }

    message SetSize = 19, handles 0 {
        size: u64,
    }

    /// Sets what `which` says: bit 0 the mode, bit 1 `mtime` (ADR-0014).
    message SetAttributes = 20, handles 0 {
        which: Flags<3>,
        mode: u32,
        seconds: u64,
        nanos: u32,
    }

    message MakeLink<'a> = 21, handles 0 {
        name: Name<'a>,
        target: Text<'a, MAX_PATH>,
    }

    message ReadLink = 22, handles 0 {}
    message ReadLinkReply<'a> = 22, handles 0 {
        status: Code,
        target: Text<'a, MAX_PATH>,
    }

    // Stage 6c (ADR-0103): the open description. A connection's record is
    // its description: the open object, a position and the append flag.
    // `share` attaches another connection to the same description, and the
    // requests below with no offset read and write at its position, which
    // the service advances inside the request.

    /// A new connection to this connection's description, with equal or
    /// fewer rights: one position for both (ADR-0103 item 1).
    message Share = 23, handles 1 {
        rights: Rights,
    }

    /// Moves the position, and gives the new one in `Done`'s count. `offset`
    /// is a signed 64-bit number in two's complement.
    message Seek = 24, handles 0 {
        whence: Below<{ whence::COUNT }>,
        offset: u64,
    }

    message SetFlags = 25, handles 0 {
        flags: Flags<{ description_flags::ALL }>,
    }

    /// The description's flags, in `Done`'s count.
    message GetFlags = 26, handles 0 {}

    /// `open` with the description's flags and, for a create, the POSIX mode
    /// (ADR-0014 rule 6).
    message OpenWithMode<'a> = 27, handles 1 {
        flags: Flags<{ open_flags::ALL }>,
        rights: Rights,
        description: Flags<{ description_flags::ALL }>,
        mode: u32,
        path: Path<'a, MAX_PATH, false>,
    }
    /// `OpenReply`, and what `StatReply` gives of the object opened, and
    /// whether this open made it: what a POSIX open needs to decide the rest
    /// without another round trip.
    message OpenWithModeReply<'a> = 27, handles 0 {
        status: Code,
        created: Below<2>,
        volume: u64,
        id: u64,
        size: u64,
        modified_seconds: u64,
        modified_nanos: u32,
        changed_seconds: u64,
        changed_nanos: u32,
        mode: u32,
        kind: Kind,
        /// With `IS_LINK`, as in `OpenReply`.
        remaining: Path<'a, MAX_PATH, true>,
        target: Text<'a, MAX_PATH>,
    }

    /// `read` at the position, which moves past what was read. The reply is
    /// `ReadReply`.
    message ReadCurrent = 28, handles 0 {
        length: u32,
    }

    /// `write` at the position, or at the end with `APPEND` in the flags or
    /// the description; the position moves past what was written.
    message WriteCurrent<'a> = 29, handles 0 {
        flags: Flags<{ write_flags::ALL }>,
        data: Data<'a, MAX_INLINE>,
    }

    message ReadBufferCurrent = 30, handles 0 {
        buffer_offset: u64,
        length: u32,
    }

    message WriteBufferCurrent = 31, handles 0 {
        buffer_offset: u64,
        length: u32,
        flags: Flags<{ write_flags::ALL }>,
    }

    /// Everything `struct stat` is made from (ADR-0103 item 10, ADR-0014):
    /// the volume, the object's id, which is never reused, and its times
    /// exactly, seconds being signed in two's complement.
    message Stat = 32, handles 0 {}
    message StatReply = 32, handles 0 {
        status: Code,
        volume: u64,
        id: u64,
        size: u64,
        modified_seconds: u64,
        modified_nanos: u32,
        changed_seconds: u64,
        changed_nanos: u32,
        mode: u32,
        kind: Kind,
    }

    /// `read_directory` with each entry's object id (ADR-0014 rule 7), for
    /// `d_ino`; the cookie is the same connection's as `read_directory`'s.
    message ReadDirectoryIds = 33, handles 0 {
        cookie: u64,
    }
    record IdEntry<'a> {
        next: u64,
        kind: Kind,
        /// The object's id, never reused (ADR-0014 rule 1).
        id: u64,
        name: Name<'a>,
    }
    message ReadDirectoryIdsReply<'a> = 33, handles 0 {
        status: Code,
        entries: Seq<'a, IdEntry<'a>, MAX_ENTRIES>,
    }

    /// On a connection to a FIFO node of the temporary directory service
    /// (ADR-0107 item 14): a stream end for one side, answered at once for
    /// a read side with `nonblock`, refused with `NOT_FOUND` (`ENXIO`) for
    /// a write side with `nonblock` while no reader is there, and otherwise
    /// held until the other side has a connection. `UNSUPPORTED` from a
    /// service with no FIFOs, `INVALID_ARGUMENT` on an object that is not
    /// one; failures are the generic error reply.
    message FifoOpen = 34, handles 0 {
        /// `fifo_side::READ` or `fifo_side::WRITE`.
        side: Below<2>,
        nonblock: Below<2>,
    }
    message FifoOpenReply = 34, handles 1 {
        status: Code,
    }

    /// The boot's time base in `Done`'s count (ADR-0191): nanoseconds that
    /// the service adds to the monotonic clock for the times it assigns.
    message TimeBase = 35, handles 0 {}

    /// The volume's free bytes in `Done`'s count, on any connection: the
    /// file manager's status line (ADR-0248).
    message FreeSpace = 37, handles 0 {}

    /// A failed volume, on the supervisor channel (ADR-0080).
    message Failure = 100, handles 0 {
        reason: Below<{ failure::COUNT }>,
    }
}

/// Every request's ordinal, for a test that none repeats.
pub const REQUESTS: [(&str, u32); 37] = [
    ("open", 1),
    ("clone", 2),
    ("read", 3),
    ("write", 4),
    ("attach", 5),
    ("detach", 6),
    ("read_buffer", 7),
    ("write_buffer", 8),
    ("get_attributes", 9),
    ("read_directory", 10),
    ("rename", 11),
    ("publish", 12),
    ("remove", 13),
    ("make_directory", 14),
    ("sync", 15),
    ("status", 16),
    ("corrupt_reads", 17),
    ("get_token", 18),
    ("set_size", 19),
    ("set_attributes", 20),
    ("make_link", 21),
    ("read_link", 22),
    ("share", 23),
    ("seek", 24),
    ("set_flags", 25),
    ("get_flags", 26),
    ("open_with_mode", 27),
    ("read_current", 28),
    ("write_current", 29),
    ("read_buffer_current", 30),
    ("write_buffer_current", 31),
    ("stat", 32),
    ("read_directory_ids", 33),
    ("fifo_open", 34),
    ("time_base", 35),
    ("narrow", 36),
    ("free_space", 37),
];

/// `fifo_open`'s side.
pub mod fifo_side {
    pub const READ: u32 = 0;
    pub const WRITE: u32 = 1;
}

/// The type bits of a FIFO's `posix_mode` (`S_IFIFO`), which the temporary
/// directory service gives their meaning (ADR-0107 item 13).
pub const S_IFIFO: u32 = 0o010000;
/// The type bits' mask (`S_IFMT`).
pub const S_IFMT: u32 = 0o170000;

#[cfg(test)]
extern crate std;

#[cfg(test)]
mod tests {
    use super::*;
    use std::vec::Vec;
    use zoraya_protocol::testing::exercise;
    use zoraya_protocol::{Message, decode, encode};

    fn check<M>(message: M, handles: usize)
    where
        M: for<'b> Message<'b> + PartialEq + core::fmt::Debug,
    {
        let mut out = [0; 2048];
        let length = encode(&message, 1, &mut out).unwrap();
        assert_eq!(M::HANDLES, handles);
        let (_, back) = decode::<M>(&out[..length], handles).unwrap();
        assert_eq!(back, message);
        exercise(&out[..length], |bytes| {
            let _ = decode::<M>(bytes, handles);
        });
    }

    #[test]
    fn fixed_messages_round_trip_and_decode_totally() {
        check(
            Clone {
                rights: Flags(rights::READ),
            },
            1,
        );
        check(
            Read {
                offset: 7,
                length: 9,
            },
            0,
        );
        check(
            Done {
                status: Below(status::TOO_MANY_OPEN),
                count: 3,
            },
            0,
        );
        check(Attach { size: 4096 }, 1);
        check(Detach {}, 0);
        check(
            ReadBuffer {
                offset: 1,
                buffer_offset: 4096,
                length: 8,
            },
            0,
        );
        check(
            WriteBuffer {
                offset: 1,
                buffer_offset: 4096,
                length: 8,
                flags: Flags(write_flags::APPEND),
            },
            0,
        );
        check(GetAttributes {}, 0);
        check(
            Attributes {
                status: Below(0),
                size: 1,
                modified: 2,
                changed: 3,
                mode: 0o644,
                kind: Below(1),
            },
            0,
        );
        check(ReadDirectory { cookie: 4 }, 0);
        check(Sync {}, 0);
        check(Status {}, 0);
        check(CorruptReads {}, 0);
        check(GetToken {}, 0);
        check(SetSize { size: 9 }, 0);
        check(
            SetAttributes {
                which: Flags(1),
                mode: 0o600,
                seconds: 1,
                nanos: 2,
            },
            0,
        );
        check(ReadLink {}, 0);
        check(
            Failure {
                reason: Below(failure::DEVICE_LOST),
            },
            0,
        );
        check(TokenReply { status: Below(0) }, 1);
        check(
            Share {
                rights: Flags(rights::READ | rights::CLONE),
            },
            1,
        );
        check(
            Seek {
                whence: Below(whence::END),
                offset: (-5i64) as u64,
            },
            0,
        );
        check(
            SetFlags {
                flags: Flags(description_flags::APPEND),
            },
            0,
        );
        check(GetFlags {}, 0);
        check(Stat {}, 0);
        check(ReadDirectoryIds { cookie: 9 }, 0);
        check(
            FifoOpen {
                side: Below(fifo_side::WRITE),
                nonblock: Below(1),
            },
            0,
        );
        check(TimeBase {}, 0);
        check(FreeSpace {}, 0);
        check(
            FifoOpenReply {
                status: Below(status::OK),
            },
            1,
        );
        check(
            StatReply {
                status: Below(0),
                volume: 1,
                id: 2,
                size: 3,
                modified_seconds: (-1i64) as u64,
                modified_nanos: 999_999_999,
                changed_seconds: 5,
                changed_nanos: 6,
                mode: 0o4755,
                kind: Below(kind::LINK),
            },
            0,
        );
        check(ReadCurrent { length: 4096 }, 0);
        check(
            ReadBufferCurrent {
                buffer_offset: 8,
                length: 9,
            },
            0,
        );
        check(
            WriteBufferCurrent {
                buffer_offset: 8,
                length: 9,
                flags: Flags(write_flags::DURABLE),
            },
            0,
        );
    }

    #[test]
    fn the_stage_6c_fields_refuse_what_they_do_not_name() {
        let mut out = [0; 64];
        let seek = Seek {
            whence: Below(whence::COUNT),
            offset: 0,
        };
        assert!(encode(&seek, 1, &mut out).is_err(), "no fourth origin");
        // An unknown description flag goes out, and the decoder refuses it.
        let flags = SetFlags {
            flags: Flags(description_flags::ALL + 1),
        };
        let length = encode(&flags, 1, &mut out).unwrap();
        assert!(decode::<SetFlags>(&out[..length], 0).is_err());
    }

    /// Each borrowed message, by hand: the lifetime stops a generic helper.
    macro_rules! borrowed {
        ($message:expr, $type:ty, $handles:expr) => {{
            let message = $message;
            let mut out = [0; 8192];
            let length = encode(&message, 1, &mut out).unwrap();
            let (_, back) = decode::<$type>(&out[..length], $handles).unwrap();
            assert_eq!(back, message);
            exercise(&out[..length.min(4096)], |bytes| {
                let _ = decode::<$type>(bytes, $handles);
            });
        }};
    }

    #[test]
    fn borrowed_messages_round_trip_and_decode_totally() {
        borrowed!(
            Open {
                flags: Flags(open_flags::CREATE),
                rights: Flags(rights::ALL),
                path: Path("a/b/c"),
            },
            Open<'_>,
            1
        );
        borrowed!(
            OpenReply {
                status: Below(status::IS_LINK),
                remaining: Path("rest"),
                target: Text("../x"),
            },
            OpenReply<'_>,
            0
        );
        borrowed!(
            ReadReply {
                status: Below(0),
                data: Data(b"contents"),
            },
            ReadReply<'_>,
            0
        );
        borrowed!(
            Write {
                offset: 3,
                flags: Flags(write_flags::DURABLE),
                data: Data(b"x"),
            },
            Write<'_>,
            0
        );
        borrowed!(
            Rename {
                mode: Below(rename_mode::NO_REPLACE),
                old: Name("a"),
                new: Name("b"),
            },
            Rename<'_>,
            1
        );
        borrowed!(
            Publish {
                mode: Below(publish_mode::REPLACE),
                name: Name("n"),
            },
            Publish<'_>,
            1
        );
        borrowed!(
            MakeLink {
                name: Name("n"),
                target: Text("/t"),
            },
            MakeLink<'_>,
            0
        );
        borrowed!(
            ReadLinkReply {
                status: Below(0),
                target: Text("t"),
            },
            ReadLinkReply<'_>,
            0
        );
        borrowed!(Remove { name: Name("n") }, Remove<'_>, 0);
        borrowed!(
            OpenWithMode {
                flags: Flags(open_flags::CREATE | open_flags::TRUNCATE),
                rights: Flags(rights::READ | rights::WRITE),
                description: Flags(description_flags::APPEND),
                mode: 0o755,
                path: Path("bin/a.out"),
            },
            OpenWithMode<'_>,
            1
        );
        borrowed!(
            OpenWithModeReply {
                status: Below(status::OK),
                remaining: Path(""),
                target: Text(""),
                created: Below(1),
                volume: 1,
                id: 9,
                size: 0,
                modified_seconds: 10,
                modified_nanos: 11,
                changed_seconds: 12,
                changed_nanos: 13,
                mode: 0o600,
                kind: Below(kind::FILE),
            },
            OpenWithModeReply<'_>,
            0
        );
        borrowed!(
            WriteCurrent {
                flags: Flags(write_flags::APPEND),
                data: Data("Київ, Plzeň".as_bytes()),
            },
            WriteCurrent<'_>,
            0
        );
        borrowed!(
            MakeDirectory {
                mode: 0o755,
                name: Name("d"),
            },
            MakeDirectory<'_>,
            0
        );
    }

    #[test]
    fn a_directory_reply_round_trips_and_decodes_totally() {
        let entries = [
            Entry {
                next: 2,
                kind: Below(1),
                name: Name("one"),
            },
            Entry {
                next: 0,
                kind: Below(2),
                name: Name("two"),
            },
        ];
        let reply = ReadDirectoryReply {
            status: Below(0),
            entries: Seq::Of(&entries),
        };
        let mut out = [0; 512];
        let length = encode(&reply, 1, &mut out).unwrap();
        let (_, back) = decode::<ReadDirectoryReply<'_>>(&out[..length], 0).unwrap();
        let got: Vec<_> = back.entries.iter().collect();
        assert_eq!(got, entries);
        exercise(&out[..length], |bytes| {
            if let Ok((_, m)) = decode::<ReadDirectoryReply<'_>>(bytes, 0) {
                assert!(m.entries.iter().all(|e| Name::is_valid(e.name.0)));
            }
        });
    }

    #[test]
    fn a_listing_with_ids_round_trips_and_decodes_totally() {
        let entries = [
            IdEntry {
                next: 2,
                kind: Below(kind::FILE),
                id: 77,
                name: Name("Київ"),
            },
            IdEntry {
                next: 0,
                kind: Below(kind::DIRECTORY),
                id: u64::MAX,
                name: Name("Plzeň"),
            },
        ];
        let reply = ReadDirectoryIdsReply {
            status: Below(0),
            entries: Seq::Of(&entries),
        };
        let mut out = [0; 512];
        let length = encode(&reply, 1, &mut out).unwrap();
        let (_, back) = decode::<ReadDirectoryIdsReply<'_>>(&out[..length], 0).unwrap();
        let got: Vec<_> = back.entries.iter().collect();
        assert_eq!(got, entries);
        exercise(&out[..length], |bytes| {
            if let Ok((_, m)) = decode::<ReadDirectoryIdsReply<'_>>(bytes, 0) {
                assert!(m.entries.iter().all(|e| Name::is_valid(e.name.0)));
            }
        });
    }

    #[test]
    fn a_name_that_climbs_is_refused_on_the_wire() {
        let mut out = [0; 64];
        let good = Remove { name: Name("ab") };
        let length = encode(&good, 1, &mut out).unwrap();
        // The name's bytes are the last two: make it `..`.
        out[length - 2..length].copy_from_slice(b"..");
        assert!(decode::<Remove<'_>>(&out[..length], 0).is_err());
    }

    #[test]
    fn no_ordinal_repeats() {
        for (i, (_, a)) in REQUESTS.iter().enumerate() {
            assert_ne!(*a, zoraya_protocol::ERROR_ORDINAL);
            assert!(REQUESTS[i + 1..].iter().all(|(_, b)| a != b));
        }
    }
}

// A runaway test fails at 512 MiB instead of taking the host (docs/backlog.md).
#[cfg(test)]
zoraya_test_alloc::cap_memory!();
