//! The chooser (ADR-0167, proposed ADR-0245): a system client of the
//! compositor holding the volume's root. The panel registers each
//! application's chooser channel with the tag of its compositor client;
//! an application asks to open, save or pick a folder; the chooser shows
//! its sheet under that application's window, and only when the person
//! confirms there answers with one connection of exactly the grant's
//! rights, and tells the compositor the grant for the window's pills.
//!
//! Slots (from `init`): 1 the console, 5 the volume's root, 6 its
//! compositor channel (marked by `AddSystemClient`), 7 the panel's
//! registration channel.

#![no_std]
#![no_main]

extern crate alloc;

use alloc::collections::VecDeque;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use zoraya_abi::spawn::Transfer;
use zoraya_abi::{
    ChannelEntry, ChannelObject, DEADLINE_INFINITE, Handle, Mode, ObjectType, Packet, Status,
    channel_signals,
};
use zoraya_app::{App, Happened};
use zoraya_chooser::{Ask, Browser, Outcome, Sheet};
use zoraya_fsclient::Client;
use zoraya_proto_chooser::{self as pc, status as answer};
use zoraya_proto_gui as gui;
use zoraya_protocol::{Below, Data, Header, Message, decode, encode};
use zoraya_rt::{
    channel_read, channel_write, clock_monotonic, handle_close, handle_type, port_create,
    port_subscribe, port_unsubscribe, port_wait, println,
};
use zoraya_tokens::Appearance;
use zoraya_user::app::{Files, Gui};

const KEY_GUI: u64 = 1;
const KEY_PANEL: u64 = 2;
const KEY_APP: u64 = 100;
/// The most applications registered at once.
const MAX_APPS: usize = 64;

struct Registered {
    end: Handle,
    tag: u32,
    name: String,
    /// The request waiting or shown: its transaction and what it asks.
    pending: Option<(u32, Ask)>,
}

fn reply<'a, M: Message<'a>>(end: Handle, m: &M, t: u32, handle: Option<Handle>) {
    let mut out = vec![0u8; 2048];
    let Ok(n) = encode(m, t, &mut out) else {
        if let Some(h) = handle {
            let _ = handle_close(h);
        }
        return;
    };
    let entry = handle.map(|h| ChannelEntry {
        handle: h,
        rights: if handle_type(h) == Ok("channel") {
            ChannelObject::RIGHTS
        } else {
            zoraya_abi::Rights::NONE
        },
        transfer: Transfer::Move,
    });
    let entries: &[ChannelEntry] = match &entry {
        Some(e) => core::slice::from_ref(e),
        None => &[],
    };
    if channel_write(end, &out[..n], entries).is_err()
        && let Some(h) = handle
    {
        let _ = handle_close(h);
    }
}

fn ask_of(bytes: &[u8]) -> Option<Result<Ask, ()>> {
    let (h, _) = Header::decode(bytes).ok()?;
    Some(match h.ordinal {
        1 => decode::<pc::Open<'_>>(bytes, 0)
            .map(|(_, m)| Ask::Open {
                filters: Ask::filters(core::str::from_utf8(m.filters.0).unwrap_or("")),
            })
            .map_err(|_| ()),
        2 => decode::<pc::Save<'_>>(bytes, 0)
            .map(|(_, m)| Ask::Save {
                suggested: String::from(core::str::from_utf8(m.suggested.0).unwrap_or("")),
                filters: Ask::filters(core::str::from_utf8(m.filters.0).unwrap_or("")),
            })
            .map_err(|_| ()),
        3 => decode::<pc::PickDirectory>(bytes, 0)
            .map(|(_, m)| Ask::Pick {
                write: m.write.0 == 1,
            })
            .map_err(|_| ()),
        _ => Err(()),
    })
}

struct Chooser {
    app: App<Gui>,
    fs: Client<Files>,
    root: Handle,
    home: String,
    port: Handle,
    apps: Vec<Option<Registered>>,
    queue: VecDeque<usize>,
    /// The sheet shown, and whose request it answers.
    shown: Option<(usize, Sheet)>,
}

impl Chooser {
    fn register(&mut self, panel: Handle) {
        let mut bytes = vec![0u8; 1024];
        loop {
            let mut handles = [panel; 1];
            let sizes = match channel_read(panel, &mut bytes, &mut handles) {
                Ok(s) => s,
                Err((Status::PeerClosed, _)) => {
                    // No more launches: the registered ones stay.
                    let _ = port_unsubscribe(self.port, KEY_PANEL);
                    return;
                }
                Err(_) => return,
            };
            let got = &handles[..sizes.handles as usize];
            let msg = &bytes[..sizes.bytes as usize];
            let t = Header::decode(msg).map_or(0, |(h, _)| h.transaction);
            let ok = match decode::<pc::AddApplication<'_>>(msg, got.len()) {
                Ok((_, m)) if handle_type(got[0]) == Ok("channel") => {
                    let slot = self.apps.iter().position(Option::is_none).or_else(|| {
                        (self.apps.len() < MAX_APPS).then(|| {
                            self.apps.push(None);
                            self.apps.len() - 1
                        })
                    });
                    let wanted = channel_signals::READABLE | channel_signals::PEER_CLOSED;
                    match slot {
                        Some(i)
                            if port_subscribe(
                                self.port,
                                got[0],
                                wanted,
                                KEY_APP + i as u64,
                                Mode::Level,
                            )
                            .is_ok() =>
                        {
                            self.apps[i] = Some(Registered {
                                end: got[0],
                                tag: m.tag,
                                name: String::from(core::str::from_utf8(m.name.0).unwrap_or("")),
                                pending: None,
                            });
                            true
                        }
                        _ => false,
                    }
                }
                _ => false,
            };
            if !ok {
                for h in got {
                    let _ = handle_close(*h);
                }
            }
            let s = if ok {
                answer::CANCELLED
            } else {
                answer::INVALID
            };
            reply(panel, &pc::Added { status: Below(s) }, t, None);
        }
    }

    /// Requests from application `i`; its channel closed: it leaves.
    fn requests(&mut self, i: usize) {
        let mut bytes = vec![0u8; 1024];
        loop {
            let Some(r) = self.apps[i].as_ref() else {
                return;
            };
            let end = r.end;
            let mut handles = [end; 1];
            let sizes = match channel_read(end, &mut bytes, &mut handles) {
                Ok(s) => s,
                Err((Status::WouldBlock, _)) => return,
                Err(_) => {
                    self.leave(i);
                    return;
                }
            };
            for h in &handles[..sizes.handles as usize] {
                let _ = handle_close(*h);
            }
            let msg = &bytes[..sizes.bytes as usize];
            let t = Header::decode(msg).map_or(0, |(h, _)| h.transaction);
            let cancelled = |s: u32| pc::Cancelled { status: Below(s) };
            match ask_of(msg) {
                Some(Ok(ask)) => {
                    let Some(r) = self.apps[i].as_mut() else {
                        return;
                    };
                    if r.pending.is_some() {
                        reply(end, &cancelled(answer::BUSY), t, None);
                    } else {
                        r.pending = Some((t, ask));
                        self.queue.push_back(i);
                    }
                }
                _ => reply(end, &cancelled(answer::INVALID), t, None),
            }
        }
    }

    fn leave(&mut self, i: usize) {
        if let Some(r) = self.apps[i].take() {
            let _ = port_unsubscribe(self.port, KEY_APP + i as u64);
            let _ = handle_close(r.end);
        }
        self.queue.retain(|q| *q != i);
        if self.shown.as_ref().is_some_and(|(s, _)| *s == i)
            && let Some((_, sheet)) = self.shown.take()
        {
            self.app.close(sheet.window);
        }
    }

    /// Shows the next request's sheet, if none is shown.
    fn next(&mut self) {
        while self.shown.is_none() {
            let Some(i) = self.queue.pop_front() else {
                return;
            };
            let Some(r) = self.apps[i].as_ref() else {
                continue;
            };
            let Some((t, ask)) = r.pending.clone() else {
                continue;
            };
            let browser = Browser::new(ask, &r.name, &self.home);
            let (end, tag) = (r.end, r.tag);
            let start = clock_monotonic();
            match Sheet::open(&mut self.app, &mut self.fs, self.root, browser, tag) {
                Some(sheet) => {
                    // The round trip's first half (plan 7e's measurement):
                    // the request read to the sheet asked for, its folder
                    // listed and its first frame committed.
                    let us = (clock_monotonic() - start) / 1000;
                    println!("chooser: sheet in {us} us");
                    self.shown = Some((i, sheet));
                }
                None => {
                    // No window of it to hang the sheet under.
                    if let Some(r) = self.apps[i].as_mut() {
                        r.pending = None;
                    }
                    let c = pc::Cancelled {
                        status: Below(answer::FAILED),
                    };
                    reply(end, &c, t, None);
                }
            }
        }
    }

    fn compositor(&mut self) -> bool {
        while let Some(h) = self.app.poll() {
            if h == Happened::Gone {
                return false;
            }
            let Some((i, sheet)) = self.shown.as_mut() else {
                continue;
            };
            let i = *i;
            let start = clock_monotonic();
            let Some(outcome) = sheet.event(&mut self.app, &mut self.fs, self.root, h) else {
                continue;
            };
            let window = sheet.window;
            self.shown = None;
            self.app.close(window);
            let Some(r) = self.apps[i].as_mut() else {
                continue;
            };
            let Some((t, _)) = r.pending.take() else {
                continue;
            };
            let (end, tag) = (r.end, r.tag);
            match outcome {
                // A grant the frame could not show is not given: the pills
                // say what a window holds (review 7e, 9).
                Outcome::Chosen { handle, grant } if grant.pill.len() > gui::MAX_LINE => {
                    let _ = handle_close(handle);
                    let c = pc::Cancelled {
                        status: Below(answer::FAILED),
                    };
                    reply(end, &c, t, None);
                }
                Outcome::Chosen { handle, grant } => {
                    let m = pc::Chosen {
                        kind: Below(grant.kind),
                        name: Data(grant.shown.as_bytes()),
                        words: Data(grant.words.as_bytes()),
                    };
                    reply(end, &m, t, Some(handle));
                    // The frame's pills show it (ADR-0172 point 4).
                    let g = gui::Granted {
                        tag,
                        words: Data(grant.pill.as_bytes()),
                    };
                    let mut out = vec![0u8; 1024];
                    if let Ok(n) = encode(&g, 0, &mut out) {
                        let _ = zoraya_app::Platform::send(&mut self.app.platform, &out[..n], None);
                    }
                    // The second half: the confirming event to the
                    // connection made and the answer and the pill sent.
                    let us = (clock_monotonic() - start) / 1000;
                    println!("chooser: answer in {us} us");
                }
                Outcome::Cancelled(s) => {
                    reply(end, &pc::Cancelled { status: Below(s) }, t, None);
                }
            }
        }
        true
    }
}

#[unsafe(no_mangle)]
fn main() -> i32 {
    let channel = |slot: u64| Handle::new(slot).filter(|h| handle_type(*h) == Ok("channel"));
    let (Some(root), Some(compositor), Some(panel)) = (channel(5), channel(6), channel(7)) else {
        println!("chooser: missing the root, the compositor or the panel");
        return 1;
    };
    let Some(app) = App::connect(Gui::new(compositor), Appearance::Light) else {
        println!("chooser: no compositor");
        return 1;
    };
    let Ok(port) = port_create() else { return 1 };
    let wanted = channel_signals::READABLE | channel_signals::PEER_CLOSED;
    if port_subscribe(port, compositor, wanted, KEY_GUI, Mode::Level).is_err()
        || port_subscribe(port, panel, wanted, KEY_PANEL, Mode::Level).is_err()
    {
        return 1;
    }
    let mut fs = Client::new(Files);
    let home = zoraya_user::app::home(&mut fs, root);
    let mut c = Chooser {
        app,
        fs,
        root,
        home,
        port,
        apps: Vec::new(),
        queue: VecDeque::new(),
        shown: None,
    };
    loop {
        let mut packets = [Packet::default(); 8];
        let _ = port_wait(port, DEADLINE_INFINITE, &mut packets);
        if !c.compositor() {
            return 0;
        }
        c.register(panel);
        for i in 0..c.apps.len() {
            c.requests(i);
        }
        c.next();
        if !c.compositor() {
            return 0;
        }
    }
}
