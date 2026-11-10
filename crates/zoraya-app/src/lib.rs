//! `zoraya-app` (ADR-0165): the loop of an application. It connects to the
//! compositor through a [`Platform`], opens toplevels whose contents are
//! `zoraya-ui` trees, paints them into memory objects and commits one frame
//! at a time (ADR-0093's frame callback), and turns the compositor's events
//! into the widgets' events. It sends only the client requests of
//! `zoraya-proto-gui` (INV-0165-2): never a request for focus, position or
//! stacking, which the protocol does not have.

#![cfg_attr(not(test), no_std)]

extern crate alloc;

use alloc::boxed::Box;
use alloc::collections::VecDeque;
use alloc::vec;
use alloc::vec::Vec;

use zoraya_proto_gui::{self as gui, status};
use zoraya_protocol::{Data, Header, Message, decode, encode};
use zoraya_tokens::{Appearance, Logical, Scale};
use zoraya_ui::{Event, Painter, Response, Widget, Window};

/// What the application runs on: the compositor's channel and memory
/// objects. The program's is a thin layer over the kernel; the tests' is a
/// fake compositor.
pub trait Platform {
    /// A handle to give with a message.
    type End: Copy;
    /// Sends one message, with a memory object's handle or none.
    fn send(&mut self, bytes: &[u8], handle: Option<Self::End>) -> bool;
    /// The next message from the compositor, waiting for it; `None` when
    /// the compositor is gone.
    fn receive(&mut self, bytes: &mut [u8]) -> Option<usize>;
    /// A memory object of `pixels` pixels, mapped: its handle to give and
    /// its number for [`Platform::pixels`].
    fn buffer(&mut self, pixels: usize) -> Option<(Self::End, usize)>;
    fn pixels(&mut self, buffer: usize) -> &mut [u32];
    fn free(&mut self, buffer: usize);
    /// The handle the message last received carried, taken (a drop's
    /// entry, ADR-0170); the platform closes one nobody took before it
    /// receives the next. By default messages carry none.
    fn handle(&mut self) -> Option<Self::End> {
        None
    }
    /// The next message if one is there, without waiting, for a program
    /// that waits on other channels too (the chooser). By default it waits.
    fn poll(&mut self, bytes: &mut [u8]) -> Poll {
        match self.receive(bytes) {
            Some(n) => Poll::Message(n),
            None => Poll::Gone,
        }
    }
}

/// What [`Platform::poll`] found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Poll {
    Message(usize),
    /// Nothing yet.
    Empty,
    /// The compositor is gone.
    Gone,
}

/// What happened, for the application to act on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Happened {
    /// A widget of `window` answered an event.
    Ui { window: u32, response: Response },
    /// The person asked to close `window` (ADR-0160).
    CloseRequested(u32),
    /// The compositor is gone.
    Gone,
    /// An entry was dropped on `window`: [`App::take_drop`] gives it.
    Dropped(u32),
    /// The person asked `window` to copy (or, `cut`, to cut) its
    /// selection: answer with [`App::offer`] (ADR-0170).
    Copy { window: u32, cut: bool },
    /// The person pasted into `window`: [`App::take_paste`] gives the text.
    Pasted(u32),
    /// The show-commands mode turned on or off (to system clients only).
    Commands(bool),
}

/// A dropped entry (ADR-0170): the connection, its display name
/// and where in the window's body it was dropped, in logical pixels.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dropped<E> {
    pub window: u32,
    pub x: u32,
    pub y: u32,
    pub name: alloc::string::String,
    pub entry: E,
}

struct Win {
    id: u32,
    ui: Window,
    /// Logical size, and the buffer at its physical size.
    size: (u32, u32),
    buffer: usize,
    /// A commit waits for its reply; a change meanwhile waits for it.
    in_flight: bool,
    dirty: bool,
}

pub struct App<P: Platform> {
    pub platform: P,
    pub scale: Scale,
    /// The display's logical size.
    pub display: (u32, u32),
    painter: Painter,
    windows: Vec<Win>,
    focus: Option<u32>,
    /// Events that came while a reply was awaited.
    queued: VecDeque<Vec<u8>>,
    /// Windows with a commit in flight, in the order sent: replies come
    /// in that order.
    commits: VecDeque<u32>,
    next_transaction: u32,
    /// Entries dropped and not yet taken.
    drops: VecDeque<Dropped<P::End>>,
    /// The window whose body holds the left button, as the last pointer
    /// event said: where a drag may start.
    held: Option<u32>,
    /// The serial of the last left press delivered, for a drag to name.
    serial: u32,
    /// The text of the last `Paste`, until taken.
    pasted: Option<alloc::string::String>,
    /// The show-commands mode, as the compositor last said (system clients).
    pub commands: bool,
    /// The theme the windows are painted in, and whether they may move
    /// (`Configure`, ADR-0169).
    pub appearance: Appearance,
    pub motion: bool,
}

fn message<'a, M: Message<'a>>(m: &M, transaction: u32) -> Vec<u8> {
    // Most messages are small; an `Offer` carries up to 32 KiB, so a
    // message that does not fit is encoded again into the largest.
    let mut out = vec![0u8; 1024];
    let n = match encode(m, transaction, &mut out) {
        Ok(n) => n,
        Err(_) => {
            out = vec![0u8; zoraya_protocol::MAX_WIRE_MESSAGE];
            encode(m, transaction, &mut out).unwrap_or(0)
        }
    };
    out.truncate(n);
    out
}

impl<P: Platform> App<P> {
    /// Connects: the display's size and scale (ADR-0164).
    pub fn connect(platform: P, appearance: Appearance) -> Option<App<P>> {
        let mut app = App {
            platform,
            scale: Scale::ONE,
            display: (0, 0),
            painter: Painter::new(Scale::ONE, appearance),
            windows: Vec::new(),
            focus: None,
            queued: VecDeque::new(),
            commits: VecDeque::new(),
            next_transaction: 1,
            drops: VecDeque::new(),
            held: None,
            serial: 0,
            pasted: None,
            commands: false,
            appearance,
            motion: true,
        };
        let reply = app.call(&gui::DisplayScale {}, None)?;
        let (_, d) = decode::<gui::DisplayScaleReply>(&reply, 0).ok()?;
        if d.status.0 != status::OK {
            return None;
        }
        app.scale = Scale::new(d.scale).unwrap_or(Scale::ONE);
        app.display = (d.width, d.height);
        app.painter = Painter::new(app.scale, appearance);
        Some(app)
    }

    /// Sends a request and waits for its reply (the same ordinal and
    /// transaction); events meanwhile are kept for [`App::wait`].
    fn call<'a, M: Message<'a>>(&mut self, m: &M, handle: Option<P::End>) -> Option<Vec<u8>> {
        let t = self.next_transaction;
        self.next_transaction = self.next_transaction.wrapping_add(1).max(1);
        if !self.platform.send(&message(m, t), handle) {
            return None;
        }
        let mut bytes = vec![0u8; zoraya_protocol::MAX_WIRE_MESSAGE];
        loop {
            let n = self.platform.receive(&mut bytes)?;
            let got = &bytes[..n];
            self.received(got);
            match Header::decode(got) {
                Ok((h, _)) if h.ordinal == M::ORDINAL && h.transaction == t => {
                    return Some(got.to_vec());
                }
                _ => self.queued.push_back(got.to_vec()),
            }
        }
    }

    fn index(&self, id: u32) -> Option<usize> {
        self.windows.iter().position(|w| w.id == id)
    }

    /// A buffer of `w × h` logical pixels at the scale.
    fn new_buffer(&mut self, w: u32, h: u32) -> Option<(P::End, usize)> {
        let k = self.scale.factor() as usize;
        self.platform.buffer(w as usize * k * h as usize * k)
    }

    /// Opens a toplevel of `w × h` logical pixels titled `title`, showing
    /// `root`; its id.
    pub fn open(&mut self, title: &str, root: Box<dyn Widget>, w: u32, h: u32) -> Option<u32> {
        let (memory, buffer) = self.new_buffer(w, h)?;
        let reply = self.call(
            &gui::CreateToplevel {
                width: w,
                height: h,
            },
            Some(memory),
        )?;
        let created = decode::<gui::Created>(&reply, 0).ok().map(|(_, c)| c);
        let Some(c) = created.filter(|c| c.status.0 == status::OK) else {
            self.platform.free(buffer);
            return None;
        };
        self.add_window(c.window, root, (w, h), buffer);
        let t = Data(title.as_bytes());
        self.call(
            &gui::SetTitle {
                window: c.window,
                text: t,
            },
            None,
        );
        self.repaint(c.window);
        Some(c.window)
    }

    /// Gives `window` a new title (the frame draws it, ADR-0160).
    pub fn set_title(&mut self, window: u32, title: &str) -> bool {
        let m = gui::SetTitle {
            window,
            text: Data(title.as_bytes()),
        };
        self.call(&m, None).and_then(|r| {
            decode::<gui::TitleDone>(&r, 0)
                .ok()
                .map(|(_, d)| d.status.0)
        }) == Some(status::OK)
    }

    fn add_window(&mut self, id: u32, root: Box<dyn Widget>, (w, h): (u32, u32), buffer: usize) {
        let mut ui = Window::new(root, (self.scale_logical(w), self.scale_logical(h)));
        ui.layout(&mut self.painter);
        self.windows.push(Win {
            id,
            ui,
            size: (w, h),
            buffer,
            in_flight: false,
            dirty: true,
        });
    }

    /// Opens a sheet of `w × h` logical pixels under the window of the
    /// client the panel tagged `tag` (a system client only: the chooser,
    /// ADR-0167); `kind` is the heading the compositor draws.
    pub fn open_sheet(
        &mut self,
        tag: u32,
        kind: u32,
        root: Box<dyn Widget>,
        w: u32,
        h: u32,
    ) -> Option<u32> {
        let (memory, buffer) = self.new_buffer(w, h)?;
        let m = gui::CreateSheet {
            tag,
            kind: zoraya_protocol::Below(kind),
            width: w,
            height: h,
        };
        let reply = self.call(&m, Some(memory));
        let created = reply
            .as_deref()
            .and_then(|r| decode::<gui::SheetCreated>(r, 0).ok())
            .map(|(_, c)| c);
        let Some(c) = created.filter(|c| c.status.0 == status::OK) else {
            self.platform.free(buffer);
            return None;
        };
        self.add_window(c.window, root, (w, h), buffer);
        self.repaint(c.window);
        Some(c.window)
    }

    /// The next thing to act on if the compositor has sent it, without
    /// waiting: `None` when nothing is there.
    pub fn poll(&mut self) -> Option<Happened> {
        let mut bytes = vec![0u8; zoraya_protocol::MAX_WIRE_MESSAGE];
        loop {
            let got = match self.queued.pop_front() {
                Some(b) => b,
                None => match self.platform.poll(&mut bytes) {
                    Poll::Message(n) => {
                        self.received(&bytes[..n]);
                        bytes[..n].to_vec()
                    }
                    Poll::Empty => return None,
                    Poll::Gone => return Some(Happened::Gone),
                },
            };
            if let Some(h) = self.handle(&got) {
                return Some(h);
            }
        }
    }

    /// A message just received: a drop's entry is taken with it, before
    /// anything else is received (the platform closes it otherwise).
    fn received(&mut self, got: &[u8]) {
        let Ok((_, d)) = decode::<gui::Drop<'_>>(got, 1) else {
            return;
        };
        let Some(entry) = self.platform.handle() else {
            return;
        };
        let text = |b: &[u8]| alloc::string::String::from(core::str::from_utf8(b).unwrap_or(""));
        self.drops.push_back(Dropped {
            window: d.window,
            x: d.x,
            y: d.y,
            name: text(d.name.0),
            entry,
        });
    }

    /// The oldest dropped entry not yet taken.
    pub fn take_drop(&mut self) -> Option<Dropped<P::End>> {
        self.drops.pop_front()
    }

    /// Starts a drag of one entry from `window` (ADR-0170), while its body
    /// holds the left press the compositor delivered: `Ok` when the
    /// compositor carries it; `Err(Some(entry))` when it was not sent (no
    /// press held, or a name too long), the entry the caller's again;
    /// `Err(None)` when the compositor refused it and closed the entry.
    pub fn drag(&mut self, window: u32, entry: P::End, name: &str) -> Result<(), Option<P::End>> {
        if self.held != Some(window) || name.is_empty() || name.len() > gui::MAX_DROPPED_NAME {
            return Err(Some(entry));
        }
        let m = gui::Drag {
            window,
            serial: self.serial,
            name: Data(name.as_bytes()),
        };
        let ok = self
            .call(&m, Some(entry))
            .and_then(|r| decode::<gui::DragDone>(&r, 0).ok().map(|(_, d)| d.status.0))
            == Some(status::OK);
        if ok { Ok(()) } else { Err(None) }
    }

    /// Answers a `Copy` with `text` (within 500 ms of it, ADR-0170);
    /// whether the compositor kept it.
    pub fn offer(&mut self, text: &str) -> bool {
        if text.len() > gui::MAX_CLIPBOARD {
            return false;
        }
        let m = gui::Offer {
            text: Data(text.as_bytes()),
        };
        self.call(&m, None).and_then(|r| {
            decode::<gui::OfferDone>(&r, 0)
                .ok()
                .map(|(_, d)| d.status.0)
        }) == Some(status::OK)
    }

    /// The text of the last paste, once.
    pub fn take_paste(&mut self) -> Option<alloc::string::String> {
        self.pasted.take()
    }

    /// Whether `window`'s body holds the left button.
    pub fn held(&self, window: u32) -> bool {
        self.held == Some(window)
    }

    fn scale_logical(&self, v: u32) -> Logical {
        Scale::ONE.logical(v)
    }

    /// The widget tree of `window`, to change it; [`App::repaint`] after.
    pub fn window(&mut self, id: u32) -> Option<&mut Window> {
        let i = self.index(id)?;
        Some(&mut self.windows[i].ui)
    }

    /// Puts `root` in `window` in place of its tree, keeping its focus, and
    /// lays it out and paints it.
    pub fn replace(&mut self, id: u32, root: Box<dyn Widget>) {
        let Some(i) = self.index(id) else { return };
        let App {
            painter, windows, ..
        } = self;
        windows[i].ui.root = root;
        windows[i].ui.layout(painter);
        self.repaint(id);
    }

    /// Paints `window` and commits it, unless a commit is in flight: then
    /// it is painted when that one's reply comes.
    pub fn repaint(&mut self, id: u32) {
        let Some(i) = self.index(id) else { return };
        self.windows[i].dirty = true;
        if self.windows[i].in_flight {
            return;
        }
        let k = self.scale.factor();
        let (w, h) = self.windows[i].size;
        let (pw, ph) = (w * k, h * k);
        let buffer = self.windows[i].buffer;
        let App {
            platform,
            painter,
            windows,
            ..
        } = self;
        let pixels = platform.pixels(buffer);
        windows[i].ui.paint(painter, pixels, pw as usize);
        windows[i].dirty = false;
        windows[i].in_flight = true;
        let t = self.next_transaction;
        self.next_transaction = self.next_transaction.wrapping_add(1).max(1);
        let m = gui::CommitTo {
            window: id,
            x: 0,
            y: 0,
            width: pw,
            height: ph,
        };
        if self.platform.send(&message(&m, t), None) {
            self.commits.push_back(id);
        } else {
            self.windows[i].in_flight = false;
        }
    }

    /// Closes `window`.
    pub fn close(&mut self, id: u32) {
        let Some(i) = self.index(id) else { return };
        self.call(&gui::Close { window: id }, None);
        let w = self.windows.remove(i);
        self.platform.free(w.buffer);
    }

    /// A new size from the compositor (ADR-0160): a new buffer at it, the
    /// tree laid out again, painted and committed.
    fn configure(&mut self, c: gui::Configure) {
        let Some(i) = self.index(c.window) else {
            return;
        };
        // The theme (ADR-0169): every window repainted in the new palette.
        let appearance = if c.dark {
            Appearance::Dark
        } else {
            Appearance::Light
        };
        if appearance != self.appearance {
            self.appearance = appearance;
            self.painter.palette = appearance.palette();
            let ids: Vec<u32> = self.windows.iter().map(|w| w.id).collect();
            for id in ids {
                self.repaint(id);
            }
        }
        self.motion = c.motion;
        if (c.width, c.height) == self.windows[i].size || c.width == 0 || c.height == 0 {
            return;
        }
        let Some((memory, buffer)) = self.new_buffer(c.width, c.height) else {
            return;
        };
        let m = gui::SetBuffer {
            window: c.window,
            width: c.width,
            height: c.height,
        };
        let ok = self.call(&m, Some(memory)).and_then(|r| {
            decode::<gui::BufferDone>(&r, 0)
                .ok()
                .map(|(_, d)| d.status.0)
        }) == Some(status::OK);
        let Some(i) = self.index(c.window) else {
            return;
        };
        if !ok {
            self.platform.free(buffer);
            return;
        }
        let old = core::mem::replace(&mut self.windows[i].buffer, buffer);
        self.platform.free(old);
        self.windows[i].size = (c.width, c.height);
        let size = (self.scale_logical(c.width), self.scale_logical(c.height));
        self.windows[i].ui.size = size;
        let App {
            painter, windows, ..
        } = self;
        windows[i].ui.layout(painter);
        self.repaint(c.window);
    }

    /// The next thing the application must act on; everything else (sizes,
    /// focus, frame replies, repaints) is handled here.
    pub fn wait(&mut self) -> Happened {
        let mut bytes = vec![0u8; zoraya_protocol::MAX_WIRE_MESSAGE];
        loop {
            let got = match self.queued.pop_front() {
                Some(b) => b,
                None => match self.platform.receive(&mut bytes) {
                    Some(n) => {
                        self.received(&bytes[..n]);
                        bytes[..n].to_vec()
                    }
                    None => return Happened::Gone,
                },
            };
            if let Some(h) = self.handle(&got) {
                return h;
            }
        }
    }

    fn ui_event(&mut self, id: u32, e: Event) -> Option<Happened> {
        let i = self.index(id)?;
        match self.windows[i].ui.event(e) {
            Response::None => None,
            Response::Repaint => {
                self.repaint(id);
                None
            }
            r => {
                self.repaint(id);
                Some(Happened::Ui {
                    window: id,
                    response: r,
                })
            }
        }
    }

    fn handle(&mut self, got: &[u8]) -> Option<Happened> {
        let (h, _) = Header::decode(got).ok()?;
        match h.ordinal {
            7 => {
                // A commit's reply: the frame is shown; paint what changed
                // meanwhile.
                let id = self.commits.pop_front()?;
                let i = self.index(id)?;
                self.windows[i].in_flight = false;
                if self.windows[i].dirty {
                    self.repaint(id);
                }
                None
            }
            19 => {
                let (_, c) = decode::<gui::Configure>(got, 0).ok()?;
                self.configure(c);
                None
            }
            20 => {
                let (_, c) = decode::<gui::CloseRequested>(got, 0).ok()?;
                Some(Happened::CloseRequested(c.window))
            }
            22 => {
                let (_, f) = decode::<gui::WindowFocus>(got, 0).ok()?;
                if f.focused {
                    self.focus = Some(f.window);
                } else if self.focus == Some(f.window) {
                    self.focus = None;
                }
                None
            }
            16 => {
                let (_, k) = decode::<gui::Key>(got, 0).ok()?;
                let id = self.focus?;
                if !k.pressed {
                    return None;
                }
                let text = (k.text.0 != 0).then(|| char::from_u32(k.text.0)).flatten();
                self.ui_event(
                    id,
                    Event::Key {
                        code: k.code,
                        text,
                        modifiers: k.modifiers.0,
                    },
                )
            }
            27 => {
                let (_, c) = decode::<gui::Copy>(got, 0).ok()?;
                Some(Happened::Copy {
                    window: c.window,
                    cut: c.cut,
                })
            }
            29 => {
                let (_, c) = decode::<gui::Commands>(got, 0).ok()?;
                self.commands = c.on;
                Some(Happened::Commands(c.on))
            }
            28 => {
                let (_, p) = decode::<gui::Paste<'_>>(got, 0).ok()?;
                self.pasted = Some(alloc::string::String::from(
                    core::str::from_utf8(p.text.0).ok()?,
                ));
                Some(Happened::Pasted(p.window))
            }
            25 => {
                let (_, d) = decode::<gui::Drop<'_>>(got, 1).ok()?;
                self.drops
                    .iter()
                    .any(|x| x.window == d.window)
                    .then_some(Happened::Dropped(d.window))
            }
            23 => {
                let (_, p) = decode::<gui::WindowPointer>(got, 0).ok()?;
                self.held = (p.buttons.0 & gui::buttons::LEFT != 0).then_some(p.window);
                self.serial = p.serial;
                let e = Event::Pointer {
                    x: self.scale_logical(p.x),
                    y: self.scale_logical(p.y),
                    buttons: p.buttons.0,
                };
                self.ui_event(p.window, e)
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests;

// A runaway test fails at 512 MiB instead of taking the host (docs/backlog.md).
#[cfg(test)]
zoraya_test_alloc::cap_memory!();
