//! What a POSIX process starts with (ADR-0107): the bootstrap record its
//! spawner writes on a channel, the fixed slots above the fd numbers, and the
//! report of a refused path (ADR-0153 rule 11). Integers are little-endian;
//! the decoders are total, as `SpawnRequest`'s are.
//!
//! ```text
//! magic "ZPB1"     4 bytes
//! header size      u32
//! pid, ppid        u32 each, 0 for none (a process the broker did not make)
//! pgid, sid        u32 each
//! uid, gid         u32 each
//! umask            u32, at most 0o777
//! ignored          u32, the signals ignored on entry, bit n for signal n
//! flags            u32: ABSOLUTE
//! cwd entry        u32, an index into the entries
//! fd count         u32
//! entry count      u32
//! realtime base   u64: nanoseconds added to the monotonic clock for
//!                  `CLOCK_REALTIME`, one per boot (ADR-0191); 0 for none
//! blocked          u32, the signal mask on entry, bit n for signal n
//!                  (ADR-0190 item 18); SIGKILL and SIGSTOP never
//! reserved         u32, zero
//! exe              u32 length, bytes: the executable's path
//! cwd              u32 length, bytes: relative to the cwd entry's prefix
//! fds              FD_SIZE bytes each: fd u32, kind u8, flags u8, tty u8,
//!                  zero; in increasing fd order
//! entries          kind u8, handle u8, two zero bytes, u32 length, prefix;
//!                  longest prefix first
//! ```
//!
//! A handle index names a handle of the message the record came in, in the
//! order of its handle list; `NO_HANDLE` is none.
//!
//! A child started with a chosen table (ADR-0433, plan part F4) may have no
//! cwd: the cwd entry is [`NO_ENTRY`] and the cwd empty, the only form a table
//! of no entries has. Any other cwd entry is a directory of the table.

use crate::Handle;

const MAGIC: &[u8; 4] = b"ZPB1";
/// The size of this version's header.
pub const HEADER_SIZE: usize = 72;
const FD_SIZE: usize = 8;
const ENTRY_HEADER: usize = 8;

/// The fd numbers a process can have (ADR-0103 item 4).
pub const OPEN_MAX: u32 = 512;
/// The longest path in the record: a file protocol path (ADR-0055).
pub const MAX_PATH: usize = 4096;
/// The most handles a channel message carries (ADR-0050).
pub const MAX_MESSAGE_HANDLES: usize = 64;
/// No handle.
pub const NO_HANDLE: u8 = 0xff;
/// The cwd entry of a record whose process has no current directory (ADR-0433).
pub const NO_ENTRY: u32 = u32::MAX;

/// The fixed slots, all above the fd numbers (ADR-0107 item 5, ADR-0153 rule
/// 11). The fds are at their numbers.
pub mod slot {
    use crate::Handle;

    /// The bootstrap channel's end: read at start-up and closed.
    pub const BOOTSTRAP: Handle = Handle(1000);
    /// The broker's channel (ADR-0104), when the process has a pid.
    pub const BROKER: Handle = Handle(1001);
    /// The process's own job (ADR-0104 item 4).
    pub const PROCESS_JOB: Handle = Handle(1002);
    /// The process itself, with `WAIT`, in a `fork` child (the snapshot's
    /// `self_slot`, ADR-0102). A spawned child receives it in the bootstrap
    /// channel's second message instead, at whatever slot that gives it,
    /// since a spawner cannot list it.
    pub const SELF: Handle = Handle(1003);
    /// The write end of the channel refused paths are reported on.
    pub const REPORT: Handle = Handle(1004);
    /// fd 0's handle: slot 0 holds none, so fd 0 is the one fd not at its
    /// number (ADR-0190).
    pub const FD0: Handle = Handle(1005);
    /// The read end of the channel the shell gives a running program more
    /// files on (ADR-0337 item 7): messages of [`crate::posix::Grants`].
    pub const GRANTS: Handle = Handle(1006);
    pub const ALL: [Handle; 7] = [BOOTSTRAP, BROKER, PROCESS_JOB, SELF, REPORT, FD0, GRANTS];

    /// The slot that holds fd `fd`'s handle: its number, but fd 0's.
    pub const fn of_fd(fd: u32) -> u32 {
        if fd == 0 { FD0.slot() } else { fd }
    }
}

/// The rights of the job handles the broker gives (ADR-0104 items 2, 4 and
/// 10). There is no call that reads a handle's rights, and a moved handle
/// must list rights it holds, so the holders name them from here.
pub mod job_rights {
    use crate::Rights;

    /// The broker's own handle to a process job.
    pub const TABLE: Rights = Rights::DUPLICATE
        .union(Rights::TRANSFER)
        .union(Rights::WAIT)
        .union(Rights::SIGNAL)
        .union(Rights::CREATE_PROCESS)
        .union(Rights::KILL);
    /// The parent's handle to a child's process job.
    pub const PARENT: Rights = Rights::WAIT
        .union(Rights::DUPLICATE)
        .union(Rights::TRANSFER);
    /// A process's handle to its own process job, at `PROCESS_JOB`.
    pub const OWN: Rights = Rights::CREATE_PROCESS.union(Rights::TRANSFER);
    /// The terminal's handle to the foreground group's job and to the
    /// session's.
    pub const TERMINAL: Rights = Rights::SIGNAL.union(Rights::KILL);
}

/// `flags`: relative paths are joined to cwd's absolute path and `..` leaves
/// an entry (ADR-0153 rule 6, `zosh`'s tables); without it, `..` stops at the
/// root of cwd's entry (ADR-0107 item 6, INV-0002-4).
pub const ABSOLUTE: u32 = 1;
const FLAGS: u32 = ABSOLUTE;

/// An fd's flag that the description in the library holds (ADR-0103 item 4).
pub const FD_NONBLOCK: u8 = 1;
/// A file's access, which only the spawner's description knows (ADR-0190
/// item 17): `FD_READ`, `FD_WRITE`, both, or neither for read and write as a
/// record of stage 6c says.
pub const FD_READ: u8 = 2;
pub const FD_WRITE: u8 = 4;
const FD_FLAGS: u8 = FD_NONBLOCK | FD_READ | FD_WRITE;

/// The second message on the bootstrap channel, with the `SELF` handle.
pub const SELF_MESSAGE: &[u8; 4] = b"ZPBS";

/// The second message when the launcher also gives a connection to `netd`
/// (ADR-0225 item 2; ADR-0287): two handles, `SELF` first and the
/// connection second. A fixed slot would not do: a handle that arrives in a
/// message takes the slot after the last one `spawn` filled, so a program
/// without a network would find a directory's channel in it.
pub const SELF_NET_MESSAGE: &[u8; 4] = b"ZPBN";

/// The second message when the launcher gives the connection to the random
/// service (ADR-0227 item 3; proposed ADR-0310): the four bytes and one byte
/// of flags, [`EXT_NET`], [`EXT_RANDOM`] and [`EXT_WINDOW`], with the handles in this order:
/// `SELF`, then the connection to `netd` if the first flag is set, then the
/// connection to the random service if the second is, then the window channel if the third is. Any other flag bit
/// makes the message unknown: the library takes `SELF` if it can and keeps
/// the rest closed.
pub const SELF_EXT_MESSAGE: &[u8; 4] = b"ZPBX";
/// A connection to `netd` follows `SELF` (as in [`SELF_NET_MESSAGE`]).
pub const EXT_NET: u8 = 1;
/// A connection to the random service follows.
pub const EXT_RANDOM: u8 = 2;
/// The client end of a window channel follows, after the others (proposed
/// ADR-0317): the compositor's, for a program whose description has `window`.
pub const EXT_WINDOW: u8 = 4;
/// The handles after `SELF` that a `ZPBX` message with `flags` carries, or
/// `None` for a flag this library does not know.
pub const fn ext_handles(flags: u8) -> Option<usize> {
    if flags & !(EXT_NET | EXT_RANDOM | EXT_WINDOW) != 0 {
        return None;
    }
    Some(
        (flags & EXT_NET != 0) as usize
            + (flags & EXT_RANDOM != 0) as usize
            + (flags & EXT_WINDOW != 0) as usize,
    )
}

/// What a slot below `OPEN_MAX` holds (ADR-0107 item 3).
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FdKind {
    /// A file connection.
    File = 0,
    /// A directory connection.
    Dir = 1,
    StreamRead = 2,
    StreamWrite = 3,
    /// A stream end at the slot, and the terminal connection by `tty`.
    Tty = 4,
    /// No handle: `/dev/null`.
    Null = 5,
}

impl FdKind {
    pub const fn from_raw(raw: u8) -> Option<FdKind> {
        Some(match raw {
            0 => FdKind::File,
            1 => FdKind::Dir,
            2 => FdKind::StreamRead,
            3 => FdKind::StreamWrite,
            4 => FdKind::Tty,
            5 => FdKind::Null,
            _ => return None,
        })
    }

    /// True if the kind has a handle at the fd's slot.
    pub const fn has_slot(self) -> bool {
        !matches!(self, FdKind::Null)
    }
}

/// What a namespace entry is (ADR-0107 item 4).
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    /// A directory connection.
    Directory = 0,
    /// The temporary directory service's root: a directory connection.
    Tmp = 1,
    /// `/dev/null`: no handle.
    Null = 2,
    /// The terminal connection.
    Terminal = 3,
    /// The process's own image (`/proc/self/exe`).
    Image = 4,
    /// The program's own directory (ADR-0341): a directory connection that
    /// `libposix` hands a child as a second connection with read rights only
    /// (item 7), and whose contents a link outside it does not lead into.
    Own = 5,
}

impl EntryKind {
    pub const fn from_raw(raw: u8) -> Option<EntryKind> {
        Some(match raw {
            0 => EntryKind::Directory,
            1 => EntryKind::Tmp,
            2 => EntryKind::Null,
            3 => EntryKind::Terminal,
            4 => EntryKind::Image,
            5 => EntryKind::Own,
            _ => return None,
        })
    }

    /// True if the entry names a directory a path can go on into.
    pub const fn is_directory(self) -> bool {
        matches!(self, EntryKind::Directory | EntryKind::Tmp | EntryKind::Own)
    }
}

/// Signal numbers (musl's AArch64 numbering, Linux's) and their request
/// kinds (ADR-0106 item 1), for the broker and `libposix`.
pub mod signal {
    use crate::RequestKind;

    pub const SIGHUP: u32 = 1;
    pub const SIGINT: u32 = 2;
    pub const SIGQUIT: u32 = 3;
    pub const SIGABRT: u32 = 6;
    pub const SIGKILL: u32 = 9;
    pub const SIGPIPE: u32 = 13;
    pub const SIGTERM: u32 = 15;
    pub const SIGCHLD: u32 = 17;
    pub const SIGCONT: u32 = 18;
    pub const SIGSTOP: u32 = 19;
    pub const SIGTSTP: u32 = 20;
    pub const SIGTTIN: u32 = 21;
    pub const SIGTTOU: u32 = 22;
    pub const SIGURG: u32 = 23;
    pub const SIGWINCH: u32 = 28;
    /// The highest standard signal.
    pub const MAX: u32 = 31;

    /// The request kind of signal `n`, 1 to 31: the four fixed kinds, else
    /// `Custom(n)`.
    pub const fn kind(n: u32) -> Option<RequestKind> {
        if n == 0 || n > MAX {
            return None;
        }
        Some(match n {
            SIGINT => RequestKind::Interrupt,
            SIGTERM => RequestKind::Terminate,
            SIGHUP => RequestKind::Hangup,
            SIGQUIT => RequestKind::Quit,
            _ => RequestKind::Custom(n),
        })
    }

    /// The signal a request stands for: `detail` when it is a valid number,
    /// else the kind's own (ADR-0046's rule); `Custom(n)` beyond 31 is 15.
    pub const fn number(kind: RequestKind, detail: u32) -> u32 {
        if detail >= 1 && detail <= MAX {
            return detail;
        }
        match kind {
            RequestKind::Interrupt => SIGINT,
            RequestKind::Terminate => SIGTERM,
            RequestKind::Hangup => SIGHUP,
            RequestKind::Quit => SIGQUIT,
            RequestKind::Custom(n) if n >= 1 && n <= MAX => n,
            RequestKind::Custom(_) => SIGTERM,
        }
    }

    /// True if the default action of signal `n` ends the process (ADR-0106
    /// item 1: 1 to 16 but 9, 24 to 27, 29 to 31). `SIGKILL` is the kernel's
    /// `job_kill`, not a request.
    pub const fn default_terminates(n: u32) -> bool {
        matches!(n, 1..=8 | 10..=16 | 24..=27 | 29..=31)
    }

    /// The stop signals and `SIGCONT`, which the broker carries out by the
    /// target's disposition (ADR-0104 items 8a and 9).
    pub const fn is_stop_or_continue(n: u32) -> bool {
        matches!(n, SIGCONT..=SIGTTOU)
    }
}

/// The process's POSIX identity (ADR-0107 item 1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Facts {
    pub pid: u32,
    pub ppid: u32,
    pub pgid: u32,
    pub sid: u32,
    pub uid: u32,
    pub gid: u32,
    pub umask: u32,
    pub ignored: u32,
    /// The boot's time base (ADR-0191): `CLOCK_REALTIME` is this plus the
    /// monotonic clock; 0 when the spawner had none.
    pub realtime_base: u64,
    /// The signal mask on entry (ADR-0190 item 18), bit n for signal n.
    pub blocked: u32,
}

/// One fd of the record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Fd {
    pub fd: u32,
    pub kind: FdKind,
    /// `FD_NONBLOCK`, `FD_READ`, `FD_WRITE`.
    pub flags: u8,
    /// For a `Tty`, the index of its terminal connection; else `NO_HANDLE`.
    pub tty: u8,
}

/// One namespace entry of the record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Entry<'a> {
    pub prefix: &'a str,
    pub kind: EntryKind,
    /// An index into the message's handles, or `NO_HANDLE`.
    pub handle: u8,
}

/// A decoded record. The fd and entry lists are kept as the bytes they were
/// checked in, and iterate without failing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Bootstrap<'a> {
    pub facts: Facts,
    /// `ABSOLUTE` or 0.
    pub flags: u32,
    pub exe: &'a str,
    pub cwd_entry: u32,
    pub cwd: &'a str,
    fds: &'a [u8],
    entries: &'a [u8],
    fd_count: usize,
    entry_count: usize,
}

fn u32_at(bytes: &[u8], at: usize) -> Option<u32> {
    let b = bytes.get(at..at.checked_add(4)?)?;
    Some(u32::from_le_bytes(b.try_into().ok()?))
}

/// An absolute path in normal form: `/`, or `/` and names separated by
/// single slashes, none `.` or `..`, no trailing slash.
pub fn is_absolute_normal(path: &str) -> bool {
    match path.strip_prefix('/') {
        Some("") => true,
        Some(rest) => is_relative_normal(rest) && !rest.is_empty(),
        None => false,
    }
}

/// A relative path in normal form: empty, or names separated by single
/// slashes, none `.` or `..`, and no NUL.
pub fn is_relative_normal(path: &str) -> bool {
    path.is_empty()
        || path
            .split('/')
            .all(|c| !c.is_empty() && c != "." && c != ".." && !c.contains('\0'))
}

/// `length`-prefixed UTF-8 at `at`, at most `MAX_PATH` bytes; and where the
/// next field starts.
fn string_at(bytes: &[u8], at: usize) -> Option<(&str, usize)> {
    let length = u32_at(bytes, at)? as usize;
    if length > MAX_PATH {
        return None;
    }
    let start = at.checked_add(4)?;
    let end = start.checked_add(length)?;
    let text = core::str::from_utf8(bytes.get(start..end)?).ok()?;
    Some((text, end))
}

fn fd_at(bytes: &[u8], at: usize) -> Fd {
    Fd {
        fd: u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]]),
        kind: FdKind::from_raw(bytes[at + 4]).unwrap_or(FdKind::Null),
        flags: bytes[at + 5],
        tty: bytes[at + 6],
    }
}

/// The one name of a terminal in a namespace table (ADR-0105 item 6,
/// ADR-0338 INV-0338-2).
pub const TTY_PATH: &str = "/dev/tty";

impl<'a> Bootstrap<'a> {
    /// Decodes a record that came with `handles` handles. Any input gives a
    /// result, never a panic.
    pub fn decode(bytes: &'a [u8], handles: usize) -> Option<Bootstrap<'a>> {
        if bytes.get(..4)? != MAGIC || u32_at(bytes, 4)? as usize != HEADER_SIZE {
            return None;
        }
        let word = |i: usize| u32_at(bytes, 8 + 4 * i);
        let facts = Facts {
            pid: word(0)?,
            ppid: word(1)?,
            pgid: word(2)?,
            sid: word(3)?,
            uid: word(4)?,
            gid: word(5)?,
            umask: word(6)?,
            ignored: word(7)?,
            realtime_base: u64::from(word(12)?) | (u64::from(word(13)?) << 32),
            blocked: word(14)?,
        };
        let flags = word(8)?;
        let cwd_entry = word(9)?;
        let fd_count = word(10)? as usize;
        let entry_count = word(11)? as usize;
        // The cwd is in one of the entries, or there is none (ADR-0433).
        let cwd_ok = cwd_entry == NO_ENTRY || (cwd_entry as usize) < entry_count;
        if facts.umask > 0o777
            || word(15)? != 0
            || flags & !FLAGS != 0
            || fd_count > OPEN_MAX as usize
            || !cwd_ok
        {
            return None;
        }
        let (exe, at) = string_at(bytes, HEADER_SIZE)?;
        let (cwd, at) = string_at(bytes, at)?;
        if !is_relative_normal(cwd) {
            return None;
        }
        // Every handle index is used once at most.
        let mut used = 0u64;
        let mut take = |index: u8| -> bool {
            if (index as usize) >= handles.min(MAX_MESSAGE_HANDLES) || used & (1 << index) != 0 {
                return false;
            }
            used |= 1 << index;
            true
        };
        let fds_end = at.checked_add(fd_count.checked_mul(FD_SIZE)?)?;
        let fds = bytes.get(at..fds_end)?;
        let mut last = None;
        for raw in fds.as_chunks::<FD_SIZE>().0 {
            let fd = u32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]]);
            let kind = FdKind::from_raw(raw[4])?;
            let tty_ok = match kind {
                FdKind::Tty => take(raw[6]),
                _ => raw[6] == NO_HANDLE,
            };
            if fd >= OPEN_MAX
                || last.is_some_and(|l| fd <= l)
                || raw[5] & !FD_FLAGS != 0
                || !tty_ok
                || raw[7] != 0
            {
                return None;
            }
            last = Some(fd);
        }
        let mut at = fds_end;
        let entries_start = at;
        let mut previous: Option<&str> = None;
        let mut cwd_kind = None;
        for index in 0..entry_count {
            let head = bytes.get(at..at.checked_add(ENTRY_HEADER)?)?;
            let kind = EntryKind::from_raw(head[0])?;
            let handle = head[1];
            let (prefix, end) = string_at(bytes, at + 4)?;
            let handle_ok = match kind {
                EntryKind::Null => handle == NO_HANDLE,
                _ => take(handle),
            };
            // INV-0338-2: the only terminal a table names is the process's
            // own `/dev/tty`, and that name is nothing else.
            if head[2..4] != [0, 0]
                || !handle_ok
                || (kind == EntryKind::Terminal) != (prefix == TTY_PATH)
                || !is_absolute_normal(prefix)
                || previous.is_some_and(|p| p.len() < prefix.len() || p == prefix)
            {
                return None;
            }
            if index == cwd_entry as usize {
                cwd_kind = Some(kind);
            }
            previous = Some(prefix);
            at = end;
        }
        let cwd_good = if cwd_entry == NO_ENTRY {
            cwd.is_empty()
        } else {
            cwd_kind.is_some_and(EntryKind::is_directory)
        };
        if at != bytes.len() || !cwd_good {
            return None;
        }
        Some(Bootstrap {
            facts,
            flags,
            exe,
            cwd_entry,
            cwd,
            fds,
            entries: &bytes[entries_start..at],
            fd_count,
            entry_count,
        })
    }

    pub fn fd_count(&self) -> usize {
        self.fd_count
    }

    pub fn entry_count(&self) -> usize {
        self.entry_count
    }

    pub fn fds(&self) -> impl Iterator<Item = Fd> + 'a {
        let fds = self.fds;
        (0..self.fd_count).map(move |i| fd_at(fds, i * FD_SIZE))
    }

    pub fn entries(&self) -> Entries<'a> {
        Entries {
            bytes: self.entries,
            left: self.entry_count,
        }
    }
}

/// The entries of a decoded record, in order.
#[derive(Debug, Clone)]
pub struct Entries<'a> {
    bytes: &'a [u8],
    left: usize,
}

impl<'a> Iterator for Entries<'a> {
    type Item = Entry<'a>;

    fn next(&mut self) -> Option<Entry<'a>> {
        if self.left == 0 {
            return None;
        }
        self.left -= 1;
        // Checked by `decode`: these cannot fail.
        let kind = EntryKind::from_raw(self.bytes[0]).unwrap_or(EntryKind::Null);
        let handle = self.bytes[1];
        let (prefix, end) = string_at(self.bytes, 4).unwrap_or(("/", self.bytes.len()));
        self.bytes = &self.bytes[end..];
        Some(Entry {
            prefix,
            kind,
            handle,
        })
    }
}

/// Writes a record into `out`, and gives its size; `None` if it does not fit
/// or `decode` would refuse it (with `handles` handles in its message).
#[allow(clippy::too_many_arguments)] // Reason: one per part of the record.
pub fn encode(
    facts: &Facts,
    flags: u32,
    exe: &str,
    cwd_entry: u32,
    cwd: &str,
    fds: &[Fd],
    entries: &[Entry<'_>],
    handles: usize,
    out: &mut [u8],
) -> Option<usize> {
    let mut at = 0usize;
    let mut put = |bytes: &[u8]| -> Option<()> {
        let end = at.checked_add(bytes.len())?;
        out.get_mut(at..end)?.copy_from_slice(bytes);
        at = end;
        Some(())
    };
    put(MAGIC)?;
    for word in [
        HEADER_SIZE as u32,
        facts.pid,
        facts.ppid,
        facts.pgid,
        facts.sid,
        facts.uid,
        facts.gid,
        facts.umask,
        facts.ignored,
        flags,
        cwd_entry,
        u32::try_from(fds.len()).ok()?,
        u32::try_from(entries.len()).ok()?,
    ] {
        put(&word.to_le_bytes())?;
    }
    put(&facts.realtime_base.to_le_bytes())?;
    put(&facts.blocked.to_le_bytes())?;
    put(&0u32.to_le_bytes())?;
    for text in [exe, cwd] {
        put(&u32::try_from(text.len()).ok()?.to_le_bytes())?;
        put(text.as_bytes())?;
    }
    for fd in fds {
        put(&fd.fd.to_le_bytes())?;
        put(&[fd.kind as u8, fd.flags, fd.tty, 0])?;
    }
    for e in entries {
        put(&[e.kind as u8, e.handle, 0, 0])?;
        put(&u32::try_from(e.prefix.len()).ok()?.to_le_bytes())?;
        put(e.prefix.as_bytes())?;
    }
    let size = at;
    Bootstrap::decode(&out[..size], handles).map(|_| size)
}

/// The most entries a program's table holds, counting those the shell gives
/// it later (ADR-0337 item 7; the limit of ADR-0107).
pub const MAX_TABLE: usize = 64;
/// The most entries one `GRANTS` message carries.
pub const MAX_GRANTED: usize = 4;
const GRANTS_MAGIC: &[u8; 4] = b"ZGR1";

/// One message on the `GRANTS` channel (ADR-0337 item 7): directory entries
/// to add to the table, each with its connection among the message's
/// handles. An entry for a prefix the table has replaces it (the shell sends
/// the whole entry, with the rights and names it now holds).
///
/// ```text
/// magic "ZGR1", count u32, then count times:
///   kind u8 (0, a directory), handle u8, 2 zero bytes, prefix length u32, prefix
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Grants<'a> {
    entries: &'a [u8],
    count: usize,
}

/// A prefix the table may be given on this channel: not `/`, and not the
/// process's own files and devices, nor the temporary directory.
fn grantable(prefix: &str) -> bool {
    is_absolute_normal(prefix)
        && prefix != "/"
        && !["/dev", "/proc", "/tmp"].iter().any(|p| {
            prefix == *p
                || prefix
                    .strip_prefix(p)
                    .is_some_and(|rest| rest.starts_with('/'))
        })
}

impl<'a> Grants<'a> {
    pub const HEADER_SIZE: usize = 8;

    /// Decodes a message that came with `handles` handles. Any input gives a
    /// result, never a panic: a count above [`MAX_GRANTED`], an entry that is
    /// not a directory, a prefix that is not absolute and normal or that is
    /// `/`, `/dev`, `/proc` or `/tmp` or below one, a handle index used twice
    /// or not in the message, or bytes left over, is `None`.
    pub fn decode(bytes: &'a [u8], handles: usize) -> Option<Grants<'a>> {
        if bytes.get(..4)? != GRANTS_MAGIC {
            return None;
        }
        let count = u32_at(bytes, 4)? as usize;
        if count == 0 || count > MAX_GRANTED {
            return None;
        }
        let mut at = Grants::HEADER_SIZE;
        let mut used = 0u64;
        for _ in 0..count {
            let head = bytes.get(at..at.checked_add(ENTRY_HEADER)?)?;
            let (prefix, end) = string_at(bytes, at + 4)?;
            let index = head[1];
            if head[0] != EntryKind::Directory as u8
                || head[2..4] != [0, 0]
                || !grantable(prefix)
                || usize::from(index) >= handles.min(MAX_MESSAGE_HANDLES)
                || used & (1 << index) != 0
            {
                return None;
            }
            used |= 1 << index;
            at = end;
        }
        (at == bytes.len()).then_some(Grants {
            entries: &bytes[Grants::HEADER_SIZE..],
            count,
        })
    }

    /// Writes a message of `entries` (prefix and the index of its connection
    /// among `handles`), and gives its size; `None` if it does not fit or
    /// `decode` would refuse it.
    pub fn encode(entries: &[(&str, u8)], handles: usize, out: &mut [u8]) -> Option<usize> {
        let mut at = 0usize;
        let mut put = |bytes: &[u8]| -> Option<()> {
            let end = at.checked_add(bytes.len())?;
            out.get_mut(at..end)?.copy_from_slice(bytes);
            at = end;
            Some(())
        };
        put(GRANTS_MAGIC)?;
        put(&u32::try_from(entries.len()).ok()?.to_le_bytes())?;
        for (prefix, handle) in entries {
            put(&[EntryKind::Directory as u8, *handle, 0, 0])?;
            put(&u32::try_from(prefix.len()).ok()?.to_le_bytes())?;
            put(prefix.as_bytes())?;
        }
        let size = at;
        Grants::decode(&out[..size], handles).map(|_| size)
    }

    /// The entries, each as a namespace entry with the index of its handle.
    pub fn entries(&self) -> Entries<'a> {
        Entries {
            bytes: self.entries,
            left: self.count,
        }
    }
}

const NET_REPORT_MAGIC: &[u8; 4] = b"ZPN1";

/// What a refused network call asked for (ADR-0224 item 5).
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetAccess {
    Reach = 0,
    Listen = 1,
}

/// One message on the `REPORT` channel for a network request `netd` answered
/// `NotGranted` (ADR-0224 item 5): the host or address the program named, the
/// port (0 for a lookup), and what it wanted. Like a path report it is the
/// program's own text and is hostile data: the decoder accepts only a host
/// name or dotted address (ASCII letters, digits, `.`, `-`, `_`, at most 253),
/// and the shell filters it again before it shows it.
///
/// ```text
/// magic "ZPN1", access u32, port u32, the target's bytes to the end
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NetReport<'a> {
    pub access: NetAccess,
    pub port: u16,
    pub target: &'a str,
}

/// Whether `text` is a host name or a dotted address: 1 to 253 of ASCII
/// letters, digits, `.`, `-` and `_`.
pub fn is_host_text(text: &str) -> bool {
    !text.is_empty()
        && text.len() <= 253
        && text
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_'))
}

impl<'a> NetReport<'a> {
    pub const HEADER_SIZE: usize = 12;

    pub fn encode(&self, out: &mut [u8]) -> Option<usize> {
        if !is_host_text(self.target) {
            return None;
        }
        let size = NetReport::HEADER_SIZE.checked_add(self.target.len())?;
        let out = out.get_mut(..size)?;
        out[..4].copy_from_slice(NET_REPORT_MAGIC);
        out[4..8].copy_from_slice(&(self.access as u32).to_le_bytes());
        out[8..12].copy_from_slice(&u32::from(self.port).to_le_bytes());
        out[12..].copy_from_slice(self.target.as_bytes());
        Some(size)
    }

    pub fn decode(bytes: &'a [u8]) -> Option<NetReport<'a>> {
        if bytes.get(..4)? != NET_REPORT_MAGIC {
            return None;
        }
        let access = match u32_at(bytes, 4)? {
            0 => NetAccess::Reach,
            1 => NetAccess::Listen,
            _ => return None,
        };
        let port = u16::try_from(u32_at(bytes, 8)?).ok()?;
        let target = core::str::from_utf8(bytes.get(NetReport::HEADER_SIZE..)?).ok()?;
        is_host_text(target).then_some(NetReport {
            access,
            port,
            target,
        })
    }
}

const SYSCALL_REPORT_MAGIC: &[u8; 4] = b"ZPS1";

/// One message on the `REPORT` channel for a `syscall()` number libposix does
/// not answer (ADR-0451 item 1), once per number: the shell shows that the
/// program asked for a call Zoraya has not got. The number is the program's
/// own and is shown as a number only.
///
/// ```text
/// magic "ZPS1", number u32 (the low 32 bits of the call's number)
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SyscallReport {
    pub number: u32,
}

impl SyscallReport {
    pub const SIZE: usize = 8;

    pub fn encode(&self, out: &mut [u8]) -> Option<usize> {
        let out = out.get_mut(..SyscallReport::SIZE)?;
        out[..4].copy_from_slice(SYSCALL_REPORT_MAGIC);
        out[4..8].copy_from_slice(&self.number.to_le_bytes());
        Some(SyscallReport::SIZE)
    }

    pub fn decode(bytes: &[u8]) -> Option<SyscallReport> {
        if bytes.len() != SyscallReport::SIZE || bytes.get(..4)? != SYSCALL_REPORT_MAGIC {
            return None;
        }
        Some(SyscallReport {
            number: u32_at(bytes, 4)?,
        })
    }
}

/// The access a refused path was named for (ADR-0153 rule 11).
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Access {
    Read = 0,
    Write = 1,
}

/// The most reports a process sends (ADR-0153 rule 11).
pub const MAX_REPORTS: u32 = 16;
const REPORT_MAGIC: &[u8; 4] = b"ZPR1";

/// One message on the `REPORT` channel: a path no entry covers, absolute and
/// normal, with the reports dropped before it.
///
/// ```text
/// magic "ZPR1", access u32, dropped u32, the path's bytes to the end
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Report<'a> {
    pub access: Access,
    /// Reports that did not fit before this one (ADR-0153 rule 11).
    pub dropped: u32,
    pub path: &'a str,
}

impl<'a> Report<'a> {
    pub const HEADER_SIZE: usize = 12;

    pub fn encode(&self, out: &mut [u8]) -> Option<usize> {
        let size = Report::HEADER_SIZE.checked_add(self.path.len())?;
        if !is_absolute_normal(self.path) || self.path.len() > MAX_PATH {
            return None;
        }
        let out = out.get_mut(..size)?;
        out[..4].copy_from_slice(REPORT_MAGIC);
        out[4..8].copy_from_slice(&(self.access as u32).to_le_bytes());
        out[8..12].copy_from_slice(&self.dropped.to_le_bytes());
        out[12..].copy_from_slice(self.path.as_bytes());
        Some(size)
    }

    pub fn decode(bytes: &'a [u8]) -> Option<Report<'a>> {
        if bytes.get(..4)? != REPORT_MAGIC {
            return None;
        }
        let access = match u32_at(bytes, 4)? {
            0 => Access::Read,
            1 => Access::Write,
            _ => return None,
        };
        let dropped = u32_at(bytes, 8)?;
        let path = core::str::from_utf8(bytes.get(Report::HEADER_SIZE..)?).ok()?;
        if !is_absolute_normal(path) || path.len() > MAX_PATH {
            return None;
        }
        Some(Report {
            access,
            dropped,
            path,
        })
    }
}

/// True if `handle` is one of the fixed slots.
pub fn is_fixed_slot(handle: Handle) -> bool {
    slot::ALL.contains(&handle)
}
