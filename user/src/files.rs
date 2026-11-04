//! A client of the file protocol (ADR-0055), for the shell and tests.

use alloc::vec;
use alloc::vec::Vec;

use zoraya_abi::spawn::Transfer;
use zoraya_abi::{
    ChannelEntry, ChannelObject, DEADLINE_INFINITE, EventObject, Handle, MemoryObject, Mode,
    ObjectType, Packet, Rights, channel_signals,
};
use zoraya_proto_file::{self as proto, open_flags, rights, status};
use zoraya_protocol::{Below, Data, ErrorReply, Flags, Message, Name, Path, decode, encode};
use zoraya_rt::{
    channel_create, channel_read, channel_write, handle_close, port_create, port_subscribe,
    port_wait,
};

/// A protocol status, or a call that failed below it.
pub type FsResult<T> = Result<T, u32>;

/// What the protocol has no status for: the connection is gone.
pub const GONE: u32 = 1000;

/// The name of a status, for people.
pub fn status_name(s: u32) -> &'static str {
    match s {
        status::OK => "Ok",
        status::NOT_FOUND => "NotFound",
        status::EXISTS => "Exists",
        status::NO_SPACE => "NoSpace",
        status::NOT_EMPTY => "NotEmpty",
        status::INVALID_NAME => "InvalidName",
        status::NOT_DIRECTORY => "NotDirectory",
        status::IS_DIRECTORY => "IsDirectory",
        status::IS_LINK => "IsLink",
        status::INVALID_ARGUMENT => "InvalidArgument",
        status::CORRUPT => "Corrupt",
        status::STORAGE_FAILED => "StorageFailed",
        status::ACCESS_DENIED => "AccessDenied",
        status::TOO_MANY_OPEN => "TooManyOpen",
        status::LIMIT_REACHED => "LimitReached",
        status::UNSUPPORTED => "Unsupported",
        GONE => "VolumeGone",
        _ => "Unknown",
    }
}

fn moved(handle: Handle, rights: Rights) -> ChannelEntry {
    ChannelEntry {
        handle,
        rights,
        transfer: Transfer::Move,
    }
}

/// Sends `m` and waits for the reply, which `reply` holds.
fn call<'a, M: Message<'a>>(
    end: Handle,
    m: &M,
    handles: &[ChannelEntry],
    reply: &mut Vec<u8>,
) -> FsResult<(usize, Option<Handle>)> {
    reply.resize(64 * 1024, 0);
    // Handles to move are this call's until they are sent: a request that
    // never goes must not leave them in the table.
    let unsent = |s: u32| {
        for entry in handles.iter().filter(|e| e.transfer == Transfer::Move) {
            let _ = handle_close(entry.handle);
        }
        s
    };
    let length = encode(m, 1, reply).map_err(|_| unsent(status::INVALID_ARGUMENT))?;
    channel_write(end, &reply[..length], handles).map_err(|_| unsent(GONE))?;
    let port = port_create().map_err(|_| GONE)?;
    let wanted = channel_signals::READABLE | channel_signals::PEER_CLOSED;
    let waited = port_subscribe(port, end, wanted, 1, Mode::Level)
        .and_then(|()| {
            let mut packets = [Packet::default()];
            port_wait(port, DEADLINE_INFINITE, &mut packets)
        })
        .map(|_| ());
    let _ = handle_close(port);
    waited.map_err(|_| GONE)?;
    let mut got = [end; 1];
    match channel_read(end, reply, &mut got) {
        Ok(sizes) => Ok((sizes.bytes as usize, (sizes.handles == 1).then_some(got[0]))),
        Err(_) => Err(GONE),
    }
}

fn check(s: u32) -> FsResult<()> {
    if s == status::OK { Ok(()) } else { Err(s) }
}

/// A reply of a `Done` request. A refused message gets the generic reply.
fn done(reply: &[u8]) -> FsResult<u64> {
    if let Ok((_, m)) = decode::<proto::Done>(reply, 0) {
        check(m.status.0)?;
        return Ok(m.count);
    }
    Err(decode::<ErrorReply>(reply, 0).map_or(status::INVALID_ARGUMENT, |(_, m)| m.status))
}

/// What `stat` gives.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stat {
    pub size: u64,
    pub modified: u64,
    pub changed: u64,
    pub mode: u32,
    pub kind: u32,
}

/// A connection to an open file or directory: closed with its handle.
pub struct File {
    pub end: Handle,
}

impl Drop for File {
    fn drop(&mut self) {
        let _ = handle_close(self.end);
    }
}

impl File {
    pub fn open(&self, path: &str, flags: u32, wanted: u32) -> FsResult<File> {
        let (client, server) = channel_create().map_err(|_| GONE)?;
        // Closed on every early return.
        let client = File { end: client };
        let m = proto::Open {
            flags: Flags(flags),
            rights: Flags(wanted),
            path: Path(path),
        };
        let mut reply = Vec::new();
        let (length, _) = call(
            self.end,
            &m,
            &[moved(server, ChannelObject::RIGHTS)],
            &mut reply,
        )?;
        match decode::<proto::OpenReply<'_>>(&reply[..length], 0) {
            Ok((_, r)) => check(r.status.0).map(|()| client),
            Err(_) => Err(done(&reply[..length])
                .err()
                .unwrap_or(status::INVALID_ARGUMENT)),
        }
    }

    /// `open`, following symbolic links as the resolver does (ADR-0013 rule
    /// 8): a relative target from the link's directory, an absolute one from
    /// this directory, at most 40 links.
    pub fn open_followed(&self, path: &str, flags: u32, wanted: u32) -> FsResult<File> {
        let mut path = alloc::string::String::from(path.trim_start_matches('/'));
        for _ in 0..40 {
            let (client, server) = channel_create().map_err(|_| GONE)?;
            let client = File { end: client };
            let m = proto::Open {
                flags: Flags(flags),
                rights: Flags(wanted),
                path: Path(&path),
            };
            let mut reply = Vec::new();
            let (length, _) = call(
                self.end,
                &m,
                &[moved(server, ChannelObject::RIGHTS)],
                &mut reply,
            )?;
            let Ok((_, r)) = decode::<proto::OpenReply<'_>>(&reply[..length], 0) else {
                return Err(done(&reply[..length])
                    .err()
                    .unwrap_or(status::INVALID_ARGUMENT));
            };
            if r.status.0 != status::IS_LINK {
                return check(r.status.0).map(|()| client);
            }
            let remaining = r.remaining.0;
            let consumed = &path[..path.len() - remaining.len()];
            let consumed = consumed.trim_end_matches('/');
            let dir = consumed.rsplit_once('/').map_or("", |(d, _)| d);
            let target = r.target.0;
            let mut next = if let Some(absolute) = target.strip_prefix('/') {
                alloc::string::String::from(absolute)
            } else if dir.is_empty() {
                alloc::string::String::from(target)
            } else {
                alloc::format!("{dir}/{target}")
            };
            if !remaining.is_empty() {
                next.push('/');
                next.push_str(remaining.trim_start_matches('/'));
            }
            path = next;
        }
        Err(status::IS_LINK)
    }

    /// The boot's time base (ADR-0191), from the volume's service.
    pub fn time_base(&self) -> FsResult<u64> {
        let mut reply = Vec::new();
        let (length, _) = call(self.end, &proto::TimeBase {}, &[], &mut reply)?;
        done(&reply[..length])
    }

    pub fn clone_as(&self, wanted: u32) -> FsResult<File> {
        let (client, server) = channel_create().map_err(|_| GONE)?;
        let client = File { end: client };
        let m = proto::Clone {
            rights: Flags(wanted),
        };
        let mut reply = Vec::new();
        let (length, _) = call(
            self.end,
            &m,
            &[moved(server, ChannelObject::RIGHTS)],
            &mut reply,
        )?;
        done(&reply[..length]).map(|_| client)
    }

    /// A second connection to this one's open description (ADR-0103): one
    /// position for both.
    pub fn share(&self, wanted: u32) -> FsResult<File> {
        let (client, server) = channel_create().map_err(|_| GONE)?;
        let client = File { end: client };
        let m = proto::Share {
            rights: Flags(wanted),
        };
        let mut reply = Vec::new();
        let (length, _) = call(
            self.end,
            &m,
            &[moved(server, ChannelObject::RIGHTS)],
            &mut reply,
        )?;
        done(&reply[..length]).map(|_| client)
    }

    /// Moves the description's position, and gives the new one.
    pub fn seek(&self, whence: u32, offset: i64) -> FsResult<u64> {
        let m = proto::Seek {
            whence: Below(whence),
            offset: offset as u64,
        };
        let mut reply = Vec::new();
        let (length, _) = call(self.end, &m, &[], &mut reply)?;
        done(&reply[..length])
    }

    pub fn read(&self, offset: u64, out: &mut [u8]) -> FsResult<usize> {
        let m = proto::Read {
            offset,
            length: out.len().min(proto::MAX_INLINE) as u32,
        };
        let mut reply = Vec::new();
        let (length, _) = call(self.end, &m, &[], &mut reply)?;
        let (_, r) = decode::<proto::ReadReply<'_>>(&reply[..length], 0).map_err(|_| {
            done(&reply[..length])
                .err()
                .unwrap_or(status::INVALID_ARGUMENT)
        })?;
        check(r.status.0)?;
        // More than was asked is the server's error, not a reason to panic.
        let into = out
            .get_mut(..r.data.0.len())
            .ok_or(status::INVALID_ARGUMENT)?;
        into.copy_from_slice(r.data.0);
        Ok(r.data.0.len())
    }

    pub fn write(&self, offset: u64, flags: u32, data: &[u8]) -> FsResult<u64> {
        let m = proto::Write {
            offset,
            flags: Flags(flags),
            data: Data(data),
        };
        let mut reply = Vec::new();
        let (length, _) = call(self.end, &m, &[], &mut reply)?;
        done(&reply[..length])
    }

    pub fn attach(&self, memory: Handle, size: u64) -> FsResult<u64> {
        let mut reply = Vec::new();
        let entry = ChannelEntry {
            handle: memory,
            rights: MemoryObject::RIGHTS,
            transfer: Transfer::Duplicate,
        };
        let (length, _) = call(self.end, &proto::Attach { size }, &[entry], &mut reply)?;
        done(&reply[..length])
    }

    pub fn write_buffer(&self, offset: u64, buffer_offset: u64, length: u32) -> FsResult<u64> {
        let m = proto::WriteBuffer {
            offset,
            buffer_offset,
            length,
            flags: Flags(0),
        };
        let mut reply = Vec::new();
        let (size, _) = call(self.end, &m, &[], &mut reply)?;
        done(&reply[..size])
    }

    pub fn read_buffer(&self, offset: u64, buffer_offset: u64, length: u32) -> FsResult<u64> {
        let m = proto::ReadBuffer {
            offset,
            buffer_offset,
            length,
        };
        let mut reply = Vec::new();
        let (size, _) = call(self.end, &m, &[], &mut reply)?;
        done(&reply[..size])
    }

    pub fn stat(&self) -> FsResult<Stat> {
        let mut reply = Vec::new();
        let (length, _) = call(self.end, &proto::GetAttributes {}, &[], &mut reply)?;
        let (_, a) = decode::<proto::Attributes>(&reply[..length], 0)
            .map_err(|_| status::INVALID_ARGUMENT)?;
        check(a.status.0)?;
        Ok(Stat {
            size: a.size,
            modified: a.modified,
            changed: a.changed,
            mode: a.mode,
            kind: a.kind.0,
        })
    }

    /// Calls `f` with each name and kind.
    pub fn list(&self, f: &mut dyn FnMut(&str, u32)) -> FsResult<()> {
        let mut cookie = 0;
        let mut reply = Vec::new();
        loop {
            let (length, _) = call(self.end, &proto::ReadDirectory { cookie }, &[], &mut reply)?;
            let (_, r) =
                decode::<proto::ReadDirectoryReply<'_>>(&reply[..length], 0).map_err(|_| {
                    done(&reply[..length])
                        .err()
                        .unwrap_or(status::INVALID_ARGUMENT)
                })?;
            check(r.status.0)?;
            cookie = 0;
            for entry in r.entries.iter() {
                f(entry.name.0, entry.kind.0);
                cookie = entry.next;
            }
            if cookie == 0 {
                return Ok(());
            }
        }
    }

    pub fn make_directory(&self, name: &str) -> FsResult<()> {
        let m = proto::MakeDirectory {
            mode: 0o755,
            name: Name(name),
        };
        let mut reply = Vec::new();
        let (length, _) = call(self.end, &m, &[], &mut reply)?;
        done(&reply[..length]).map(|_| ())
    }

    pub fn remove(&self, name: &str) -> FsResult<()> {
        let mut reply = Vec::new();
        let (length, _) = call(
            self.end,
            &proto::Remove { name: Name(name) },
            &[],
            &mut reply,
        )?;
        done(&reply[..length]).map(|_| ())
    }

    pub fn token(&self) -> FsResult<Handle> {
        let mut reply = Vec::new();
        let (length, handle) = call(self.end, &proto::GetToken {}, &[], &mut reply)?;
        decode::<proto::TokenReply>(&reply[..length], usize::from(handle.is_some())).map_err(
            |_| {
                done(&reply[..length])
                    .err()
                    .unwrap_or(status::INVALID_ARGUMENT)
            },
        )?;
        handle.ok_or(status::INVALID_ARGUMENT)
    }

    /// Renames `old` here to `new` in the directory of `target`.
    pub fn rename(&self, old: &str, target: &File, new: &str) -> FsResult<()> {
        let token = target.token()?;
        let m = proto::Rename {
            mode: Below(proto::rename_mode::REPLACE),
            old: Name(old),
            new: Name(new),
        };
        let mut reply = Vec::new();
        let entry = moved(token, EventObject::RIGHTS);
        let (length, _) = call(self.end, &m, &[entry], &mut reply)?;
        done(&reply[..length]).map(|_| ())
    }

    pub fn sync(&self) -> FsResult<()> {
        let mut reply = Vec::new();
        let (length, _) = call(self.end, &proto::Sync {}, &[], &mut reply)?;
        done(&reply[..length]).map(|_| ())
    }
}

/// Reads a whole file, by inline requests.
/// The most `read_all` reads: a larger file is `LimitReached`, not a heap
/// that grows until the program fails.
pub const READ_ALL_LIMIT: usize = 16 << 20;

pub fn read_all(file: &File) -> FsResult<Vec<u8>> {
    let mut data = Vec::new();
    let mut chunk = vec![0; proto::MAX_INLINE];
    loop {
        let got = file.read(data.len() as u64, &mut chunk)?;
        if got == 0 {
            return Ok(data);
        }
        if data.len() + got > READ_ALL_LIMIT {
            return Err(status::LIMIT_REACHED);
        }
        data.extend_from_slice(&chunk[..got]);
    }
}

/// `dir/name` split at its last `/`.
pub fn split_path(path: &str) -> (&str, &str) {
    match path.rsplit_once('/') {
        Some((dir, name)) => (dir, name),
        None => ("", path),
    }
}

/// What every shell connection may do.
pub const ALL: u32 = rights::ALL;
/// The flags of a directory open.
pub const DIRECTORY: u32 = open_flags::DIRECTORY;

/// The crash test's file `i`: its length and contents depend on `i`.
pub fn bytes_of(i: u64) -> Vec<u8> {
    let length = 1 + (i as usize * 997) % (32 * 1024);
    let mut data = vec![0; length];
    for (at, byte) in data.iter_mut().enumerate() {
        *byte = (i as usize + at * 7) as u8;
    }
    data
}
