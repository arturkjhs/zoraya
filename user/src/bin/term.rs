#![no_std]
#![no_main]

//! The terminal (ADR-0096): a client of the compositor at slot 5, and the
//! terminal end of the shell's channel at slot 6. It draws what the shell
//! writes, and sends the shell what the person does: the bytes keys make,
//! the pointer's reports when a program asked for them, focus, and the text
//! of a paste (`Paste`, never `Input`). Dragging selects text, and System+C
//! offers it to the compositor's clipboard when the compositor asks
//! (ADR-0170, ADR-0332 item 8). It answers no query: what it sends comes from
//! `zoraya_term::Session`'s gesture methods and nowhere else (INV-0332-2).
//!
//! Since stage 7b it is a toplevel window (ADR-0160): its size is logical
//! at the display's scale (ADR-0164), a `Configure` of another size gives
//! the grid a new size and the window a new buffer, and the close button
//! (`CloseRequested`) ends it, as `exit` in the shell does.

use alloc::collections::VecDeque;
use alloc::vec::Vec;

use zoraya_abi::spawn::Transfer;
use zoraya_abi::{
    ChannelEntry, DEADLINE_INFINITE, Handle, MapFlags, MemoryFlags, Mode, Packet, Rights, Status,
    channel_signals,
};
use zoraya_proto_gui as gui;
use zoraya_proto_term::{Input, MAX_INPUT, Output, Paste, Size, Title};
use zoraya_protocol::{Data, Header, NO_REPLY, decode, encode};
use zoraya_term::{Look, Outgoing, Session, Term, Waited, await_reply, sync_hold};
use zoraya_tokens::{Appearance, Scale};
use zoraya_user::{
    channel_read, channel_write, clock_monotonic, handle_close, handle_duplicate, memory_create,
    memory_map, memory_unmap, port_create, port_subscribe, port_unsubscribe, port_wait, println,
};

extern crate alloc;

const KEY_COMPOSITOR: u64 = 1;
const KEY_SHELL: u64 = 2;
/// The shell's end has room again, subscribed only while keys wait.
const KEY_WRITABLE: u64 = 3;
/// Key bytes held for a shell that is slow to read them. A person cannot
/// type this far ahead; past it keys are dropped and counted.
const PENDING_MAX: usize = 4096;
/// Paste bytes held for a slow shell: two of the largest pastes. A paste that
/// does not fit whole is dropped whole, so the shell never gets half of one.
const PASTE_PENDING_MAX: usize = 2 * zoraya_proto_term::paste::MAX_PASTE_BYTES;
/// Shell messages taken per wake, so that keys and frames stay prompt.
const BATCH: usize = 16;

struct Window {
    compositor: Handle,
    shell: Handle,
    session: Session,
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
    /// What is not yet sent: the shell's queue was full. It goes out in
    /// order, input up to `MAX_INPUT` a message, when the shell has room (the
    /// TCG key-burst loss, docs/backlog.md).
    pending: VecDeque<Outgoing>,
    /// The bytes of input and of paste in `pending`.
    pending_input: usize,
    pending_paste: usize,
    /// Input that did not fit in `pending`.
    dropped: u32,
    port: Handle,
    /// `KEY_WRITABLE` is subscribed.
    waiting: bool,
    message: Vec<u8>,
    /// When a synchronized update (mode 2026) began to hold the drawing.
    sync_since: Option<u64>,
    /// When to wake to draw a held update anyway.
    sync_deadline: Option<u64>,
}

fn slot(n: u64) -> Handle {
    Handle::new(n).expect("slot is not zero")
}

/// How long one wait for the compositor lasts before `term` says it is
/// waiting; it waits on for as long as the compositor lives.
const SLOW_NS: u64 = 5_000_000_000;

/// One wait on `end`: a message, nothing within `nanos`, or the channel
/// closed.
fn wait_once(end: Handle, reply: &mut [u8], nanos: u64) -> Waited {
    let Ok(port) = port_create() else {
        return Waited::Gone;
    };
    let wanted = channel_signals::READABLE | channel_signals::PEER_CLOSED;
    if port_subscribe(port, end, wanted, 1, Mode::Level).is_err() {
        let _ = handle_close(port);
        return Waited::Gone;
    }
    let mut packets = [Packet::default()];
    let _ = port_wait(port, clock_monotonic() + nanos, &mut packets);
    let _ = handle_close(port);
    let mut handles = [end; 1];
    match channel_read(end, reply, &mut handles) {
        Ok(sizes) => {
            for h in &handles[..sizes.handles as usize] {
                let _ = handle_close(*h);
            }
            Waited::Message(sizes.bytes as usize)
        }
        Err((Status::WouldBlock, _)) => Waited::Nothing,
        Err(_) => Waited::Gone,
    }
}

/// Sends `m` to the compositor and waits for its reply, for as long as the
/// compositor lives (`zoraya_term::await_reply`); `None` when it is gone.
fn ask<'a, M: zoraya_protocol::Message<'a>>(
    end: Handle,
    m: &M,
    handles: &[ChannelEntry],
    reply: &mut [u8],
) -> Option<usize> {
    let mut out = [0u8; 64];
    let n = encode(m, 1, &mut out).ok()?;
    channel_write(end, &out[..n], handles).ok()?;
    let is_event =
        |r: &[u8]| Header::decode(r).map_or(true, |(h, _)| gui::CLIENT_EVENTS.contains(&h.ordinal));
    await_reply(
        |r| wait_once(end, r, SLOW_NS),
        reply,
        is_event,
        || println!("term: waiting for the compositor"),
    )
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
            self.session.term.redraw();
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
        self.session.term.resize(columns, rows);
        let (columns, rows) = self.session.term.size();
        self.look.clear(self.pixels);
        self.session.term.redraw();
        let size = Size {
            columns: columns as u32,
            rows: rows as u32,
        };
        if let Ok(n) = encode(&size, NO_REPLY, &mut out) {
            let _ = channel_write(self.shell, &out[..n], &[]);
        }
        // The whole buffer goes with the next commit.
        self.outstanding = false;
        self.session
            .term
            .render(&mut self.look, self.pixels, self.stride, true);
        self.commit(0, self.height);
    }

    fn show(&mut self) {
        // A program that began a synchronized update gets the screen held
        // until it ends the update, or for `SYNC_HOLD_MS`; a commit that waits
        // for its reply is waited for (`zoraya_term::sync_hold`).
        let h = sync_hold(
            clock_monotonic(),
            self.sync_since,
            self.session.term.modes().synchronized,
            self.outstanding,
        );
        self.sync_since = h.since;
        self.sync_deadline = h.deadline;
        if !h.draw {
            return;
        }
        if let Some((first, last)) =
            self.session
                .term
                .render(&mut self.look, self.pixels, self.stride, true)
        {
            let cell = self.look.cell_height();
            let y = self.look.pad + first * cell;
            let h = (last - first + 1) * cell;
            self.commit(y, h.min(self.height - y));
        }
    }

    /// Takes what the session has to send into the queue for `flush`.
    /// Input goes whole or not at all, so that the shell never gets half an
    /// escape sequence or half a UTF-8 letter; a paste goes whole or not
    /// at all, for the same reason.
    fn collect(&mut self) {
        let mut paste: Vec<Outgoing> = Vec::new();
        for item in self.session.take_outbox() {
            match item {
                Outgoing::Input(bytes) => {
                    if self.pending_input + bytes.len() <= PENDING_MAX {
                        self.pending_input += bytes.len();
                        self.pending.push_back(Outgoing::Input(bytes));
                    } else {
                        self.dropped = self.dropped.saturating_add(bytes.len() as u32);
                    }
                }
                Outgoing::Paste { bytes, last } => {
                    paste.push(Outgoing::Paste { bytes, last });
                    if last {
                        let size: usize = paste
                            .iter()
                            .map(|p| match p {
                                Outgoing::Paste { bytes, .. } => bytes.len(),
                                Outgoing::Input(_) => 0,
                            })
                            .sum();
                        if self.pending_paste + size <= PASTE_PENDING_MAX {
                            self.pending_paste += size;
                            self.pending.extend(paste.drain(..));
                        } else {
                            println!("term: a paste of {size} bytes dropped: the shell is slow");
                            paste.clear();
                        }
                    }
                }
            }
        }
    }

    /// Sends what waits, in order and in as few messages as fit, and stops
    /// at the first the shell's queue has no room for: the rest waits for
    /// `WRITABLE`.
    fn flush(&mut self) {
        self.collect();
        while let Some(front) = self.pending.front_mut() {
            let mut out = [0u8; 64 + zoraya_proto_term::MAX_PASTE];
            let result = match front {
                Outgoing::Input(bytes) => {
                    let n = bytes.len().min(MAX_INPUT);
                    let m = Input {
                        dropped: self.dropped,
                        bytes: Data(&bytes[..n]),
                    };
                    match encode(&m, NO_REPLY, &mut out) {
                        Ok(length) => channel_write(self.shell, &out[..length], &[]).map(|()| n),
                        // Cannot happen for n <= MAX_INPUT; counted if it does.
                        Err(_) => Ok(usize::MAX),
                    }
                }
                Outgoing::Paste { bytes, last } => {
                    let m = Paste {
                        last: *last,
                        bytes: Data(&bytes[..]),
                    };
                    match encode(&m, NO_REPLY, &mut out) {
                        Ok(length) => {
                            channel_write(self.shell, &out[..length], &[]).map(|()| bytes.len())
                        }
                        Err(_) => Ok(usize::MAX),
                    }
                }
            };
            match result {
                Ok(usize::MAX) => match self.pending.pop_front() {
                    // Not encodable (it cannot happen: the parts are bounded
                    // by what the messages carry): dropped, and counted.
                    Some(Outgoing::Input(bytes)) => {
                        self.pending_input -= bytes.len();
                        self.dropped = self.dropped.saturating_add(bytes.len() as u32);
                    }
                    Some(Outgoing::Paste { bytes, .. }) => self.pending_paste -= bytes.len(),
                    None => {}
                },
                Ok(n) => match front {
                    Outgoing::Input(bytes) => {
                        self.dropped = 0;
                        self.pending_input -= n;
                        if n == bytes.len() {
                            self.pending.pop_front();
                        } else {
                            bytes.drain(..n);
                        }
                    }
                    Outgoing::Paste { .. } => {
                        self.pending_paste -= n;
                        self.pending.pop_front();
                    }
                },
                // The shell is slow: the terminal does not wait for it, and
                // WRITABLE says when its queue has room.
                Err(Status::WouldBlock) => break,
                // Any other refusal (no kernel memory) leaves WRITABLE
                // asserted: waiting for it would spin, so what waits is
                // dropped and counted (review finding 8).
                Err(_) => {
                    self.dropped = self.dropped.saturating_add(self.pending_input as u32);
                    self.pending.clear();
                    self.pending_input = 0;
                    self.pending_paste = 0;
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

    /// The cell under a point of the window's body, in logical pixels.
    fn cell_at(&self, x: u32, y: u32) -> (usize, usize) {
        let k = self.scale as usize;
        let (columns, rows) = self.session.term.size();
        self.look
            .cell_at(x as usize * k, y as usize * k, columns, rows)
    }

    /// The clipboard asked the terminal for its selection (System+C): offered
    /// when there is one, and nothing otherwise, so that the clipboard stays as
    /// it was.
    fn offer(&mut self) {
        let Some(text) = self.session.copy(&self.look, gui::MAX_CLIPBOARD) else {
            return;
        };
        let m = gui::Offer {
            text: Data(text.as_bytes()),
        };
        let mut out = alloc::vec![0u8; 64 + text.len()];
        if let Ok(n) = encode(&m, NO_REPLY, &mut out) {
            let _ = channel_write(self.compositor, &out[..n], &[]);
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
                // The wheel moves the view through the scrollback, as
                // Shift+PageUp does (proposed ADR-0322), or is reported to a
                // program that asked for the mouse (ADR-0385).
                46 => {
                    if let Ok((_, w)) = decode::<gui::Wheel>(bytes, 0)
                        && w.window == self.id
                    {
                        let (column, row) = self.cell_at(w.x, w.y);
                        self.session.wheel(column, row, w.dy, w.modifiers.0);
                    }
                }
                // The pointer over the body: a drag selects, or is reported.
                30 => {
                    if let Ok((_, p)) = decode::<gui::WindowPointerMods>(bytes, 0)
                        && p.window == self.id
                    {
                        let (column, row) = self.cell_at(p.x, p.y);
                        self.session
                            .pointer(column, row, p.buttons.0, p.modifiers.0);
                    }
                }
                22 => {
                    if let Ok((_, f)) = decode::<gui::WindowFocus>(bytes, 0)
                        && f.window == self.id
                    {
                        self.session.focus(f.focused);
                    }
                }
                // System+C and System+X (ADR-0170): the selection is offered;
                // a terminal cuts nothing.
                27 => {
                    if let Ok((_, c)) = decode::<gui::Copy>(bytes, 0)
                        && c.window == self.id
                    {
                        self.offer();
                    }
                }
                // System+V: the clipboard's text, for the shell, as a paste.
                28 => {
                    if let Ok((_, p)) = decode::<gui::Paste<'_>>(bytes, 0)
                        && p.window == self.id
                    {
                        self.session.paste(p.text.0);
                    }
                }
                16 => {
                    let Ok((_, k)) = decode::<gui::Key>(bytes, 0) else {
                        continue;
                    };
                    if !k.pressed {
                        continue;
                    }
                    // 0 is no text: Ctrl with Space makes none.
                    let text = (k.text.0 != 0).then(|| char::from_u32(k.text.0)).flatten();
                    self.session.key(k.code, k.modifiers.0, text);
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
                    self.session.output(b"\n[the shell ended]\n");
                    self.show();
                    zoraya_user::process_exit(0);
                }
                Err(_) => zoraya_user::process_exit(1),
            };
            let bytes = &self.message[..length];
            if let Ok((_, m)) = decode::<Output<'_>>(bytes, 0) {
                self.session.output(m.bytes.0);
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
    let Some(n) = ask(compositor, &gui::DisplayScale {}, &[], &mut reply) else {
        println!("term: the compositor is gone");
        return 1;
    };
    let Ok((_, display)) = decode::<gui::DisplayScaleReply>(&reply[..n], 0) else {
        println!("term: the compositor's answer is not a display's");
        return 1;
    };
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
    let Some(n) = ask(compositor, &create, &[entry], &mut reply) else {
        println!("term: the compositor is gone");
        return 1;
    };
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
    let Ok(port) = port_create() else {
        println!("term: no port");
        return 1;
    };
    let mut window = Window {
        compositor,
        shell,
        session: Session::new(Term::new(columns, rows)),
        look,
        id,
        scale: k as u32,
        pixels,
        mapped,
        stride: width,
        height,
        outstanding: false,
        pending: VecDeque::new(),
        pending_input: 0,
        pending_paste: 0,
        dropped: 0,
        port,
        waiting: false,
        sync_since: None,
        sync_deadline: None,
        // As large as any message: a `Paste` carries up to 32 KiB, and a
        // read into less is an error (review 7f's check).
        message: alloc::vec![0u8; zoraya_protocol::MAX_WIRE_MESSAGE],
    };
    window.look.clear(window.pixels);
    window
        .session
        .term
        .render(&mut window.look, window.pixels, width, true);
    window.commit(0, height);
    let wanted = channel_signals::READABLE | channel_signals::PEER_CLOSED;
    if port_subscribe(port, compositor, wanted, KEY_COMPOSITOR, Mode::Level).is_err()
        || port_subscribe(port, shell, wanted, KEY_SHELL, Mode::Level).is_err()
    {
        println!("term: cannot wait on its channels");
        return 1;
    }
    let mut packets = [Packet::default(); 3];
    loop {
        let deadline = window.sync_deadline.unwrap_or(DEADLINE_INFINITE);
        // A held synchronized update's deadline passing is no packet, and no
        // error: the screen is shown below.
        let count = match port_wait(port, deadline, &mut packets) {
            Ok(count) => count,
            Err(Status::DeadlineExceeded) => 0,
            Err(_) => return 1,
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
