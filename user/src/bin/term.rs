#![no_std]
#![no_main]

//! The terminal (ADR-0096): a client of the compositor at slot 5, and the
//! terminal end of the shell's channel at slot 6. It draws what the shell
//! writes, and sends the shell the bytes keys make. It answers no query.

use alloc::vec::Vec;

use zoraya_abi::spawn::Transfer;
use zoraya_abi::{
    ChannelEntry, DEADLINE_INFINITE, Handle, MapFlags, MemoryFlags, Mode, Packet, Rights, Status,
    channel_signals,
};
use zoraya_font::{CELL_HEIGHT, CELL_WIDTH};
use zoraya_proto_gui::{self as gui, modifiers};
use zoraya_proto_term::{Input, MAX_INPUT, Output, Size};
use zoraya_protocol::{Data, Header, NO_REPLY, decode, encode};
use zoraya_term::{PAD, Term, key_bytes, surface_size};
use zoraya_user::gui::wait_reply;
use zoraya_user::{
    channel_read, channel_write, handle_close, handle_duplicate, memory_create, memory_map,
    port_create, port_subscribe, port_wait, println,
};

extern crate alloc;

const COLUMNS: usize = 80;
const ROWS: usize = 24;
const KEY_COMPOSITOR: u64 = 1;
const KEY_SHELL: u64 = 2;
/// Shell messages taken per wake, so that keys and frames stay prompt.
const BATCH: usize = 16;

struct Window {
    compositor: Handle,
    shell: Handle,
    term: Term,
    pixels: &'static mut [u32],
    stride: usize,
    height: usize,
    /// A commit waits for its frame callback.
    outstanding: bool,
    /// Input the shell's queue had no room for.
    dropped: u32,
    message: Vec<u8>,
}

fn slot(n: u64) -> Handle {
    Handle::new(n).expect("slot is not zero")
}

/// Sends `m` to the compositor and waits for its reply.
fn ask<'a, M: zoraya_protocol::Message<'a>>(
    end: Handle,
    m: &M,
    handles: &[ChannelEntry],
    reply: &mut [u8],
) -> Option<usize> {
    let mut out = [0u8; 64];
    let n = encode(m, 1, &mut out).ok()?;
    channel_write(end, &out[..n], handles).ok()?;
    // Events may come first (a new surface gets `Focus`): skip them.
    loop {
        let n = wait_reply(end, reply, 5_000_000_000)?;
        let ordinal = Header::decode(&reply[..n]).ok()?.0.ordinal;
        if !gui::CLIENT_EVENTS.contains(&ordinal) {
            return Some(n);
        }
    }
}

impl Window {
    fn commit(&mut self, y: usize, height: usize) {
        let m = gui::Commit {
            x: 0,
            y: y as u32,
            width: self.stride as u32,
            height: height as u32,
        };
        let mut out = [0u8; 64];
        if let Ok(n) = encode(&m, 1, &mut out)
            && channel_write(self.compositor, &out[..n], &[]).is_ok()
        {
            self.outstanding = true;
        }
    }

    fn show(&mut self) {
        if self.outstanding {
            return;
        }
        if let Some((first, last)) = self.term.render(self.pixels, self.stride, true) {
            let y = PAD + first * CELL_HEIGHT;
            let h = (last - first + 1) * CELL_HEIGHT;
            self.commit(y, h.min(self.height - y));
        }
    }

    fn send(&mut self, bytes: &[u8]) {
        if bytes.is_empty() {
            return;
        }
        let m = Input {
            dropped: self.dropped,
            bytes: Data(bytes),
        };
        let mut out = [0u8; 64 + MAX_INPUT];
        let Ok(n) = encode(&m, NO_REPLY, &mut out) else {
            return;
        };
        match channel_write(self.shell, &out[..n], &[]) {
            Ok(()) => self.dropped = 0,
            // The shell is slow: the terminal does not wait for it.
            Err(_) => self.dropped = self.dropped.saturating_add(bytes.len() as u32),
        }
    }

    fn compositor_messages(&mut self) {
        let mut handles = [self.compositor; 1];
        loop {
            let length = match channel_read(self.compositor, &mut self.message, &mut handles) {
                Ok(sizes) => {
                    for h in &handles[..sizes.handles as usize] {
                        let _ = handle_close(*h);
                    }
                    sizes.bytes as usize
                }
                Err((Status::WouldBlock, _)) => return,
                Err(_) => zoraya_user::process_exit(1),
            };
            let bytes = &self.message[..length];
            let Ok((header, _)) = Header::decode(bytes) else {
                continue;
            };
            match header.ordinal {
                3 => self.outstanding = false,
                16 => {
                    let Ok((_, k)) = decode::<gui::Key>(bytes, 0) else {
                        continue;
                    };
                    if !k.pressed {
                        continue;
                    }
                    let shift = k.modifiers.0 & modifiers::SHIFT != 0;
                    let half = (self.term.size().1 / 2) as isize;
                    match k.code {
                        zoraya_keymap::code::PAGE_UP if shift => self.term.scroll_view(half),
                        zoraya_keymap::code::PAGE_DOWN if shift => self.term.scroll_view(-half),
                        _ => {
                            // 0 is no text: Ctrl with Space sends nothing.
                            let text = (k.text.0 != 0).then(|| char::from_u32(k.text.0)).flatten();
                            let mut out = [0u8; 8];
                            let n = key_bytes(k.code, shift, text, &mut out);
                            self.send(&out[..n]);
                        }
                    }
                }
                _ => {}
            }
        }
    }

    fn shell_messages(&mut self) {
        let mut handles = [self.shell; 1];
        for _ in 0..BATCH {
            let length = match channel_read(self.shell, &mut self.message, &mut handles) {
                Ok(sizes) => {
                    for h in &handles[..sizes.handles as usize] {
                        let _ = handle_close(*h);
                    }
                    sizes.bytes as usize
                }
                Err((Status::WouldBlock, _)) => return,
                Err((Status::PeerClosed, _)) => {
                    self.term.write(b"\n[the shell ended]\n");
                    self.show();
                    zoraya_user::process_exit(0);
                }
                Err(_) => zoraya_user::process_exit(1),
            };
            if let Ok((_, m)) = decode::<Output<'_>>(&self.message[..length], 0) {
                self.term.write(m.bytes.0);
            }
        }
    }
}

#[unsafe(no_mangle)]
fn main() -> i32 {
    let compositor = slot(5);
    let shell = slot(6);
    let mut reply = [0u8; 64];
    let n = ask(compositor, &gui::Display {}, &[], &mut reply).expect("the compositor answers");
    let (_, display) = decode::<gui::DisplayReply>(&reply[..n], 0).expect("a display reply");
    if display.status.0 != gui::status::OK {
        println!("term: no display");
        return 1;
    }
    // At most 80 × 24, less on a small display.
    let fit = |side: u32, cell: usize| (side as usize).saturating_sub(2 * PAD + 8) / cell;
    let columns = COLUMNS.min(fit(display.width, CELL_WIDTH)).max(1);
    let rows = ROWS.min(fit(display.height, CELL_HEIGHT)).max(1);
    let (width, height) = surface_size(columns, rows);
    let size = (width * height * 4).next_multiple_of(4096) as u64;
    let memory = memory_create(size, MemoryFlags::NONE).expect("surface memory");
    let address = memory_map(memory, 0, size, MapFlags::WRITE).expect("map the surface");
    // SAFETY: a new writable mapping of `size` bytes, page-aligned, kept for
    // the process's life and reached only through this slice.
    let pixels = unsafe { core::slice::from_raw_parts_mut(address as *mut u32, width * height) };
    // The compositor gets a read-only view (ADR-0093).
    let view = handle_duplicate(memory, Rights::READ.union(Rights::TRANSFER)).expect("view");
    let entry = ChannelEntry {
        handle: view,
        rights: Rights::READ,
        transfer: Transfer::Move,
    };
    let create = gui::Create {
        width: width as u32,
        height: height as u32,
    };
    let n = ask(compositor, &create, &[entry], &mut reply).expect("the compositor answers");
    match decode::<gui::Done>(&reply[..n], 0) {
        Ok((_, d)) if d.status.0 == gui::status::OK => {}
        other => {
            println!("term: no surface: {other:?}");
            return 1;
        }
    }
    let mut out = [0u8; 64];
    let size_message = Size {
        columns: columns as u32,
        rows: rows as u32,
    };
    if let Ok(n) = encode(&size_message, NO_REPLY, &mut out) {
        let _ = channel_write(shell, &out[..n], &[]);
    }
    let mut window = Window {
        compositor,
        shell,
        term: Term::new(columns, rows),
        pixels,
        stride: width,
        height,
        outstanding: false,
        dropped: 0,
        message: alloc::vec![0u8; 8192],
    };
    window.term.render(window.pixels, width, true);
    window.commit(0, height);
    let port = port_create().expect("port");
    let wanted = channel_signals::READABLE | channel_signals::PEER_CLOSED;
    port_subscribe(port, compositor, wanted, KEY_COMPOSITOR, Mode::Level).expect("subscribe");
    port_subscribe(port, shell, wanted, KEY_SHELL, Mode::Level).expect("subscribe");
    let mut packets = [Packet::default(); 2];
    loop {
        let Ok(count) = port_wait(port, DEADLINE_INFINITE, &mut packets) else {
            return 1;
        };
        for p in &packets[..count] {
            match p.key {
                KEY_COMPOSITOR => window.compositor_messages(),
                _ => window.shell_messages(),
            }
        }
        window.show();
    }
}
