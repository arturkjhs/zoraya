//! The kernel under `zoraya-app` and `zoraya-fsclient` (stage 7e): the
//! compositor's channel and pixel buffers for an application, and file
//! connections for the programs that browse the volume.

use alloc::vec::Vec;

use zoraya_abi::spawn::Transfer;
use zoraya_abi::{
    ChannelEntry, ChannelObject, DEADLINE_INFINITE, EventObject, Handle, MapFlags, MemoryFlags, ObjectType,
    Mode, Packet, Rights, Status, channel_signals,
};
use zoraya_rt::{
    channel_create, channel_read, channel_write, handle_close, handle_duplicate, handle_type,
    memory_create, memory_map, memory_unmap, port_create, port_subscribe, port_wait,
};

/// Waits until `end` is readable or closed.
fn wait(end: Handle) -> bool {
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
}

impl Gui {
    pub fn new(end: Handle) -> Gui {
        Gui {
            end,
            buffers: Vec::new(),
        }
    }

    fn read(&mut self, bytes: &mut [u8]) -> Result<usize, Status> {
        let mut handles = [self.end; 1];
        let sizes = channel_read(self.end, bytes, &mut handles).map_err(|(s, _)| s)?;
        for h in &handles[..sizes.handles as usize] {
            let _ = handle_close(*h);
        }
        Ok(sizes.bytes as usize)
    }
}

impl zoraya_app::Platform for Gui {
    type End = Handle;

    fn send(&mut self, bytes: &[u8], handle: Option<Handle>) -> bool {
        let entry = handle.map(|h| ChannelEntry {
            handle: h,
            rights: Rights::READ,
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
