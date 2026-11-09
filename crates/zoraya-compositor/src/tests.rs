//! The compositor against a fake platform: clients, drivers and memory are
//! plain data, and every hostile case of ADR-0093 and ADR-0094 is a test.

use std::cell::Cell;
use std::collections::{BTreeMap, VecDeque};
use std::vec;
use std::vec::Vec;

use super::*;
use zoraya_protocol::{Seq, WireError};

const W: u32 = 640;
const H: u32 = 480;
const FRAMEBUFFER: u32 = 999;
/// The first window's body at scale 1: the frame at `ORIGIN` below the
/// 28-pixel bar, inside a 1-pixel border and under a 36-pixel title bar.
const BX: i64 = 73;
const BY: i64 = 106;

#[derive(Default)]
struct Peer {
    /// Messages to the compositor.
    inbox: VecDeque<(Vec<u8>, Vec<u32>)>,
    /// Messages from the compositor.
    outbox: Vec<Vec<u8>>,
    /// How many messages the peer's queue holds before `Full`.
    room: usize,
    /// The peer closed its end.
    gone: bool,
}

struct Memory {
    pixels: Vec<u32>,
    /// Bytes in the object.
    size: u64,
    /// Every read sees new values: a client rewriting its buffer.
    changing: bool,
    reads: Cell<u64>,
}

#[derive(Default)]
struct Fake {
    peers: BTreeMap<u32, Peer>,
    memory: BTreeMap<u32, Memory>,
    closed: Vec<u32>,
    watched: BTreeMap<u64, u32>,
    mapped: BTreeMap<u64, u32>,
    frame: Vec<u32>,
    /// The display driver: its size, whether it answers, the flushes seen.
    display: Option<u32>,
    display_size: (u32, u32),
    display_dead: bool,
    /// The display driver's queue is full: flushes cannot be sent.
    display_full: bool,
    /// What `Info` answers, and whether `Attach` gets no reply.
    info_status: u32,
    info_format: u32,
    refuse_attach: bool,
    flushes: Vec<(u32, u32, u32, u32)>,
    attaches: u32,
    clock: u64,
    next: u32,
}

impl Fake {
    fn new() -> Fake {
        Fake {
            display_size: (W, H),
            info_format: display::FORMAT_XRGB,
            next: 100,
            ..Fake::default()
        }
    }

    fn channel(&mut self, room: usize) -> u32 {
        self.next += 1;
        self.peers.insert(
            self.next,
            Peer {
                room,
                ..Peer::default()
            },
        );
        self.next
    }

    /// A memory object: whole pages, as the kernel makes them.
    fn memory(&mut self, pixels: Vec<u32>, size: u64, changing: bool) -> u32 {
        let size = size.checked_next_multiple_of(4096).unwrap_or(size);
        self.next += 1;
        self.memory.insert(
            self.next,
            Memory {
                pixels,
                size,
                changing,
                reads: Cell::new(0),
            },
        );
        self.next
    }

    fn send(&mut self, end: u32, bytes: Vec<u8>, handles: Vec<u32>) {
        self.peers
            .get_mut(&end)
            .unwrap()
            .inbox
            .push_back((bytes, handles));
    }

    fn take(&mut self, end: u32) -> Vec<Vec<u8>> {
        core::mem::take(&mut self.peers.get_mut(&end).unwrap().outbox)
    }
}

impl Platform for Fake {
    type End = u32;

    fn read(&mut self, end: u32, bytes: &mut [u8], handles: &mut Vec<u32>) -> Incoming {
        let Some(peer) = self.peers.get_mut(&end) else {
            return Incoming::Closed;
        };
        match peer.inbox.pop_front() {
            Some((m, h)) if m.len() > bytes.len() || h.len() > 64 => Incoming::Refused,
            Some((m, h)) => {
                bytes[..m.len()].copy_from_slice(&m);
                handles.extend(h);
                Incoming::Message(m.len())
            }
            None if peer.gone => Incoming::Closed,
            None => Incoming::Empty,
        }
    }

    fn write(&mut self, end: u32, bytes: &[u8], handles: &[u32]) -> Outgoing {
        assert!(
            handles.is_empty(),
            "the compositor sends no client a handle"
        );
        // The display driver: a flush is answered at once, unless it hangs.
        if Some(end) == self.display && !self.peers[&end].gone {
            if self.display_full {
                return Outgoing::Full;
            }
            let (header, _) = Header::decode(bytes).unwrap();
            assert_eq!(header.ordinal, 3, "only flushes are sent unasked");
            let (_, f) = decode::<display::Flush>(bytes, 0).unwrap();
            self.flushes.push((f.x, f.y, f.width, f.height));
            if !self.display_dead {
                let mut reply = vec![0; 64];
                let n = encode(
                    &display::Done { status: Below(0) },
                    header.transaction,
                    &mut reply,
                )
                .unwrap();
                reply.truncate(n);
                self.peers
                    .get_mut(&end)
                    .unwrap()
                    .inbox
                    .push_back((reply, vec![]));
            }
            return Outgoing::Sent;
        }
        let peer = self.peers.get_mut(&end).unwrap();
        if peer.gone {
            return Outgoing::Closed;
        }
        if peer.outbox.len() >= peer.room {
            return Outgoing::Full;
        }
        peer.outbox.push(bytes.to_vec());
        Outgoing::Sent
    }

    fn close(&mut self, end: u32) {
        assert!(!self.closed.contains(&end), "{end} closed twice");
        self.closed.push(end);
        self.watched.retain(|_, e| *e != end);
    }

    fn kind(&mut self, end: u32) -> Kind {
        if self.peers.contains_key(&end) {
            Kind::Channel
        } else if self.memory.contains_key(&end) {
            Kind::Memory
        } else {
            Kind::Other
        }
    }

    fn watch(&mut self, end: u32, key: u64) -> bool {
        self.watched.insert(key, end);
        true
    }

    fn map(&mut self, memory: u32, length: u64) -> Option<u64> {
        let m = self.memory.get(&memory)?;
        if length > m.size {
            return None;
        }
        let address = u64::from(memory) << 32;
        self.mapped.insert(address, memory);
        Some(address)
    }

    fn unmap(&mut self, address: u64, _length: u64) {
        assert!(self.mapped.remove(&address).is_some());
    }

    fn copy_from(&mut self, address: u64, out: &mut [u32]) -> bool {
        let id = (address >> 32) as u32;
        let at = (address & 0xffff_ffff) as usize / 4;
        let Some(m) = self.memory.get_mut(&id) else {
            return false;
        };
        for (i, o) in out.iter_mut().enumerate() {
            let n = m.reads.get() + 1;
            m.reads.set(n);
            *o = if m.changing {
                n as u32
            } else {
                m.pixels[at + i]
            };
        }
        true
    }

    fn framebuffer(&mut self, width: u32, height: u32) -> Option<u32> {
        if self.frame.len() != (width * height) as usize {
            self.frame = vec![0; (width * height) as usize];
        }
        Some(FRAMEBUFFER)
    }

    fn pixels(&mut self) -> &mut [u32] {
        &mut self.frame
    }

    fn call(
        &mut self,
        end: u32,
        message: &[u8],
        handles: &[u32],
        reply: &mut [u8],
    ) -> Option<usize> {
        assert_eq!(Some(end), self.display);
        if self.display_dead {
            return None;
        }
        let (header, _) = Header::decode(message).unwrap();
        let t = header.transaction;
        let n = match header.ordinal {
            1 => encode(
                &display::InfoReply {
                    status: Below(self.info_status),
                    width: self.display_size.0,
                    height: self.display_size.1,
                    format: self.info_format,
                },
                t,
                reply,
            ),
            2 if self.refuse_attach => return None,
            2 => {
                assert_eq!(handles, [FRAMEBUFFER]);
                self.attaches += 1;
                encode(&display::Done { status: Below(0) }, t, reply)
            }
            _ => panic!("unexpected display call: flushes do not wait"),
        };
        n.ok()
    }

    fn wait(&mut self, ready: &mut Vec<u64>, deadline: Option<u64>) {
        for (key, end) in &self.watched {
            if let Some(p) = self.peers.get(end)
                && (!p.inbox.is_empty() || p.gone)
            {
                ready.push(*key);
            }
        }
        // Nothing ready: time passes to the deadline.
        if ready.is_empty()
            && let Some(d) = deadline
        {
            self.clock = self.clock.max(d);
        }
    }

    fn now(&mut self) -> u64 {
        self.clock += 1000;
        self.clock
    }
}

struct Setup {
    c: Compositor<Fake>,
    manager: u32,
    keyboard: u32,
    tablet: u32,
}

fn setup() -> Setup {
    setup_with(|_| {})
}

/// A compositor whose fake is changed by `change` before it starts.
fn setup_with(change: impl FnOnce(&mut Fake)) -> Setup {
    let mut f = Fake::new();
    change(&mut f);
    let manager = f.channel(64);
    let display = f.channel(64);
    let keyboard = f.channel(64);
    let tablet = f.channel(64);
    f.display = Some(display);
    let c = Compositor::new(f, manager, Some(display), &[keyboard, tablet]);
    Setup {
        c,
        manager,
        keyboard,
        tablet,
    }
}

fn message<'a, M: Message<'a>>(m: &M, transaction: u32) -> Vec<u8> {
    let mut out = vec![0; 4096];
    let n = encode(m, transaction, &mut out).unwrap();
    out.truncate(n);
    out
}

impl Setup {
    fn f(&mut self) -> &mut Fake {
        self.c.platform()
    }

    /// A client added through the manager, with `room` in its queue.
    fn client(&mut self, room: usize) -> u32 {
        let end = self.f().channel(room);
        let m = self.manager;
        self.f().send(m, message(&gui::AddClient {}, 9), vec![end]);
        self.c.step();
        let replies = self.f().take(m);
        let (_, done) = decode::<gui::ManagerDone>(replies.last().unwrap(), 0).unwrap();
        assert_eq!(done.status.0, status::OK);
        end
    }

    /// A client with a surface of `w × h` filled with `colour`.
    fn window(&mut self, w: u32, h: u32, colour: u32) -> (u32, u32) {
        let end = self.client(64);
        let memory = self
            .f()
            .memory(vec![colour; (w * h) as usize], u64::from(w * h * 4), false);
        assert_eq!(self.create(end, w, h, memory), status::OK);
        (end, memory)
    }

    fn create(&mut self, end: u32, w: u32, h: u32, memory: u32) -> u32 {
        let m = gui::Create {
            width: w,
            height: h,
        };
        self.f().send(end, message(&m, 2), vec![memory]);
        self.c.step();
        let replies = self.f().take(end);
        let done = replies
            .iter()
            .find_map(|r| decode::<gui::Done>(r, 0).ok())
            .expect("a Done reply");
        done.1.status.0
    }

    fn commit(&mut self, end: u32, x: u32, y: u32, w: u32, h: u32) -> u32 {
        let m = gui::Commit {
            x,
            y,
            width: w,
            height: h,
        };
        self.f().send(end, message(&m, 3), vec![]);
        self.c.step();
        self.frame_done(end).expect("a FrameDone")
    }

    fn frame_done(&mut self, end: u32) -> Option<u32> {
        let replies = self.f().take(end);
        replies
            .iter()
            .find_map(|r| decode::<gui::FrameDone>(r, 0).ok())
            .map(|(_, d)| d.status.0)
    }

    fn keys(&mut self, codes: &[(u32, u32)]) {
        let events: Vec<input::Event> = codes
            .iter()
            .map(|(code, value)| input::Event {
                kind: Below(input::kind::KEY),
                code: *code,
                value: *value,
            })
            .collect();
        let k = self.keyboard;
        let m = input::Frame {
            dropped: 0,
            events: Seq::Of(&events),
        };
        self.f().send(k, message(&m, 0), vec![]);
        self.c.step();
    }

    fn point(&mut self, x: u32, y: u32, button: Option<u32>) {
        let mut events = vec![
            input::Event {
                kind: Below(input::kind::AXIS),
                code: 0,
                value: x,
            },
            input::Event {
                kind: Below(input::kind::AXIS),
                code: 1,
                value: y,
            },
        ];
        if let Some(value) = button {
            events.push(input::Event {
                kind: Below(input::kind::KEY),
                code: code::BTN_LEFT,
                value,
            });
        }
        let t = self.tablet;
        let m = input::Frame {
            dropped: 0,
            events: Seq::Of(&events),
        };
        self.f().send(t, message(&m, 0), vec![]);
        self.c.step();
    }

    fn pixel(&mut self, x: i64, y: i64) -> u32 {
        self.f().frame[(y * i64::from(W) + x) as usize]
    }

    /// The wallpaper's pixel: what is shown where nothing else is.
    fn paper(&self, x: i64, y: i64) -> u32 {
        self.c.wallpaper[(y * i64::from(W) + x) as usize]
    }
}

/// Every event a client got, as (ordinal, bytes).
fn events(out: &[Vec<u8>]) -> Vec<(u32, Vec<u8>)> {
    out.iter()
        .map(|m| (Header::decode(m).unwrap().0.ordinal, m.clone()))
        .filter(|(o, _)| gui::CLIENT_EVENTS.contains(o))
        .collect()
}

fn keys_of(out: &[Vec<u8>]) -> Vec<(u32, bool, Option<char>)> {
    out.iter()
        .filter_map(|m| decode::<gui::Key>(m, 0).ok())
        .map(|(_, k)| {
            (
                k.code,
                k.pressed,
                char::from_u32(k.text.0).filter(|c| *c != '\0'),
            )
        })
        .collect()
}

#[test]
fn rect_arithmetic() {
    let a = Rect::new(0, 0, 10, 10);
    let b = Rect::new(5, 5, 10, 10);
    assert_eq!(a.intersect(&b), Rect::new(5, 5, 5, 5));
    assert_eq!(a.union(&b), Rect::new(0, 0, 15, 15));
    assert!(a.intersect(&Rect::new(10, 0, 5, 5)).is_empty());
    assert_eq!(Rect::default().union(&b), b);
    assert_eq!(b.union(&Rect::default()), b);
    assert!(a.contains(9, 9) && !a.contains(10, 9) && !a.contains(-1, 0));
    assert_eq!(Rect::from_client(0, 0, 10, 10, 10, 10), Some(a));
    for (x, y, w, h) in [
        (0, 0, 0, 1),
        (0, 0, 11, 1),
        (10, 0, 1, 1),
        (u32::MAX, 0, 1, 1),
        (0, 0, u32::MAX, u32::MAX),
    ] {
        assert_eq!(Rect::from_client(x, y, w, h, 10, 10), None);
    }
}

#[test]
fn the_display_is_attached_at_start() {
    let mut s = setup();
    assert_eq!(s.c.size(), (W, H));
    assert_eq!(s.f().attaches, 1);
    // The first round shows the whole screen once.
    s.c.step();
    assert_eq!(s.f().flushes, [(0, 0, W, H)]);
    assert_eq!(s.pixel(5, i64::from(H) - 5), s.paper(5, i64::from(H) - 5));
}

#[test]
fn a_window_appears_after_its_commit() {
    let mut s = setup();
    let end = s.client(64);
    s.f().send(end, message(&gui::Display {}, 1), vec![]);
    s.c.step();
    let (_, d) = decode::<gui::DisplayReply>(&s.f().take(end)[0], 0).unwrap();
    assert_eq!((d.status.0, d.width, d.height), (status::OK, W, H));
    let memory = s
        .f()
        .memory(vec![0x0012_3456; 100 * 50], 100 * 50 * 4, false);
    assert_eq!(s.create(end, 100, 50, memory), status::OK);
    let area = s.c.placement(0).unwrap();
    assert_eq!((area.x, area.y), (BX, BY));
    // Created, not yet committed: the compositor's copy is black.
    assert_eq!(s.pixel(BX, BY), 0);
    let border = zoraya_tokens::LIGHT.border.pixel();
    assert_eq!(s.pixel(BX - 1, BY + 10), border);
    s.f().flushes.clear();
    assert_eq!(s.commit(end, 0, 0, 100, 50), status::OK);
    assert_eq!(s.pixel(BX, BY), 0x0012_3456);
    assert_eq!(s.pixel(BX + 50, BY + 25), 0x0012_3456);
    assert_eq!(s.pixel(BX + 100, BY + 10), border);
    assert_eq!(s.f().flushes, [(73, 106, 100, 50)]);
    assert_eq!(s.c.stats().commits, 1);
    // Damage of part of it flushes that part.
    s.f().flushes.clear();
    assert_eq!(s.commit(end, 10, 20, 5, 3), status::OK);
    assert_eq!(s.f().flushes, [(83, 126, 5, 3)]);
}

/// INV-0093-3.
#[test]
fn hostile_surfaces_and_damage_are_refused_and_others_go_on() {
    let mut s = setup();
    let (good, _) = s.window(20, 20, 7);
    let end = s.client(64);
    // A commit before any surface.
    assert_eq!(s.commit(end, 0, 0, 1, 1), status::NO_SURFACE);
    // Sizes over the display, zero, absurd.
    for (w, h) in [
        (W + 1, 10),
        (10, H + 1),
        (0, 10),
        (10, 0),
        (u32::MAX, u32::MAX),
    ] {
        let memory = s.f().memory(vec![], u64::MAX, false);
        assert_eq!(s.create(end, w, h, memory), status::BAD_BUFFER, "{w}x{h}");
        assert!(s.f().closed.contains(&memory));
    }
    // A memory object smaller than declared.
    let small = s.f().memory(vec![0; 10], 4096, false);
    assert_eq!(s.create(end, 100, 100, small), status::BAD_BUFFER);
    // Not a memory object at all.
    let channel = s.f().channel(1);
    assert_eq!(s.create(end, 10, 10, channel), status::BAD_BUFFER);
    // Create with no handle, or two.
    let m = gui::Create {
        width: 10,
        height: 10,
    };
    s.f().send(end, message(&m, 2), vec![]);
    s.c.step();
    assert!(
        decode::<gui::Done>(&s.f().take(end)[0], 0)
            .is_ok_and(|(_, d)| d.status.0 == status::BAD_BUFFER)
    );
    let (a, b) = (
        s.f().memory(vec![0; 400], 1600, false),
        s.f().memory(vec![0; 400], 1600, false),
    );
    s.f().send(end, message(&m, 2), vec![a, b]);
    s.c.step();
    assert!(s.f().closed.contains(&a) && s.f().closed.contains(&b));
    s.f().take(end);
    // A real surface, then damage outside it.
    let ok = s.f().memory(vec![1; 100], 400, false);
    assert_eq!(s.create(end, 10, 10, ok), status::OK);
    let again = s.f().memory(vec![1; 100], 400, false);
    assert_eq!(s.create(end, 10, 10, again), status::EXISTS);
    for (x, y, w, h) in [
        (0, 0, 11, 1),
        (10, 0, 1, 1),
        (0, 0, 0, 0),
        (u32::MAX, u32::MAX, 2, 2),
        (5, 5, 6, 1),
    ] {
        assert_eq!(s.commit(end, x, y, w, h), status::BAD_RANGE);
    }
    // The good client is served throughout.
    assert_eq!(s.commit(good, 0, 0, 20, 20), status::OK);
    assert_eq!(s.pixel(BX, BY), 7);
}

/// INV-0093-2: the manager's and drivers' ordinals mean nothing on a client
/// channel, and the other client hears nothing of them.
#[test]
fn a_client_cannot_use_manager_or_input_messages() {
    let mut s = setup();
    let (watcher, _) = s.window(10, 10, 1);
    s.f().take(watcher);
    let (end, _) = s.window(10, 10, 2);
    s.f().take(end);
    let spares = [s.f().channel(8), s.f().channel(8), s.f().channel(8)];
    let spare = spares[0];
    let attempts = [
        (message(&gui::AddClient {}, 5), vec![spares[0]]),
        (message(&gui::AttachDisplay {}, 5), vec![spares[1]]),
        (message(&gui::AttachInput {}, 5), vec![spares[2]]),
        (message(&gui::Stats {}, 5), vec![]),
        (
            message(
                &input::Frame {
                    dropped: 0,
                    events: Seq::Of(&[input::Event {
                        kind: Below(input::kind::KEY),
                        code: 30,
                        value: 1,
                    }]),
                },
                5,
            ),
            vec![],
        ),
        (
            message(
                &gui::Key {
                    dropped: 0,
                    code: 30,
                    pressed: true,
                    modifiers: Flags(0),
                    text: Below('a' as u32),
                },
                5,
            ),
            vec![],
        ),
    ];
    for (bytes, handles) in attempts {
        s.f().send(end, bytes, handles);
        s.c.step();
        let replies = s.f().take(end);
        let (_, e) = decode::<ErrorReply>(&replies[0], 0).unwrap();
        assert_eq!(e.status, status::UNSUPPORTED);
    }
    assert!(
        spares.iter().all(|e| s.f().closed.contains(e)),
        "what a client sent is closed, not used"
    );
    let _ = spare;
    assert_eq!(s.c.client_count(), 2);
    assert!(events(&s.f().take(watcher)).is_empty());
    let unseen = s.f().channel(1);
    assert!(!s.f().watched.values().any(|e| *e == unseen));
}

/// INV-0093-4: client memory is read once per pixel of a commit, and the
/// screen holds what was copied, whatever the client writes after.
#[test]
fn shared_pixels_are_read_once_and_composed_from_the_copy() {
    let mut s = setup();
    let end = s.client(64);
    let memory = s.f().memory(vec![], 32 * 32 * 4, true);
    assert_eq!(s.create(end, 32, 32, memory), status::OK);
    assert_eq!(s.commit(end, 0, 0, 32, 32), status::OK);
    assert_eq!(s.f().memory[&memory].reads.get(), 1024);
    // The upper half, above the round lower corners.
    let shown: Vec<u32> = (0..16)
        .flat_map(|y| (0..32).map(move |x| (x, y)))
        .map(|(x, y)| s.pixel(BX + x, BY + y))
        .collect();
    assert_eq!(shown, (1..=512).collect::<Vec<u32>>());
    // A redraw for another reason (a second window on top, then gone)
    // composes from the copy and reads no client memory.
    let spot = (BX + (CASCADE as i64) + 2, BY + (CASCADE as i64) + 2);
    let before = s.pixel(spot.0, spot.1);
    let (other, _) = s.window(8, 8, 5);
    let reads = s.f().memory[&memory].reads.get();
    s.f().peers.get_mut(&other).unwrap().gone = true;
    s.c.step();
    assert_eq!(s.f().memory[&memory].reads.get(), reads);
    assert_eq!(s.pixel(spot.0, spot.1), before);
    assert_eq!(s.pixel(BX + 15, BY + 15), 15 * 32 + 16);
}

/// INV-0093-5: a client that never reads, or floods commits, stops nobody.
#[test]
fn a_flooding_or_deaf_client_does_not_stop_others() {
    let mut s = setup();
    let (good, _) = s.window(10, 10, 3);
    // Deaf: its queue holds nothing.
    let deaf = s.client(64);
    let memory = s.f().memory(vec![9; 100], 400, false);
    assert_eq!(s.create(deaf, 10, 10, memory), status::OK);
    s.f().peers.get_mut(&deaf).unwrap().room = 0;
    // Flooding: a hundred commits queued at once.
    let m = gui::Commit {
        x: 0,
        y: 0,
        width: 10,
        height: 10,
    };
    for _ in 0..100 {
        s.f().send(deaf, message(&m, 3), vec![]);
    }
    for round in 0..10 {
        let commit = gui::Commit {
            x: 0,
            y: 0,
            width: 1,
            height: 1,
        };
        s.f().send(good, message(&commit, 3), vec![]);
        s.c.step();
        assert_eq!(s.frame_done(good), Some(status::OK), "round {round}");
    }
    // One of the flood per round, no more.
    assert_eq!(s.f().peers[&deaf].inbox.len(), 90);
}

/// INV-0093-1 and INV-0094-1: keys go to the focused client only, and F12
/// moves focus without reaching anyone.
#[test]
fn keys_go_to_the_focused_client_and_f12_switches() {
    let mut s = setup();
    let (a, _) = s.window(40, 40, 1);
    let (b, _) = s.window(40, 40, 2);
    s.f().take(a);
    s.f().take(b);
    assert_eq!(s.c.focus(), Some(0), "the first window kept focus");
    s.keys(&[(30, 1), (30, 0)]);
    assert_eq!(
        keys_of(&s.f().take(a)),
        [(30, true, Some('a')), (30, false, None)]
    );
    assert!(events(&s.f().take(b)).is_empty());
    s.keys(&[(code::F12, 1), (code::F12, 2), (code::F12, 0)]);
    assert_eq!(s.c.focus(), Some(1));
    let to_a = s.f().take(a);
    let to_b = s.f().take(b);
    assert!(
        keys_of(&to_a).is_empty() && keys_of(&to_b).is_empty(),
        "F12 reached nobody"
    );
    assert!(
        to_a.iter()
            .any(|m| decode::<gui::Focus>(m, 0).is_ok_and(|(_, f)| !f.focused))
    );
    assert!(
        to_b.iter()
            .any(|m| decode::<gui::Focus>(m, 0).is_ok_and(|(_, f)| f.focused))
    );
    s.keys(&[(code::LEFT_SHIFT, 1), (48, 1)]);
    assert_eq!(
        keys_of(&s.f().take(b)),
        [(code::LEFT_SHIFT, true, None), (48, true, Some('B'))]
    );
    assert!(keys_of(&s.f().take(a)).is_empty());
    // F12 wraps around.
    s.keys(&[(code::F12, 1), (code::F12, 0)]);
    assert_eq!(s.c.focus(), Some(0));
}

/// INV-0093-1: pointer events go to the surface under the pointer, in its
/// own coordinates; a click focuses and raises.
#[test]
fn pointer_events_go_only_to_the_surface_under_it() {
    let mut s = setup();
    let (a, _) = s.window(60, 60, 1);
    let (b, _) = s.window(60, 60, 2);
    s.f().take(a);
    s.f().take(b);
    // Axis values are scaled to the screen: the B window's interior.
    let scale = |p: i64, side: u32| (p * 65535 / i64::from(side - 1)) as u32;
    let (bx, by) = (BX + (CASCADE as i64) + 40, BY + (CASCADE as i64) + 40);
    s.point(scale(bx, W), scale(by, H), None);
    let got: Vec<_> = s
        .f()
        .take(b)
        .iter()
        .filter_map(|m| decode::<gui::Pointer>(m, 0).ok())
        .map(|(_, p)| (p.x, p.y))
        .collect();
    assert_eq!(got.len(), 1);
    let (px, py) = got[0];
    assert!(
        (39..=40).contains(&px) && (39..=40).contains(&py),
        "{px},{py}"
    );
    assert!(events(&s.f().take(a)).is_empty());
    // Over A only (outside B): A hears, B does not.
    s.point(scale(BX + 2, W), scale(BY + 2, H), Some(1));
    assert!(
        s.f()
            .take(a)
            .iter()
            .any(|m| decode::<gui::Pointer>(m, 0).is_ok())
    );
    assert!(
        s.f()
            .take(b)
            .iter()
            .all(|m| decode::<gui::Pointer>(m, 0).is_err())
    );
    assert_eq!(s.c.focus(), Some(0), "a click focuses");
    // Outside every surface: nothing, and no focus change.
    s.point(
        scale(i64::from(W) - 2, W),
        scale(i64::from(H) - 2, H),
        Some(1),
    );
    assert!(events(&s.f().take(a)).is_empty() && events(&s.f().take(b)).is_empty());
    assert_eq!(s.c.focus(), Some(0));
    // The pointer is drawn, and moving it repairs what was under it.
    let under = s.pixel(1, 2);
    s.point(0, 0, None);
    assert_eq!(s.pixel(0, 0), zoraya_tokens::LIGHT.fg_text.pixel());
    assert_eq!(s.pixel(1, 2), zoraya_tokens::LIGHT.bg_control.pixel());
    s.point(65535, 65535, None);
    assert_eq!(s.pixel(1, 2), under);
}

/// INV-0094-2: no client message changes focus.
#[test]
fn clients_cannot_take_focus() {
    let mut s = setup();
    let (_a, _) = s.window(10, 10, 1);
    let (b, _) = s.window(10, 10, 2);
    let tries = [
        message(&gui::Display {}, 1),
        message(
            &gui::Commit {
                x: 0,
                y: 0,
                width: 1,
                height: 1,
            },
            3,
        ),
        message(
            &gui::Focus {
                dropped: 0,
                focused: true,
            },
            1,
        ),
        message(
            &gui::Pointer {
                dropped: 0,
                x: 1,
                y: 1,
                buttons: Flags(1),
            },
            1,
        ),
        vec![1, 2, 3],
    ];
    for t in tries {
        s.f().send(b, t, vec![]);
        s.c.step();
        assert_eq!(s.c.focus(), Some(0));
    }
}

/// INV-0094-3: a full queue loses that client's events, counted, and the
/// count arrives with the next that fits.
#[test]
fn a_full_client_loses_only_its_own_events() {
    let mut s = setup();
    let (a, _) = s.window(10, 10, 1);
    s.f().take(a);
    s.f().peers.get_mut(&a).unwrap().room = 0;
    for _ in 0..5 {
        s.keys(&[(30, 1), (30, 0)]);
    }
    s.f().peers.get_mut(&a).unwrap().room = 64;
    s.keys(&[(31, 1)]);
    let got: Vec<u32> = s
        .f()
        .take(a)
        .iter()
        .filter_map(|m| decode::<gui::Key>(m, 0).ok())
        .map(|(_, k)| k.dropped)
        .collect();
    assert_eq!(got, [10]);
}

#[test]
fn a_closed_client_leaves_and_focus_moves_to_the_top() {
    let mut s = setup();
    s.c.step();
    let before = s.pixel(BX + 5, BY + 5);
    let (a, memory) = s.window(10, 10, 1);
    let (b, _) = s.window(10, 10, 2);
    s.f().take(b);
    s.f().peers.get_mut(&a).unwrap().gone = true;
    s.c.step();
    assert_eq!(s.c.client_count(), 1);
    assert!(s.f().closed.contains(&a) && s.f().closed.contains(&memory));
    assert!(s.f().mapped.len() == 1, "its mapping went");
    assert_eq!(s.c.focus(), Some(1));
    assert!(
        s.f()
            .take(b)
            .iter()
            .any(|m| decode::<gui::Focus>(m, 0).is_ok_and(|(_, f)| f.focused))
    );
    assert_eq!(s.pixel(BX + 5, BY + 5), before);
}

#[test]
fn clients_past_the_limit_are_refused() {
    let mut s = setup();
    for _ in 0..MAX_CLIENTS {
        s.client(8);
    }
    let end = s.f().channel(8);
    let m = s.manager;
    s.f().send(m, message(&gui::AddClient {}, 4), vec![end]);
    s.c.step();
    let (_, d) = decode::<gui::ManagerDone>(&s.f().take(m)[0], 0).unwrap();
    assert_eq!(d.status.0, status::NO_MEMORY);
    assert!(s.f().closed.contains(&end));
}

#[test]
fn a_message_too_large_closes_its_client() {
    let mut s = setup();
    let end = s.client(8);
    s.f().send(end, vec![0; 8192], vec![]);
    s.c.step();
    assert_eq!(s.c.client_count(), 0);
    assert!(s.f().closed.contains(&end));
}

#[test]
fn the_manager_refuses_wrong_handles() {
    let mut s = setup();
    let m = s.manager;
    let memory = s.f().memory(vec![], 4096, false);
    for bytes in [
        message(&gui::AddClient {}, 4),
        message(&gui::AttachInput {}, 4),
        message(&gui::AttachDisplay {}, 4),
    ] {
        let h = s.f().memory(vec![], 4096, false);
        s.f().send(m, bytes, vec![h]);
        s.c.step();
        let (_, d) = decode::<gui::ManagerDone>(&s.f().take(m)[0], 0).unwrap();
        assert_ne!(d.status.0, status::OK);
        assert!(s.f().closed.contains(&h));
    }
    s.f().send(m, message(&gui::AddClient {}, 4), vec![]);
    s.f().send(m, message(&gui::Stats {}, 4), vec![memory]);
    s.c.step();
    s.c.step();
    assert!(s.f().closed.contains(&memory));
    assert_eq!(s.c.client_count(), 0);
}

#[test]
fn a_lost_display_is_replaced_and_the_screen_shown_again() {
    let mut s = setup();
    let (a, _) = s.window(10, 10, 4);
    let old = hang(&mut s, a);
    assert!(s.f().closed.contains(&old));
    let m = s.manager;
    s.f().take(m);
    // A new driver through the manager: attached, and everything flushed.
    let new = s.f().channel(8);
    s.f().display = Some(new);
    s.f().display_dead = false;
    s.f().flushes.clear();
    let m = s.manager;
    s.f().send(m, message(&gui::AttachDisplay {}, 7), vec![new]);
    s.c.step();
    let (_, d) = decode::<gui::ManagerDone>(&s.f().take(m)[0], 0).unwrap();
    assert_eq!(d.status.0, status::OK);
    assert_eq!(s.f().attaches, 2);
    assert_eq!(s.f().flushes, [(0, 0, W, H)]);
    assert_eq!(s.pixel(BX, BY), 4);
    // A second display while one is attached is refused.
    let extra = s.f().channel(8);
    s.f()
        .send(m, message(&gui::AttachDisplay {}, 7), vec![extra]);
    s.c.step();
    let (_, d) = decode::<gui::ManagerDone>(&s.f().take(m)[0], 0).unwrap();
    assert_eq!(d.status.0, status::EXISTS);
}

#[test]
fn a_lost_input_driver_is_dropped_and_a_new_one_attached() {
    let mut s = setup();
    let (a, _) = s.window(10, 10, 1);
    s.f().take(a);
    let k = s.keyboard;
    s.f().peers.get_mut(&k).unwrap().gone = true;
    s.c.step();
    assert!(s.f().closed.contains(&k));
    let new = s.f().channel(8);
    let m = s.manager;
    s.f().send(m, message(&gui::AttachInput {}, 7), vec![new]);
    s.c.step();
    s.keyboard = new;
    s.keys(&[(30, 1)]);
    assert_eq!(keys_of(&s.f().take(a)), [(30, true, Some('a'))]);
}

#[test]
fn invalid_input_events_are_ignored() {
    let mut s = setup();
    let (a, _) = s.window(10, 10, 1);
    s.f().take(a);
    let bad = [
        input::Event {
            kind: Below(input::kind::KEY),
            code: 0x300,
            value: 1,
        },
        input::Event {
            kind: Below(input::kind::KEY),
            code: 30,
            value: 3,
        },
        input::Event {
            kind: Below(input::kind::AXIS),
            code: 2,
            value: 0,
        },
        input::Event {
            kind: Below(input::kind::AXIS),
            code: 0,
            value: 70000,
        },
    ];
    let k = s.keyboard;
    s.f().send(
        k,
        message(
            &input::Frame {
                dropped: 0,
                events: Seq::Of(&bad),
            },
            0,
        ),
        vec![],
    );
    s.f().send(k, vec![9; 40], vec![]);
    s.c.step();
    assert!(events(&s.f().take(a)).is_empty());
    assert!(!s.f().closed.contains(&k));
}

#[test]
fn stats_count_frames_and_commits() {
    let mut s = setup();
    let (a, _) = s.window(10, 10, 1);
    s.commit(a, 0, 0, 10, 10);
    let m = s.manager;
    s.f().send(m, message(&gui::Stats {}, 4), vec![]);
    s.c.step();
    let (_, r) = decode::<gui::StatsReply>(&s.f().take(m)[0], 0).unwrap();
    assert_eq!(r.commits, 1);
    assert!(r.frames >= 2 && r.compose_ns > 0 && r.flush_ns > 0);
    // The manager is read before the display's last answer in a round.
    assert!(r.flushes <= r.frames && r.flushes + 1 >= r.frames);
    assert_eq!(r.hangs, 0);
}

#[test]
fn windows_are_cascaded_and_kept_on_screen() {
    let mut s = setup();
    // Larger than the work area: at the corner, the title bar under the bar.
    s.window(W, H, 1);
    assert_eq!(s.c.placement(0).map(|r| (r.x, r.y)), Some((1, 64)));
    s.window(W - 10, H - 10, 1);
    // Its title bar stays on the screen.
    let r = s.c.placement(1).unwrap();
    assert!(r.x >= 1 && r.right() <= i64::from(W) && r.y - 36 >= 28);
    s.window(10, 10, 1);
    assert_eq!(
        s.c.placement(2).map(|r| (r.x, r.y)),
        Some((BX + 2 * (CASCADE as i64), BY + 2 * (CASCADE as i64)))
    );
}

#[test]
fn wire_errors_are_not_panics() {
    // A Create whose body is short is refused, not a panic.
    let mut s = setup();
    let end = s.client(8);
    let mut bytes = message(
        &gui::Create {
            width: 1,
            height: 1,
        },
        2,
    );
    bytes.truncate(20);
    bytes[12..16].copy_from_slice(&4u32.to_le_bytes());
    s.f().send(end, bytes, vec![]);
    s.c.step();
    assert!(s.c.client_count() == 1);
    let _ = WireError::Truncated;
}

// Holes cargo-mutants found (TASK-14 review).

#[test]
fn the_frame_is_the_compositors_drawing() {
    // ADR-0160's frame around a 100 × 50 body: the title bar above in
    // bg.frame.focused, a one-pixel border all round, the shadow outside.
    let mut s = setup();
    let (end, _) = s.window(100, 50, 7);
    s.commit(end, 0, 0, 100, 50);
    let (x, y) = (BX, BY);
    let p = zoraya_tokens::LIGHT;
    assert_eq!(s.pixel(x + 60, y - 30), p.bg_frame_focused.pixel());
    assert_eq!(s.pixel(x - 1, y + 10), p.border.pixel());
    assert_eq!(s.pixel(x + 100, y + 10), p.border.pixel());
    assert_eq!(s.pixel(x + 50, y + 50), p.border.pixel());
    assert_eq!(s.pixel(x + 50, y - 36), p.border.pixel());
    assert_eq!(s.pixel(x + 50, y + 25), 7);
    // Outside the border: no frame colour, and the shadow darkens below.
    assert_ne!(s.pixel(x + 101, y + 10), p.border.pixel());
    assert_ne!(s.pixel(x + 50, y + 52), s.paper(x + 50, y + 52));
}

/// Attaches `end` as a new display through the manager, and gives the
/// status.
fn attach_display(s: &mut Setup, end: u32) -> u32 {
    let m = s.manager;
    s.f().display = Some(end);
    s.f().send(m, message(&gui::AttachDisplay {}, 7), vec![end]);
    s.c.step();
    let replies = s.f().take(m);
    replies
        .iter()
        .find_map(|r| decode::<gui::ManagerDone>(r, 0).ok())
        .unwrap()
        .1
        .status
        .0
}

/// Makes the display driver stop answering, commits through `client`, and
/// steps until the compositor drops the driver; gives the dropped end.
fn hang(s: &mut Setup, client: u32) -> u32 {
    let old = s.f().display.unwrap();
    s.f().display_dead = true;
    assert_eq!(s.commit(client, 0, 0, 1, 1), status::OK);
    for _ in 0..4 {
        if s.f().closed.contains(&old) {
            return old;
        }
        s.c.step();
    }
    panic!("the hung display was not dropped");
}

#[test]
fn a_display_that_closes_unasked_is_lost() {
    let mut s = setup();
    let display = s.f().display.unwrap();
    s.f().peers.get_mut(&display).unwrap().gone = true;
    s.c.step();
    assert!(s.f().closed.contains(&display));
}

#[test]
fn a_display_with_a_wrong_answer_is_refused() {
    let cases: [(u32, u32, (u32, u32), bool); 5] = [
        (0, 0, (W, H), false),
        (1, display::FORMAT_XRGB, (W, H), false),
        (0, display::FORMAT_XRGB, (0, H), false),
        (0, display::FORMAT_XRGB, (W, 0), false),
        (0, display::FORMAT_XRGB, (W, H), true),
    ];
    for (status, format, size, refuse) in cases {
        let mut s = setup();
        let (a, _) = s.window(4, 4, 1);
        hang(&mut s, a);
        s.f().display_dead = false;
        s.f().info_status = status;
        s.f().info_format = format;
        s.f().display_size = size;
        s.f().refuse_attach = refuse;
        let end = s.f().channel(8);
        assert_eq!(attach_display(&mut s, end), status::UNSUPPORTED);
        assert!(s.f().closed.contains(&end));
    }
}

#[test]
fn a_display_of_a_new_size_is_taken() {
    let mut s = setup();
    let (a, _) = s.window(4, 4, 1);
    hang(&mut s, a);
    s.f().display_dead = false;
    s.f().display_size = (W / 2, H);
    let end = s.f().channel(8);
    assert_eq!(attach_display(&mut s, end), status::OK);
    assert_eq!(s.c.size(), (W / 2, H));
}

#[test]
fn a_one_pixel_wide_display_is_composed() {
    let mut s = setup_with(|f| f.display_size = (1, 4));
    s.c.step();
    assert_eq!(s.f().flushes, [(0, 0, 1, 4)]);
}

#[test]
fn the_manager_takes_exactly_one_handle_for_a_driver() {
    let mut s = setup();
    let m = s.manager;
    for bytes in [
        message(&gui::AttachDisplay {}, 4),
        message(&gui::AttachInput {}, 4),
    ] {
        s.f().send(m, bytes.clone(), vec![]);
        s.c.step();
        let (_, d) = decode::<gui::ManagerDone>(&s.f().take(m)[0], 0).unwrap();
        assert_eq!(d.status.0, status::UNSUPPORTED);
        let channel = s.f().channel(8);
        let memory = s.f().memory(vec![], 4096, false);
        s.f().send(m, bytes, vec![channel, memory]);
        s.c.step();
        let (_, d) = decode::<gui::ManagerDone>(&s.f().take(m)[0], 0).unwrap();
        assert_eq!(d.status.0, status::UNSUPPORTED);
        assert!(s.f().closed.contains(&channel) && s.f().closed.contains(&memory));
    }
}

#[test]
fn an_object_of_some_pages_but_too_few_is_refused() {
    let mut s = setup();
    let end = s.client(8);
    let memory = s.f().memory(vec![0; 2048], 8192, false);
    assert_eq!(s.create(end, 100, 50, memory), status::BAD_BUFFER);
}

#[test]
fn f12_goes_to_the_next_window_in_the_switchers_order() {
    let mut s = setup();
    let ends = [s.client(64), s.client(64), s.client(64)];
    for i in [2, 1, 0] {
        let memory = s.f().memory(vec![1; 16], 64, false);
        assert_eq!(s.create(ends[i], 4, 4, memory), status::OK);
    }
    // The first window took focus; the stack's top is the last created.
    assert_eq!(s.c.focus(), Some(2));
    s.keys(&[(code::F12, 1), (code::F12, 0)]);
    assert_eq!(s.c.focus(), Some(0));
    s.keys(&[(code::F12, 1), (code::F12, 0)]);
    // Then the stack from the top down: every window in turn.
    assert_eq!(s.c.focus(), Some(1));
}

#[test]
fn a_window_that_nearly_fills_the_screen_starts_at_the_corner() {
    let mut s = setup();
    s.window(W - 1, H - 1, 1);
    assert_eq!(s.c.placement(0).map(|r| (r.x, r.y)), Some((1, 64)));
}

#[test]
fn a_part_of_a_surface_is_composed_from_its_own_pixels() {
    let mut s = setup();
    let end = s.client(64);
    let memory = s.f().memory((0..256).collect(), 1024, false);
    assert_eq!(s.create(end, 16, 16, memory), status::OK);
    assert_eq!(s.commit(end, 3, 2, 4, 4), status::OK);
    let (x, y) = (BX, BY);
    assert_eq!(s.pixel(x + 3, y + 2), 2 * 16 + 3);
    assert_eq!(s.pixel(x + 6, y + 5), 5 * 16 + 6);
    assert_eq!(s.pixel(x + 2, y + 2), 0, "outside the commit");
}

#[test]
fn pointer_and_focus_events_carry_the_dropped_count() {
    let mut s = setup();
    let (a, _) = s.window(60, 60, 1);
    let (b, _) = s.window(10, 10, 2);
    s.f().take(a);
    s.f().take(b);
    let scale = |p: i64, side: u32| (p * 65535 / i64::from(side - 1)) as u32;
    let (x, y) = (BX, BY);
    s.f().peers.get_mut(&a).unwrap().room = 0;
    s.point(scale(x + 2, W), scale(y + 2, H), None);
    s.point(scale(x + 5, W), scale(y + 5, H), None);
    s.f().peers.get_mut(&a).unwrap().room = 64;
    s.point(scale(x + 8, W), scale(y + 8, H), None);
    let got: Vec<u32> = s
        .f()
        .take(a)
        .iter()
        .filter_map(|m| decode::<gui::Pointer>(m, 0).ok())
        .map(|(_, p)| p.dropped)
        .collect();
    assert_eq!(got, [2]);
    // Focus: a is full when F12 takes focus from it.
    s.f().peers.get_mut(&a).unwrap().room = 0;
    s.keys(&[(code::F12, 1), (code::F12, 0)]);
    s.f().peers.get_mut(&a).unwrap().room = 64;
    s.keys(&[(code::F12, 1), (code::F12, 0)]);
    let focus: Vec<u32> = s
        .f()
        .take(a)
        .iter()
        .filter_map(|m| decode::<gui::Focus>(m, 0).ok())
        .map(|(_, f)| f.dropped)
        .collect();
    assert_eq!(focus, [1]);
}

#[test]
fn a_closed_client_flushes_just_its_frame() {
    let mut s = setup();
    let (a, _) = s.window(10, 10, 1);
    let (_b, _) = s.window(10, 10, 2);
    let f = s.c.frame();
    let reach = |s: &Setup, client| {
        let k = Key { client, id: 0 };
        s.c.window(k).unwrap().reach(&f)
    };
    // a's window and shadow, and b's (focus moved to b): one rectangle.
    let both = reach(&s, 0)
        .union(&reach(&s, 1))
        .intersect(&Rect::new(0, 0, W.into(), H.into()));
    s.f().flushes.clear();
    s.f().peers.get_mut(&a).unwrap().gone = true;
    s.c.step();
    assert_eq!(
        s.f().flushes,
        [(
            both.x as u32,
            both.y as u32,
            both.width as u32,
            both.height as u32
        )]
    );
}

#[test]
fn the_last_axis_value_is_the_last_pixel() {
    let mut s = setup();
    let (a, _) = s.window(W, H, 1);
    s.f().take(a);
    s.point(65535, 65535, None);
    let got: Vec<(u32, u32)> = s
        .f()
        .take(a)
        .iter()
        .filter_map(|m| decode::<gui::Pointer>(m, 0).ok())
        .map(|(_, p)| (p.x, p.y))
        .collect();
    // The window is at (1, 64): the last pixel is (W - 2, H - 65) in it.
    assert_eq!(got, [(W - 2, H - 65)]);
}

fn press(s: &mut Setup, x: u32, y: u32, button: u32, value: u32) {
    let events = [
        input::Event {
            kind: Below(input::kind::AXIS),
            code: 0,
            value: x,
        },
        input::Event {
            kind: Below(input::kind::AXIS),
            code: 1,
            value: y,
        },
        input::Event {
            kind: Below(input::kind::KEY),
            code: button,
            value,
        },
    ];
    let t = s.tablet;
    let m = input::Frame {
        dropped: 0,
        events: Seq::Of(&events),
    };
    s.f().send(t, message(&m, 0), vec![]);
    s.c.step();
}

#[test]
fn any_button_click_raises_the_window_under_it() {
    let scale = |p: i64, side: u32| (p * 65535 / i64::from(side - 1)) as u32;
    let (x, y) = (BX, BY);
    for button in [code::BTN_LEFT, code::BTN_RIGHT, code::BTN_MIDDLE] {
        let mut s = setup();
        let (a, _) = s.window(60, 60, 1);
        let (b, _) = s.window(60, 60, 2);
        s.commit(a, 0, 0, 60, 60);
        s.commit(b, 0, 0, 60, 60);
        let overlap = (x + 50, y + 50);
        assert_eq!(s.pixel(overlap.0, overlap.1), 2);
        press(&mut s, scale(x + 2, W), scale(y + 2, H), button, 1);
        assert_eq!(s.pixel(overlap.0, overlap.1), 1, "button {button:#x}");
        assert_eq!(s.c.focus(), Some(0));
        // b is still shown where a does not cover it.
        assert_eq!(
            s.pixel(x + (CASCADE as i64) + 50, y + (CASCADE as i64) + 50),
            2
        );
        press(&mut s, scale(x + 2, W), scale(y + 2, H), button, 0);
        // A click on the window already on top changes nothing on screen.
        s.f().flushes.clear();
        press(&mut s, scale(x + 3, W), scale(y + 3, H), button, 1);
        let flushed = s.f().flushes.clone();
        assert!(
            flushed.iter().all(|f| f.2 < 20 && f.3 < 20),
            "only the pointer moved, not the 62-pixel frame: {flushed:?}"
        );
    }
}

#[test]
fn buttons_are_held_and_released() {
    let mut s = setup();
    let (a, _) = s.window(60, 60, 1);
    s.f().take(a);
    let scale = |p: i64, side: u32| (p * 65535 / i64::from(side - 1)) as u32;
    let (x, y) = (BX, BY);
    press(&mut s, scale(x + 2, W), scale(y + 2, H), code::BTN_LEFT, 1);
    press(&mut s, scale(x + 4, W), scale(y + 4, H), code::BTN_LEFT, 0);
    let got: Vec<u32> = s
        .f()
        .take(a)
        .iter()
        .filter_map(|m| decode::<gui::Pointer>(m, 0).ok())
        .map(|(_, p)| p.buttons.0)
        .collect();
    assert_eq!(got, [gui::buttons::LEFT, 0]);
}

/// Review R10(b): a reply lost to a full queue is not counted as a dropped
/// event: the first event after it carries 0.
#[test]
fn a_lost_reply_is_not_a_dropped_event() {
    let mut s = setup();
    let (a, _) = s.window(10, 10, 1);
    s.f().take(a);
    s.f().peers.get_mut(&a).unwrap().room = 0;
    let m = gui::Commit {
        x: 0,
        y: 0,
        width: 1,
        height: 1,
    };
    s.f().send(a, message(&m, 3), vec![]);
    s.c.step();
    assert!(s.f().take(a).is_empty(), "the reply was not delivered");
    s.f().peers.get_mut(&a).unwrap().room = 64;
    s.keys(&[(30, 1)]);
    let dropped: Vec<u32> = s
        .f()
        .take(a)
        .iter()
        .filter_map(|m| decode::<gui::Key>(m, 0).ok())
        .map(|(_, k)| k.dropped)
        .collect();
    assert_eq!(dropped, [0]);
}

/// Review R10(d): when an input driver goes, the modifiers it held go too.
#[test]
fn modifiers_held_by_a_lost_driver_are_forgotten() {
    let mut s = setup();
    let (a, _) = s.window(10, 10, 1);
    s.f().take(a);
    s.keys(&[(code::LEFT_SHIFT, 1)]);
    let k = s.keyboard;
    s.f().peers.get_mut(&k).unwrap().gone = true;
    s.c.step();
    let new = s.f().channel(8);
    let m = s.manager;
    s.f().send(m, message(&gui::AttachInput {}, 7), vec![new]);
    s.c.step();
    s.keyboard = new;
    s.f().take(a);
    s.keys(&[(30, 1)]);
    assert_eq!(keys_of(&s.f().take(a)), [(30, true, Some('a'))]);
}

/// ADR-0093, Decision: a display driver that never answers a flush stalls
/// nobody. Clients keep getting input and frame callbacks; at the deadline
/// the driver is dropped and the manager told; a new driver gets the whole
/// screen in one flush.
#[test]
fn a_display_driver_that_never_answers_stalls_nobody() {
    let mut s = setup();
    let (a, _) = s.window(20, 20, 1);
    let (b, _) = s.window(20, 20, 2);
    s.c.step();
    s.f().take(a);
    s.f().take(b);
    s.f().display_dead = true;
    s.f().flushes.clear();
    let display = s.f().display.unwrap();
    // This commit's flush goes out and is never answered.
    assert_eq!(s.commit(a, 0, 0, 20, 20), status::OK);
    assert_eq!(s.f().flushes.len(), 1);
    // Meanwhile: frame callbacks and keys, and no second flush.
    for i in 0..5 {
        assert_eq!(s.commit(a, i, 0, 1, 1), status::OK);
        assert_eq!(s.commit(b, 0, i, 1, 1), status::OK);
        s.keys(&[(30, 1), (30, 0)]);
        assert_eq!(keys_of(&s.f().take(a)).len(), 2, "round {i}");
    }
    assert_eq!(s.f().flushes.len(), 1, "one flush in flight");
    assert!(!s.f().closed.contains(&display));
    // The deadline passes: the driver is dropped, and the manager hears.
    let m = s.manager;
    s.f().take(m);
    s.f().clock += FLUSH_DEADLINE_NS;
    s.c.step();
    assert!(s.f().closed.contains(&display));
    let told = s.f().take(m);
    assert!(
        told.iter()
            .any(|r| decode::<gui::DisplayHung>(r, 0).is_ok())
    );
    assert_eq!(s.c.stats().hangs, 1);
    // Still serving with no display at all.
    assert_eq!(s.commit(b, 0, 0, 2, 2), status::OK);
    s.f().display_dead = false;
    s.f().flushes.clear();
    let new = s.f().channel(8);
    assert_eq!(attach_display(&mut s, new), status::OK);
    assert_eq!(s.f().flushes, [(0, 0, W, H)]);
}

/// ADR-0093, Decision: commits made while a flush is in flight go to the
/// driver together, as their union, when it answers.
#[test]
fn frames_composed_during_a_flush_go_out_together() {
    let mut s = setup();
    let (a, _) = s.window(40, 40, 1);
    s.c.step();
    s.c.step();
    let display = s.f().display.unwrap();
    s.f().flushes.clear();
    s.f().display_dead = true;
    assert_eq!(s.commit(a, 0, 0, 1, 1), status::OK);
    assert_eq!(s.commit(a, 10, 10, 2, 2), status::OK);
    assert_eq!(s.commit(a, 30, 5, 1, 1), status::OK);
    let (x, y) = (BX, BY);
    assert_eq!(s.f().flushes, [(x as u32, y as u32, 1, 1)]);
    // The answer to the first arrives: the next flush is the union of the
    // other two.
    s.f().display_dead = false;
    let mut done = vec![0; 64];
    let n = encode(&display::Done { status: Below(0) }, 3, &mut done).unwrap();
    done.truncate(n);
    s.f().send(display, done, vec![]);
    s.c.step();
    assert_eq!(s.f().flushes[1], ((x + 10) as u32, (y + 5) as u32, 21, 7));
}

/// Check of the ADR-0093 Decision, error 3: a driver that stops reading
/// its channel, so no flush can even be sent, is hung too; the flush is
/// tried again meanwhile.
#[test]
fn a_display_driver_whose_queue_stays_full_is_hung() {
    let mut s = setup();
    let (a, _) = s.window(10, 10, 1);
    s.c.step();
    s.c.step();
    let display = s.f().display.unwrap();
    s.f().flushes.clear();
    s.f().display_full = true;
    assert_eq!(s.commit(a, 0, 0, 1, 1), status::OK);
    assert!(s.f().flushes.is_empty());
    // Room again before the deadline: the waiting flush goes out by itself.
    s.f().display_full = false;
    s.c.step();
    assert_eq!(s.f().flushes.len(), 1);
    s.c.step();
    // Full for good: dropped at the deadline, and the manager told.
    s.f().display_full = true;
    assert_eq!(s.commit(a, 1, 1, 1, 1), status::OK);
    let m = s.manager;
    s.f().take(m);
    for _ in 0..(FLUSH_DEADLINE_NS / RETRY_NS + 2) {
        if s.f().closed.contains(&display) {
            break;
        }
        s.c.step();
    }
    assert!(s.f().closed.contains(&display));
    assert!(
        s.f()
            .take(m)
            .iter()
            .any(|r| decode::<gui::DisplayHung>(r, 0).is_ok())
    );
}

#[path = "tests_windows.rs"]
mod windows;

#[path = "tests_rooms.rs"]
mod rooms;
#[path = "tests_sheets.rs"]
mod sheets;
