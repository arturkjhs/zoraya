#![no_std]
#![no_main]

//! The compositor (ADR-0093, ADR-0094) as a process: `zoraya-compositor`'s
//! loop over Zoraya's calls. Slot 5 is the manager channel from `init`, slot
//! 6 the display driver, slots 7 to 10 input drivers.

use alloc::vec::Vec;

use zoraya_abi::spawn::Transfer;
use zoraya_abi::{
    ChannelEntry, ChannelObject, DEADLINE_INFINITE, Handle, MapFlags, MemoryFlags, MemoryObject,
    Mode, ObjectType, Packet, Status, channel_signals,
};
use zoraya_compositor::{Compositor, Incoming, Kind, Outgoing, Platform};
use zoraya_user::files::File;
use zoraya_user::{
    channel_read, channel_write, clock_monotonic, handle_close, handle_duplicate, handle_type,
    memory_create, memory_map, memory_unmap, port_create, port_subscribe, port_unsubscribe,
    port_wait, println,
};

extern crate alloc;

// Its description declares the desktop's settings (ADR-0169), which
// zoraya_compositor::settings reads from the same file.
zoraya_user::describe!("system", "compd");

/// How long a driver may take to answer.
const DRIVER_NS: u64 = 2_000_000_000;

struct Frame {
    object: Handle,
    pixels: &'static mut [u32],
    width: u32,
    height: u32,
}

struct System {
    port: Handle,
    calls: Handle,
    frame: Option<Frame>,
}

fn moved(handle: Handle) -> ChannelEntry {
    ChannelEntry {
        handle,
        rights: if handle_type(handle) == Ok("memory") {
            MemoryObject::RIGHTS
        } else {
            ChannelObject::RIGHTS
        },
        transfer: Transfer::Move,
    }
}

impl Platform for System {
    type End = Handle;

    fn read(&mut self, end: Handle, bytes: &mut [u8], handles: &mut Vec<Handle>) -> Incoming {
        let mut got = [end; 64];
        match channel_read(end, bytes, &mut got) {
            Ok(sizes) => {
                handles.extend_from_slice(&got[..sizes.handles as usize]);
                Incoming::Message(sizes.bytes as usize)
            }
            Err((Status::WouldBlock, _)) => Incoming::Empty,
            Err((Status::PeerClosed, _)) => Incoming::Closed,
            Err(_) => Incoming::Refused,
        }
    }

    fn write(&mut self, end: Handle, bytes: &[u8], handles: &[Handle]) -> Outgoing {
        let entries: Vec<ChannelEntry> = handles.iter().map(|h| moved(*h)).collect();
        match channel_write(end, bytes, &entries) {
            Ok(()) => Outgoing::Sent,
            Err(Status::PeerClosed) => Outgoing::Closed,
            Err(_) => Outgoing::Full,
        }
    }

    fn close(&mut self, end: Handle) {
        let _ = handle_close(end);
    }

    fn kind(&mut self, end: Handle) -> Kind {
        match handle_type(end) {
            Ok("channel") => Kind::Channel,
            Ok("memory") => Kind::Memory,
            _ => Kind::Other,
        }
    }

    fn watch(&mut self, end: Handle, key: u64) -> bool {
        let wanted = channel_signals::READABLE | channel_signals::PEER_CLOSED;
        // A key reused for a new end: the old subscription went with its
        // handle, but say so anyway.
        let _ = port_unsubscribe(self.port, key);
        port_subscribe(self.port, end, wanted, key, Mode::Level).is_ok()
    }

    fn map(&mut self, memory: Handle, length: u64) -> Option<u64> {
        memory_map(memory, 0, length, MapFlags::READ).ok()
    }

    fn unmap(&mut self, address: u64, length: u64) {
        let _ = memory_unmap(address, length);
    }

    fn copy_from(&mut self, address: u64, out: &mut [u32]) -> bool {
        for (i, o) in out.iter_mut().enumerate() {
            // SAFETY: `address` is inside a read-only mapping of this
            // process that the compositor made for the surface, and the
            // compositor checked the rectangle against the surface, so every
            // word read is inside it. The client can change it at any time:
            // each word is read once, and a pixel needs no validation.
            *o = unsafe { ((address + 4 * i as u64) as *const u32).read_volatile() };
        }
        true
    }

    fn framebuffer(&mut self, width: u32, height: u32) -> Option<Handle> {
        let same = self
            .frame
            .as_ref()
            .is_some_and(|f| (f.width, f.height) == (width, height));
        if !same {
            let pixels = width as usize * height as usize;
            let size = (pixels as u64 * 4).next_multiple_of(4096);
            let object = memory_create(size, MemoryFlags::NONE).ok()?;
            let Ok(address) = memory_map(object, 0, size, MapFlags::WRITE) else {
                let _ = handle_close(object);
                return None;
            };
            // SAFETY: a new writable mapping of `size` bytes, page-aligned,
            // which this process keeps for its life and reaches only
            // through this slice.
            let slice = unsafe { core::slice::from_raw_parts_mut(address as *mut u32, pixels) };
            // An older framebuffer stays mapped: a driver may still hold a
            // pin on it until it is reset (ADR-0054).
            self.frame = Some(Frame {
                object,
                pixels: slice,
                width,
                height,
            });
        }
        let frame = self.frame.as_ref()?;
        handle_duplicate(frame.object, MemoryObject::RIGHTS).ok()
    }

    fn pixels(&mut self) -> &mut [u32] {
        match self.frame.as_mut() {
            Some(f) => f.pixels,
            None => &mut [],
        }
    }

    fn call(
        &mut self,
        end: Handle,
        message: &[u8],
        handles: &[Handle],
        reply: &mut [u8],
    ) -> Option<usize> {
        let entries: Vec<ChannelEntry> = handles.iter().map(|h| moved(*h)).collect();
        if channel_write(end, message, &entries).is_err() {
            for h in handles {
                let _ = handle_close(*h);
            }
            return None;
        }
        let wanted = channel_signals::READABLE | channel_signals::PEER_CLOSED;
        port_subscribe(self.calls, end, wanted, 1, Mode::Level).ok()?;
        let mut packets = [Packet::default()];
        let waited = port_wait(self.calls, clock_monotonic() + DRIVER_NS, &mut packets);
        let _ = port_unsubscribe(self.calls, 1);
        waited.ok()?;
        let mut got = [end; 1];
        let sizes = channel_read(end, reply, &mut got).ok()?;
        for h in &got[..sizes.handles as usize] {
            let _ = handle_close(*h);
        }
        Some(sizes.bytes as usize)
    }

    fn wait(&mut self, ready: &mut Vec<u64>, deadline: Option<u64>) {
        let mut packets = [Packet::default(); 16];
        let until = deadline.unwrap_or(DEADLINE_INFINITE);
        if let Ok(count) = port_wait(self.port, until, &mut packets) {
            ready.extend(packets[..count].iter().map(|p| p.key));
        }
    }

    fn now(&mut self) -> u64 {
        clock_monotonic()
    }

    fn note(&mut self, line: &str) {
        println!("{line}");
    }
}

fn channel_at(slot: u64) -> Option<Handle> {
    Handle::new(slot).filter(|h| handle_type(*h) == Ok("channel"))
}

#[unsafe(no_mangle)]
fn main() -> i32 {
    let system = System {
        port: port_create().expect("port"),
        calls: port_create().expect("port"),
        frame: None,
    };
    let manager = channel_at(5).expect("a manager channel at slot 5");
    let display = channel_at(6);
    let inputs: Vec<Handle> = (7..11).filter_map(channel_at).collect();
    // The desktop's settings, once, at start (ADR-0169); bad lines are
    // reported and keep their defaults.
    let bytes = channel_at(11)
        .map(|end| File { end })
        .and_then(|config| {
            let f = config
                .open("settings", 0, zoraya_proto_file::rights::READ)
                .ok()?;
            zoraya_user::files::read_all(&f).ok()
        })
        .unwrap_or_default();
    // The same declarations the compositor reads (the packer checks this
    // copy against the file).
    debug_assert_eq!(DESCRIPTION, zoraya_compositor::settings::DESCRIPTION);
    let (settings, problems) = zoraya_compositor::settings::read(&bytes);
    for p in problems {
        println!("{p}");
    }
    let mut compositor = Compositor::with_settings(system, manager, display, &inputs, settings);
    compositor.run()
}
