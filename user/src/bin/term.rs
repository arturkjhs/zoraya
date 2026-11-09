#![no_std]
#![no_main]

//! The terminal (ADR-0096): a client of the compositor at slot 5, and the
//! terminal end of the shell's channel at slot 6. It draws what the shell
//! writes, and sends the shell the bytes keys make. It answers no query.
//!
//! Since stage 7b it is a toplevel window (ADR-0160): its size is logical
//! at the display's scale (ADR-0164), a `Configure` of another size gives
//! the grid a new size and the window a new buffer, and the close button
//! (`CloseRequested`) ends it, as `exit` in the shell does.

use alloc::vec::Vec;

use zoraya_abi::spawn::Transfer;
use zoraya_abi::{
    ChannelEntry, DEADLINE_INFINITE, Handle, MapFlags, MemoryFlags, Mode, Packet, Rights, Status,
    channel_signals,
};
use zoraya_proto_gui::{self as gui, modifiers};
use zoraya_proto_term::{Input, MAX_INPUT, Output, Size, Title};
use zoraya_protocol::{Data, Header, NO_REPLY, decode, encode};
use zoraya_term::{Look, Term, key_bytes};
use zoraya_tokens::{Appearance, Scale};
use zoraya_user::gui::wait_reply;
use zoraya_user::{
    channel_read, channel_write, handle_close, handle_duplicate, memory_create, memory_map,
    memory_unmap, port_create, port_subscribe, port_unsubscribe, port_wait, println,
};

extern crate alloc;

const KEY_COMPOSITOR: u64 = 1;
const KEY_SHELL: u64 = 2;
/// The shell's end has room again, subscribed only while keys wait.
const KEY_WRITABLE: u64 = 3;
/// Key bytes held for a shell that is slow to read them. A person cannot
/// type this far ahead; past it keys are dropped and counted.
const PENDING_MAX: usize = 4096;
/// Shell messages taken per wake, so that keys and frames stay prompt.
const BATCH: usize = 16;

struct Window {
    compositor: Handle,
    shell: Handle,
    term: Term,
    look: Look,
    /// The compositor's name for the window, and the display's scale.
    id: u32,
    scale: u32,
    pixels: &'static mut [u32],
    /// The buffer's mapping: its address and length.
    mapped: (u64, u64),
    stride: usize,
    height: usize,
    /// A commit waits for its frame callback.
    outstanding: bool,
    /// Key bytes not yet sent: the shell's queue was full. They go out
    /// together, up to `MAX_INPUT` a message, when it has room (the TCG
    /// key-burst loss, docs/backlog.md).
    pending: Vec<u8>,
    /// Input that did not fit in `pending`.
    dropped: u32,
    port: Handle,
    /// `KEY_WRITABLE` is subscribed.
    waiting: bool,
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

/// A buffer of `width × height` pixels: the writable mapping, and a
/// read-only view for the compositor (ADR-0093).
fn buffer(width: usize, height: usize) -> Option<(&'static mut [u32], (u64, u64), ChannelEntry)> {
    let size = (width * height * 4).next_multiple_of(4096) as u64;
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
    // SAFETY: a new writable mapping of `size` bytes, page-aligned, kept
    // until the window gets another buffer and reached only through this
    // slice, which is replaced before the mapping goes.
    let pixels = unsafe { core::slice::from_raw_parts_mut(address as *mut u32, width * height) };
    let entry = ChannelEntry {
        handle: view,
        rights: Rights::READ,
        transfer: Transfer::Move,
    };
    Some((pixels, (address, size), entry))
}

impl Window {
    fn commit(&mut self, y: usize, height: usize) {
        let m = gui::CommitTo {
            window: self.id,
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
        } else {
            // Not sent: draw it all again next time (review S2).
            self.term.redraw();
        }
    }

    /// The window is to be `width × height` logical pixels: a grid that
    /// fits, a buffer of exactly that size, everything drawn again, and
    /// the shell told the new grid.
    fn configure(&mut self, width: u32, height: u32) {
        let (pw, ph) = (
            (width * self.scale) as usize,
            (height * self.scale) as usize,
        );
        if (pw, ph) == (self.stride, self.height) || pw == 0 || ph == 0 {
            return;
        }
        let Some((pixels, mapped, entry)) = buffer(pw, ph) else {
            return;
        };
        let m = gui::SetBuffer {
            window: self.id,
            width,
            height,
        };
        let mut out = [0u8; 64];
        let Ok(n) = encode(&m, NO_REPLY, &mut out) else {
            return;
        };
        if channel_write(self.compositor, &out[..n], &[entry]).is_err() {
            let _ = memory_unmap(mapped.0, mapped.1);
            return;
        }
        let old = core::mem::replace(&mut self.mapped, mapped);
        self.pixels = pixels;
        // The old slice was replaced above: nothing refers to the old
        // mapping any more.
        let _ = memory_unmap(old.0, old.1);
        self.stride = pw;
        self.height = ph;
        let cw = self.look.cell_width();
        let ch = self.look.cell_height();
        let columns = pw.saturating_sub(2 * self.look.pad) / cw;
        let rows = ph.saturating_sub(2 * self.look.pad) / ch;
        self.term.resize(columns, rows);
        let (columns, rows) = self.term.size();
        self.look.clear(self.pixels);
        self.term.redraw();
        let size = Size {
            columns: columns as u32,
            rows: rows as u32,
        };
        if let Ok(n) = encode(&size, NO_REPLY, &mut out) {
            let _ = channel_write(self.shell, &out[..n], &[]);
        }
        // The whole buffer goes with the next commit.
        self.outstanding = false;
        self.term
            .render(&mut self.look, self.pixels, self.stride, true);
        self.commit(0, self.height);
    }

    fn show(&mut self) {
        if self.outstanding {
            return;
        }
        if let Some((first, last)) =
            self.term
                .render(&mut self.look, self.pixels, self.stride, true)
        {
            let cell = self.look.cell_height();
            let y = self.look.pad + first * cell;
            let h = (last - first + 1) * cell;
            self.commit(y, h.min(self.height - y));
        }
    }

    /// Queues key bytes for the shell; `flush` sends them.
    /// A key's bytes go whole or not at all, so that the shell never gets
    /// half an escape sequence or half a UTF-8 letter.
    fn send(&mut self, bytes: &[u8]) {
        if self.pending.len() + bytes.len() <= PENDING_MAX {
            self.pending.extend_from_slice(bytes);
        } else {
            self.dropped = self.dropped.saturating_add(bytes.len() as u32);
        }
    }

    /// Sends what keys wait, in as few messages as fit, and stops at the
    /// first the shell's queue has no room for: the rest wait for
    /// `WRITABLE`, in order.
    fn flush(&mut self) {
        while !self.pending.is_empty() {
            let n = self.pending.len().min(MAX_INPUT);
            let m = Input {
                dropped: self.dropped,
                bytes: Data(&self.pending[..n]),
            };
            let mut out = [0u8; 64 + MAX_INPUT];
            let Ok(length) = encode(&m, NO_REPLY, &mut out) else {
                // Cannot happen for n <= MAX_INPUT; counted if it does.
                self.dropped = self.dropped.saturating_add(self.pending.len() as u32);
                self.pending.clear();
                break;
            };
            match channel_write(self.shell, &out[..length], &[]) {
                Ok(()) => {
                    self.dropped = 0;
                    self.pending.drain(..n);
                }
                // The shell is slow: the terminal does not wait for it, and
                // WRITABLE says when its queue has room.
                Err(Status::WouldBlock) => break,
                // Any other refusal (no kernel memory) leaves WRITABLE
                // asserted: waiting for it would spin, so these keys are
                // dropped and counted (review finding 8).
                Err(_) => {
                    self.dropped = self.dropped.saturating_add(self.pending.len() as u32);
                    self.pending.clear();
                    break;
                }
            }
        }
        let want = !self.pending.is_empty();
        if want && !self.waiting {
            self.waiting = port_subscribe(
                self.port,
                self.shell,
                channel_signals::WRITABLE,
                KEY_WRITABLE,
                Mode::Level,
            )
            .is_ok();
        } else if !want && self.waiting {
            let _ = port_unsubscribe(self.port, KEY_WRITABLE);
            self.waiting = false;
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
                7 => self.outstanding = false,
                19 => {
                    if let Ok((_, c)) = decode::<gui::Configure>(bytes, 0)
                        && c.window == self.id
                    {
                        self.configure(c.width, c.height);
                    }
                }
                // The close button: the terminal ends, as `exit` does.
                20 => zoraya_user::process_exit(0),
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
            let bytes = &self.message[..length];
            if let Ok((_, m)) = decode::<Output<'_>>(bytes, 0) {
                self.term.write(m.bytes.0);
            } else if let Ok((_, t)) = decode::<Title<'_>>(bytes, 0) {
                // `zosh — <directory>` (ADR-0168), never from output.
                let mut title = alloc::vec::Vec::with_capacity(t.text.0.len() + 9);
                title.extend_from_slice("zosh — ".as_bytes());
                title.extend_from_slice(t.text.0);
                let m = gui::SetTitle {
                    window: self.id,
                    text: Data(&title),
                };
                let mut out = [0u8; 320];
                if let Ok(n) = encode(&m, NO_REPLY, &mut out) {
                    let _ = channel_write(self.compositor, &out[..n], &[]);
                }
            }
        }
    }
}

#[unsafe(no_mangle)]
fn main() -> i32 {
    let compositor = slot(5);
    let shell = slot(6);
    let mut reply = [0u8; 64];
    let n =
        ask(compositor, &gui::DisplayScale {}, &[], &mut reply).expect("the compositor answers");
    let (_, display) = decode::<gui::DisplayScaleReply>(&reply[..n], 0).expect("a display reply");
    if display.status.0 != gui::status::OK {
        println!("term: no display");
        return 1;
    }
    // The light set until 7f's settings (ADR-0169); the scale is the
    // display's (ADR-0164).
    let scale = Scale::new(display.scale).unwrap_or(Scale::ONE);
    let k = scale.factor() as usize;
    let look = Look::new(scale, Appearance::Light);
    let (columns, rows) = look.grid_for(display.width * k as u32, display.height * k as u32);
    let (width, height) = look.surface_size(columns, rows);
    // Whole logical pixels.
    let (width, height) = (width.next_multiple_of(k), height.next_multiple_of(k));
    let Some((pixels, mapped, entry)) = buffer(width, height) else {
        println!("term: no memory for the window");
        return 1;
    };
    let create = gui::CreateToplevel {
        width: (width / k) as u32,
        height: (height / k) as u32,
    };
    let n = ask(compositor, &create, &[entry], &mut reply).expect("the compositor answers");
    let id = match decode::<gui::Created>(&reply[..n], 0) {
        Ok((_, d)) if d.status.0 == gui::status::OK => d.window,
        other => {
            println!("term: no window: {other:?}");
            return 1;
        }
    };
    let mut out = [0u8; 300];
    let title = gui::SetTitle {
        window: id,
        text: Data(b"zosh"),
    };
    if let Ok(n) = encode(&title, NO_REPLY, &mut out) {
        let _ = channel_write(compositor, &out[..n], &[]);
    }
    let size_message = Size {
        columns: columns as u32,
        rows: rows as u32,
    };
    if let Ok(n) = encode(&size_message, NO_REPLY, &mut out) {
        let _ = channel_write(shell, &out[..n], &[]);
    }
    let port = port_create().expect("port");
    let mut window = Window {
        compositor,
        shell,
        term: Term::new(columns, rows),
        look,
        id,
        scale: k as u32,
        pixels,
        mapped,
        stride: width,
        height,
        outstanding: false,
        pending: Vec::new(),
        dropped: 0,
        port,
        waiting: false,
        // As large as any message: a `Paste` carries up to 32 KiB, and a
        // read into less is an error (review 7f's check).
        message: alloc::vec![0u8; zoraya_protocol::MAX_WIRE_MESSAGE],
    };
    window.look.clear(window.pixels);
    window
        .term
        .render(&mut window.look, window.pixels, width, true);
    window.commit(0, height);
    let wanted = channel_signals::READABLE | channel_signals::PEER_CLOSED;
    port_subscribe(port, compositor, wanted, KEY_COMPOSITOR, Mode::Level).expect("subscribe");
    port_subscribe(port, shell, wanted, KEY_SHELL, Mode::Level).expect("subscribe");
    let mut packets = [Packet::default(); 3];
    loop {
        let Ok(count) = port_wait(port, DEADLINE_INFINITE, &mut packets) else {
            return 1;
        };
        for p in &packets[..count] {
            match p.key {
                KEY_COMPOSITOR => window.compositor_messages(),
                KEY_SHELL => window.shell_messages(),
                _ => {}
            }
        }
        window.flush();
        window.show();
    }
}
