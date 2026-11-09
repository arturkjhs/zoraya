//! The kernel under `zoraya-app` and `zoraya-fsclient` (stage 7e): the
//! compositor's channel and pixel buffers for an application, and file
//! connections for the programs that browse the volume.

use alloc::vec::Vec;

use zoraya_abi::spawn::Transfer;
use zoraya_abi::{
    ChannelEntry, ChannelObject, DEADLINE_INFINITE, EventObject, Handle, MapFlags, MemoryFlags,
    Mode, ObjectType, Packet, Rights, Status, channel_signals,
};
use zoraya_rt::{
    channel_create, channel_read, channel_write, handle_close, handle_duplicate, handle_type,
    memory_create, memory_map, memory_unmap, port_create, port_subscribe, port_wait,
};

/// Waits until `end` is readable or closed.
pub fn wait(end: Handle) -> bool {
    let Ok(port) = port_create() else {
        return false;
    };
    let wanted = channel_signals::READABLE | channel_signals::PEER_CLOSED;
    let ok = port_subscribe(port, end, wanted, 1, Mode::Level)
        .and_then(|()| {
            let mut packets = [Packet::default()];
            port_wait(port, DEADLINE_INFINITE, &mut packets)
        })
        .is_ok();
    let _ = handle_close(port);
    ok
}

struct Mapped {
    address: u64,
    size: u64,
    pixels: usize,
}

/// An application's compositor channel, for [`zoraya_app::App`].
pub struct Gui {
    pub end: Handle,
    buffers: Vec<Option<Mapped>>,
    /// The handle the last message carried (a drop's entry), until taken.
    last: Option<Handle>,
}

impl Gui {
    pub fn new(end: Handle) -> Gui {
        Gui {
            end,
            buffers: Vec::new(),
            last: None,
        }
    }

    fn read(&mut self, bytes: &mut [u8]) -> Result<usize, Status> {
        let mut handles = [self.end; 1];
        let sizes = channel_read(self.end, bytes, &mut handles).map_err(|(s, _)| s)?;
        // A handle not taken with the message before is closed now.
        if let Some(h) = self.last.take() {
            let _ = handle_close(h);
        }
        self.last = handles[..sizes.handles as usize].first().copied();
        Ok(sizes.bytes as usize)
    }
}

impl zoraya_app::Platform for Gui {
    type End = Handle;

    fn handle(&mut self) -> Option<Handle> {
        self.last.take()
    }

    fn send(&mut self, bytes: &[u8], handle: Option<Handle>) -> bool {
        // A buffer's view goes read-only; a dragged entry is a channel.
        let entry = handle.map(|h| ChannelEntry {
            handle: h,
            rights: if handle_type(h) == Ok("channel") {
                ChannelObject::RIGHTS
            } else {
                Rights::READ
            },
            transfer: Transfer::Move,
        });
        let entries: &[ChannelEntry] = match &entry {
            Some(e) => core::slice::from_ref(e),
            None => &[],
        };
        channel_write(self.end, bytes, entries).is_ok()
    }

    fn receive(&mut self, bytes: &mut [u8]) -> Option<usize> {
        loop {
            match self.read(bytes) {
                Ok(n) => return Some(n),
                Err(Status::WouldBlock) => {
                    if !wait(self.end) {
                        return None;
                    }
                }
                Err(_) => return None,
            }
        }
    }

    fn poll(&mut self, bytes: &mut [u8]) -> zoraya_app::Poll {
        match self.read(bytes) {
            Ok(n) => zoraya_app::Poll::Message(n),
            Err(Status::WouldBlock) => zoraya_app::Poll::Empty,
            Err(_) => zoraya_app::Poll::Gone,
        }
    }

    fn buffer(&mut self, pixels: usize) -> Option<(Handle, usize)> {
        let size = (pixels * 4).next_multiple_of(4096) as u64;
        let memory = memory_create(size, MemoryFlags::NONE).ok()?;
        let Ok(address) = memory_map(memory, 0, size, MapFlags::WRITE) else {
            let _ = handle_close(memory);
            return None;
        };
        let view = handle_duplicate(memory, Rights::READ.union(Rights::TRANSFER));
        let _ = handle_close(memory);
        let Ok(view) = view else {
            let _ = memory_unmap(address, size);
            return None;
        };
        let mapped = Mapped {
            address,
            size,
            pixels,
        };
        let i = match self.buffers.iter().position(Option::is_none) {
            Some(i) => {
                self.buffers[i] = Some(mapped);
                i
            }
            None => {
                self.buffers.push(Some(mapped));
                self.buffers.len() - 1
            }
        };
        Some((view, i))
    }

    fn pixels(&mut self, buffer: usize) -> &mut [u32] {
        match self.buffers.get(buffer).and_then(Option::as_ref) {
            // SAFETY: a writable mapping of `size` bytes made by `buffer`,
            // page-aligned and holding `pixels` u32 values, kept until
            // `free` unmaps it; the slice lives no longer than `&mut self`.
            Some(m) => unsafe { core::slice::from_raw_parts_mut(m.address as *mut u32, m.pixels) },
            None => &mut [],
        }
    }

    fn free(&mut self, buffer: usize) {
        if let Some(m) = self.buffers.get_mut(buffer).and_then(Option::take) {
            let _ = memory_unmap(m.address, m.size);
        }
    }
}

/// File connections over the kernel's channels, for
/// [`zoraya_fsclient::Client`].
pub struct Files;

impl zoraya_fsclient::Link for Files {
    type Handle = Handle;

    fn call(
        &mut self,
        on: Handle,
        request: &[u8],
        handles: &[Handle],
        reply: &mut [u8],
        got: &mut Vec<Handle>,
    ) -> Option<usize> {
        let entries: Vec<ChannelEntry> = handles
            .iter()
            .map(|h| ChannelEntry {
                handle: *h,
                rights: if handle_type(*h) == Ok("event") {
                    EventObject::RIGHTS
                } else {
                    ChannelObject::RIGHTS
                },
                transfer: Transfer::Move,
            })
            .collect();
        if channel_write(on, request, &entries).is_err() {
            for h in handles {
                let _ = handle_close(*h);
            }
            return None;
        }
        if !wait(on) {
            return None;
        }
        let mut list = [on; 1];
        let sizes = channel_read(on, reply, &mut list).ok()?;
        got.extend_from_slice(&list[..sizes.handles as usize]);
        Some(sizes.bytes as usize)
    }

    fn channel(&mut self) -> Option<(Handle, Handle)> {
        channel_create().ok()
    }

    fn close(&mut self, h: Handle) {
        let _ = handle_close(h);
    }
}

/// The shell's description, for `shell.home`'s default and type.
const ZOSH: &str = include_str!("../describe/system/zosh.cmd");

/// `shell.home`, as the shell reads it from `/config/settings` through
/// `root`.
pub fn home(fs: &mut zoraya_fsclient::Client<Files>, root: Handle) -> alloc::string::String {
    let declared = zoraya_describe::parse(ZOSH.as_bytes())
        .map(|d| d.settings)
        .unwrap_or_default();
    let bytes = fs
        .open(root, "config/settings", 0, zoraya_proto_file::rights::READ)
        .ok()
        .and_then(|f| {
            let b = fs.read_all(f).ok();
            fs.close(f);
            b
        })
        .unwrap_or_default();
    let v = zoraya_settings::read(&bytes, &declared);
    alloc::string::String::from(v.get("shell.home").unwrap_or("/home"))
}

/// An application's side of its chooser channel (ADR-0167): one request
/// at a time, and its answer read when the channel is readable.
pub mod chooser {
    use alloc::string::String;
    use alloc::vec;

    use zoraya_abi::{Handle, Status};
    use zoraya_proto_chooser as pc;
    use zoraya_protocol::{Data, Header, decode, encode};
    use zoraya_rt::{channel_read, channel_write, handle_close};

    /// Asks to open a file (`suggested` `None`) or to save one under a
    /// name starting as `suggested`; `filters` are extensions.
    pub fn ask(chooser: Handle, suggested: Option<&str>, filters: &str, t: u32) -> bool {
        let mut out = vec![0u8; 1024];
        let n = match suggested {
            Some(name) => encode(
                &pc::Save {
                    title: Data(b""),
                    suggested: Data(name.as_bytes()),
                    filters: Data(filters.as_bytes()),
                },
                t,
                &mut out,
            ),
            None => encode(
                &pc::Open {
                    title: Data(b""),
                    filters: Data(filters.as_bytes()),
                },
                t,
                &mut out,
            ),
        };
        n.is_ok_and(|n| channel_write(chooser, &out[..n], &[]).is_ok())
    }

    /// What the chooser's channel held.
    pub enum Answer {
        /// Nothing yet.
        Waiting,
        /// The connection and the display name.
        Chosen(Handle, String),
        /// Nothing chosen, with the chooser's status.
        Ended(u32),
        /// The chooser is gone.
        Gone,
    }

    /// The chooser's answer, if one is there.
    pub fn answer(chooser: Handle) -> Answer {
        let mut bytes = vec![0u8; 2048];
        let mut handles = [chooser; 1];
        let sizes = match channel_read(chooser, &mut bytes, &mut handles) {
            Ok(s) => s,
            Err((Status::WouldBlock, _)) => return Answer::Waiting,
            Err(_) => return Answer::Gone,
        };
        let got = &handles[..sizes.handles as usize];
        let msg = &bytes[..sizes.bytes as usize];
        let ordinal = Header::decode(msg).map_or(0, |(h, _)| h.ordinal);
        if ordinal == 8
            && let Ok((_, m)) = decode::<pc::Chosen<'_>>(msg, got.len())
        {
            let shown = String::from(core::str::from_utf8(m.name.0).unwrap_or("?"));
            return Answer::Chosen(got[0], shown);
        }
        for h in got {
            let _ = handle_close(*h);
        }
        Answer::Ended(decode::<pc::Cancelled>(msg, 0).map_or(u32::MAX, |(_, c)| c.status.0))
    }
}
