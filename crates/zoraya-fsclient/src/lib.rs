//! A client of the file protocol (ADR-0055) for the GUI's programs that
//! browse and change the volume: the chooser and the file manager (plan
//! 7e). It speaks over any [`Link`]: the kernel's channels in a program,
//! the service itself in a host test. It never widens what a connection
//! allows: every connection it makes asks for rights the caller names, and
//! the service grants at most the parent's (ADR-0055, INV-0013-5).

#![no_std]
#![forbid(unsafe_code)]

extern crate alloc;

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use zoraya_proto_file::{self as proto, kind, open_flags, rename_mode, status};
use zoraya_protocol::{Below, Data, Flags, Message, Name, Path, decode, encode};

/// The largest message either way.
const BUFFER: usize = zoraya_protocol::MAX_WIRE_MESSAGE;
/// The longest file `read_all` reads.
pub const MAX_READ: u64 = 16 << 20;

/// What the client needs of a channel.
pub trait Link {
    type Handle: Copy + PartialEq;

    /// Sends `request` with `handles` (moved) on `on` and waits for the
    /// reply: its length in `reply`, and the handles it carried in `got`.
    /// `None` when the channel is gone.
    fn call(
        &mut self,
        on: Self::Handle,
        request: &[u8],
        handles: &[Self::Handle],
        reply: &mut [u8],
        got: &mut Vec<Self::Handle>,
    ) -> Option<usize>;

    /// A new channel: the end to keep and the end to send.
    fn channel(&mut self) -> Option<(Self::Handle, Self::Handle)>;

    fn close(&mut self, h: Self::Handle);
}

/// A failed request's status (`zoraya-proto-file`'s `status`); `GONE` when
/// the channel itself failed.
pub type Status = u32;
pub const GONE: Status = u32::MAX;

/// One name in a directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Listed {
    pub name: String,
    /// `zoraya-proto-file`'s `kind`.
    pub kind: u32,
}

impl Listed {
    pub fn is_dir(&self) -> bool {
        self.kind == kind::DIRECTORY
    }
}

pub struct Client<L: Link> {
    pub link: L,
    transaction: u32,
    out: Vec<u8>,
    reply: Vec<u8>,
    got: Vec<L::Handle>,
}

impl<L: Link> Client<L> {
    pub fn new(link: L) -> Client<L> {
        Client {
            link,
            transaction: 1,
            out: vec![0; BUFFER],
            reply: vec![0; BUFFER],
            got: Vec::new(),
        }
    }

    fn call<'a, M: Message<'a>>(
        &mut self,
        on: L::Handle,
        m: &M,
        handles: &[L::Handle],
    ) -> Result<usize, Status> {
        let t = self.transaction;
        self.transaction = self.transaction.wrapping_add(1).max(1);
        let n = match encode(m, t, &mut self.out) {
            Ok(n) => n,
            Err(_) => {
                for h in handles {
                    self.link.close(*h);
                }
                return Err(status::INVALID_ARGUMENT);
            }
        };
        self.got.clear();
        let length = self
            .link
            .call(on, &self.out[..n], handles, &mut self.reply, &mut self.got)
            .ok_or(GONE)?;
        // A reply carries handles only where this client asked for them.
        if !self.got.is_empty() && M::ORDINAL != proto::GetToken::ORDINAL {
            for h in self.got.drain(..) {
                self.link.close(h);
            }
        }
        Ok(length)
    }

    /// The status of a `Done` (or of the error reply the service sends for
    /// a request it refuses whole).
    fn done(&self, length: usize) -> Result<u64, Status> {
        let bytes = &self.reply[..length];
        if let Ok((_, d)) = decode::<proto::Done>(bytes, 0) {
            return if d.status.0 == status::OK {
                Ok(d.count)
            } else {
                Err(d.status.0)
            };
        }
        match decode::<zoraya_protocol::ErrorReply>(bytes, 0) {
            Ok((_, e)) => Err(e.status),
            Err(_) => Err(status::INVALID_ARGUMENT),
        }
    }

    /// A request that gives a new connection in a channel it sends.
    fn connect<'a, M: Message<'a>>(&mut self, on: L::Handle, m: &M) -> Result<L::Handle, Status> {
        let (mine, theirs) = self.link.channel().ok_or(GONE)?;
        let length = match self.call(on, m, &[theirs]) {
            Ok(n) => n,
            Err(e) => {
                self.link.close(mine);
                return Err(e);
            }
        };
        let ok = if M::ORDINAL == proto::Open::ORDINAL {
            match decode::<proto::OpenReply<'_>>(&self.reply[..length], 0) {
                Ok((_, r)) if r.status.0 == status::OK => Ok(()),
                Ok((_, r)) => Err(r.status.0),
                Err(_) => self.done(length).map(|_| ()),
            }
        } else {
            self.done(length).map(|_| ())
        };
        match ok {
            Ok(()) => Ok(mine),
            Err(e) => {
                self.link.close(mine);
                Err(e)
            }
        }
    }

    /// Opens `path` (relative to `dir`, `/`-separated, no `.` or `..`)
    /// with `rights`; `flags` are `open_flags`.
    pub fn open(
        &mut self,
        dir: L::Handle,
        path: &str,
        flags: u32,
        rights: u32,
    ) -> Result<L::Handle, Status> {
        let path = path.trim_start_matches('/');
        if path.is_empty() {
            return self.clone_with(dir, rights);
        }
        let m = proto::Open {
            flags: Flags(flags & open_flags::ALL),
            rights: Flags(rights),
            path: Path(path),
        };
        self.connect(dir, &m)
    }

    /// A new connection to the same object with `rights` (at most the
    /// connection's own).
    pub fn clone_with(&mut self, conn: L::Handle, rights: u32) -> Result<L::Handle, Status> {
        let m = proto::Clone {
            rights: Flags(rights),
        };
        self.connect(conn, &m)
    }

    /// A connection to `dir` through which only `name` can be used, with
    /// `rights` (ADR-0153 rule 2).
    pub fn narrow(&mut self, dir: L::Handle, name: &str, rights: u32) -> Result<L::Handle, Status> {
        if !Name::is_valid(name) {
            return Err(status::INVALID_NAME);
        }
        let m = proto::Narrow {
            rights: Flags(rights),
            name: Name(name),
        };
        self.connect(dir, &m)
    }

    /// Every name in the directory, in the service's order.
    pub fn list(&mut self, dir: L::Handle) -> Result<Vec<Listed>, Status> {
        let mut out = Vec::new();
        let mut cookie = 0;
        loop {
            let length = self.call(dir, &proto::ReadDirectory { cookie }, &[])?;
            let (_, r) = match decode::<proto::ReadDirectoryReply<'_>>(&self.reply[..length], 0) {
                Ok(r) => r,
                Err(_) => return Err(self.done(length).err().unwrap_or(status::INVALID_ARGUMENT)),
            };
            if r.status.0 != status::OK {
                return Err(r.status.0);
            }
            let mut last = None;
            for e in r.entries.iter() {
                out.push(Listed {
                    name: String::from(e.name.0),
                    kind: e.kind.0,
                });
                last = Some(e.next);
            }
            match last {
                Some(next) if next != 0 && next != cookie => cookie = next,
                _ => return Ok(out),
            }
        }
    }

    /// The object's size in bytes and its kind.
    /// The volume's free bytes (ADR-0248).
    pub fn free_space(&mut self, conn: L::Handle) -> Result<u64, Status> {
        let length = self.call(conn, &proto::FreeSpace {}, &[])?;
        self.done(length)
    }

    pub fn attributes(&mut self, conn: L::Handle) -> Result<(u64, u32), Status> {
        let length = self.call(conn, &proto::GetAttributes {}, &[])?;
        match decode::<proto::Attributes>(&self.reply[..length], 0) {
            Ok((_, a)) if a.status.0 == status::OK => Ok((a.size, a.kind.0)),
            Ok((_, a)) => Err(a.status.0),
            Err(_) => Err(self.done(length).err().unwrap_or(status::INVALID_ARGUMENT)),
        }
    }

    /// A file's size, kind and modification time (nanoseconds since the
    /// epoch, as the service keeps it).
    pub fn info(&mut self, conn: L::Handle) -> Result<(u64, u32, u64), Status> {
        let length = self.call(conn, &proto::GetAttributes {}, &[])?;
        match decode::<proto::Attributes>(&self.reply[..length], 0) {
            Ok((_, a)) if a.status.0 == status::OK => Ok((a.size, a.kind.0, a.modified)),
            Ok((_, a)) => Err(a.status.0),
            Err(_) => Err(self.done(length).err().unwrap_or(status::INVALID_ARGUMENT)),
        }
    }

    /// The boot's time base (ADR-0191): nanoseconds the service adds to the
    /// monotonic clock for the times it assigns.
    pub fn time_base(&mut self, conn: L::Handle) -> Result<u64, Status> {
        let length = self.call(conn, &proto::TimeBase {}, &[])?;
        self.done(length)
    }

    /// The whole file, up to [`MAX_READ`] bytes.
    pub fn read_all(&mut self, file: L::Handle) -> Result<Vec<u8>, Status> {
        let mut out = Vec::new();
        loop {
            let m = proto::Read {
                offset: out.len() as u64,
                length: proto::MAX_INLINE as u32,
            };
            let length = self.call(file, &m, &[])?;
            let (_, r) = match decode::<proto::ReadReply<'_>>(&self.reply[..length], 0) {
                Ok(r) => r,
                Err(_) => return Err(self.done(length).err().unwrap_or(status::INVALID_ARGUMENT)),
            };
            if r.status.0 != status::OK {
                return Err(r.status.0);
            }
            if r.data.0.is_empty() {
                return Ok(out);
            }
            out.extend_from_slice(r.data.0);
            if out.len() as u64 > MAX_READ {
                return Err(status::LIMIT_REACHED);
            }
        }
    }

    /// Replaces the file's contents with `bytes`.
    pub fn write_all(&mut self, file: L::Handle, bytes: &[u8]) -> Result<(), Status> {
        let mut at = 0;
        while at < bytes.len() || at == 0 {
            let end = (at + proto::MAX_INLINE).min(bytes.len());
            let m = proto::Write {
                offset: at as u64,
                flags: Flags(0),
                data: Data(&bytes[at..end]),
            };
            let length = self.call(file, &m, &[])?;
            let wrote = self.done(length)? as usize;
            if wrote == 0 && end > at {
                return Err(status::NO_SPACE);
            }
            at += wrote;
            if bytes.is_empty() {
                break;
            }
        }
        let length = self.call(
            file,
            &proto::SetSize {
                size: bytes.len() as u64,
            },
            &[],
        )?;
        self.done(length).map(|_| ())
    }

    /// Makes the directory `name` in `dir`.
    pub fn make_directory(&mut self, dir: L::Handle, name: &str) -> Result<(), Status> {
        if !Name::is_valid(name) {
            return Err(status::INVALID_NAME);
        }
        let m = proto::MakeDirectory {
            mode: 0o755,
            name: Name(name),
        };
        let length = self.call(dir, &m, &[])?;
        self.done(length).map(|_| ())
    }

    /// Removes `name` from `dir` (a directory only when empty).
    pub fn remove(&mut self, dir: L::Handle, name: &str) -> Result<(), Status> {
        if !Name::is_valid(name) {
            return Err(status::INVALID_NAME);
        }
        let length = self.call(dir, &proto::Remove { name: Name(name) }, &[])?;
        self.done(length).map(|_| ())
    }

    /// Renames `old` to `new` in `dir`, refusing to replace `new`.
    pub fn rename(&mut self, dir: L::Handle, old: &str, new: &str) -> Result<(), Status> {
        self.rename_into(dir, old, dir, new)
    }

    /// Moves `old` of `dir` to `new` in the directory `target` (the same
    /// volume), refusing to replace `new`.
    pub fn rename_into(
        &mut self,
        dir: L::Handle,
        old: &str,
        target: L::Handle,
        new: &str,
    ) -> Result<(), Status> {
        if !Name::is_valid(old) || !Name::is_valid(new) {
            return Err(status::INVALID_NAME);
        }
        let length = self.call(target, &proto::GetToken {}, &[])?;
        let token = match decode::<proto::TokenReply>(&self.reply[..length], self.got.len()) {
            Ok((_, r)) if r.status.0 == status::OK && self.got.len() == 1 => self.got[0],
            Ok((_, r)) => {
                for h in self.got.drain(..) {
                    self.link.close(h);
                }
                return Err(if r.status.0 == status::OK {
                    status::INVALID_ARGUMENT
                } else {
                    r.status.0
                });
            }
            Err(_) => {
                for h in self.got.drain(..) {
                    self.link.close(h);
                }
                return Err(self.done(length).err().unwrap_or(status::INVALID_ARGUMENT));
            }
        };
        self.got.clear();
        let m = proto::Rename {
            mode: Below(rename_mode::NO_REPLACE),
            old: Name(old),
            new: Name(new),
        };
        let length = self.call(dir, &m, &[token])?;
        self.done(length).map(|_| ())
    }

    pub fn close(&mut self, h: L::Handle) {
        self.link.close(h);
    }
}

// A runaway test fails at 512 MiB instead of taking the host (docs/backlog.md).
#[cfg(test)]
zoraya_test_alloc::cap_memory!();
