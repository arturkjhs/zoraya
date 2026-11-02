//! The compositor against a fake platform: clients, drivers and memory are
//! plain data, and every hostile case of ADR-0093 and ADR-0094 is a test.

use std::cell::Cell;
use std::collections::{BTreeMap, VecDeque};
use std::vec;
use std::vec::Vec;

use super::*;
use zoraya_protocol::{Seq, WireError};

const W: u32 = 320;
const H: u32 = 200;
const FRAMEBUFFER: u32 = 999;

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
    flushes: Vec<(u32, u32, u32, u32)>,
    attaches: u32,
    clock: u64,
    next: u32,
}

impl Fake {
    fn new() -> Fake {
        Fake {
            display_size: (W, H),
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
                    status: Below(0),
                    width: self.display_size.0,
                    height: self.display_size.1,
                    format: display::FORMAT_XRGB,
                },
                t,
                reply,
            ),
            2 => {
                assert_eq!(handles, [FRAMEBUFFER]);
                self.attaches += 1;
                encode(&display::Done { status: Below(0) }, t, reply)
            }
            3 => {
                let (_, f) = decode::<display::Flush>(message, 0).unwrap();
                self.flushes.push((f.x, f.y, f.width, f.height));
                encode(&display::Done { status: Below(0) }, t, reply)
            }
            _ => panic!("unexpected display request"),
        };
        n.ok()
    }

    fn wait(&mut self, ready: &mut Vec<u64>) {
        for (key, end) in &self.watched {
            if let Some(p) = self.peers.get(end)
                && (!p.inbox.is_empty() || p.gone)
            {
                ready.push(*key);
            }
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
    let mut f = Fake::new();
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
    assert_eq!(s.pixel(0, 0), BACKGROUND);
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
    assert_eq!((area.x, area.y), ORIGIN);
    // Created, not yet committed: the compositor's copy is black.
    assert_eq!(s.pixel(ORIGIN.0, ORIGIN.1), 0);
    assert_eq!(s.pixel(ORIGIN.0 - 1, ORIGIN.1), BORDER_FOCUSED);
    s.f().flushes.clear();
    assert_eq!(s.commit(end, 0, 0, 100, 50), status::OK);
    assert_eq!(s.pixel(ORIGIN.0, ORIGIN.1), 0x0012_3456);
    assert_eq!(s.pixel(ORIGIN.0 + 99, ORIGIN.1 + 49), 0x0012_3456);
    assert_eq!(s.pixel(ORIGIN.0 + 100, ORIGIN.1 + 50), BORDER_FOCUSED);
    assert_eq!(s.f().flushes, [(16, 16, 100, 50)]);
    assert_eq!(s.c.stats().commits, 1);
    // Damage of part of it flushes that part.
    s.f().flushes.clear();
    assert_eq!(s.commit(end, 10, 20, 5, 3), status::OK);
    assert_eq!(s.f().flushes, [(26, 36, 5, 3)]);
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
    assert_eq!(s.pixel(ORIGIN.0, ORIGIN.1), 7);
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
    let memory = s.f().memory(vec![], 16 * 16 * 4, true);
    assert_eq!(s.create(end, 16, 16, memory), status::OK);
    assert_eq!(s.commit(end, 0, 0, 16, 16), status::OK);
    assert_eq!(s.f().memory[&memory].reads.get(), 256);
    let shown: Vec<u32> = (0..16)
        .flat_map(|y| (0..16).map(move |x| (x, y)))
        .map(|(x, y)| s.pixel(ORIGIN.0 + x, ORIGIN.1 + y))
        .collect();
    assert_eq!(shown, (1..=256).collect::<Vec<u32>>());
    // A redraw for another reason (a second window on top, then gone)
    // composes from the copy and reads no client memory.
    let (other, _) = s.window(8, 8, 5);
    let reads = s.f().memory[&memory].reads.get();
    s.f().peers.get_mut(&other).unwrap().gone = true;
    s.c.step();
    assert_eq!(s.f().memory[&memory].reads.get(), reads);
    assert_eq!(s.pixel(ORIGIN.0 + CASCADE, ORIGIN.1 + CASCADE), BACKGROUND);
    assert_eq!(s.pixel(ORIGIN.0 + 15, ORIGIN.1 + 15), 256);
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
    let (bx, by) = (ORIGIN.0 + CASCADE + 40, ORIGIN.1 + CASCADE + 40);
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
    s.point(scale(ORIGIN.0 + 2, W), scale(ORIGIN.1 + 2, H), Some(1));
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
    s.point(0, 0, None);
    assert_eq!(s.pixel(0, 0), 0);
    assert_eq!(s.pixel(1, 2), 0x00ff_ffff);
    s.point(65535, 65535, None);
    assert_eq!(s.pixel(1, 2), BACKGROUND);
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
    assert_eq!(s.pixel(ORIGIN.0, ORIGIN.1), BACKGROUND);
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
    s.f().display_dead = true;
    // Commits are still answered with no display.
    assert_eq!(s.commit(a, 0, 0, 10, 10), status::OK);
    let old = s.f().display.unwrap();
    assert!(s.f().closed.contains(&old));
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
    assert_eq!(s.pixel(ORIGIN.0, ORIGIN.1), 4);
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
}

#[test]
fn windows_are_cascaded_and_kept_on_screen() {
    let mut s = setup();
    s.window(W, H, 1);
    assert_eq!(s.c.placement(0).map(|r| (r.x, r.y)), Some((0, 0)));
    s.window(W - 10, H - 10, 1);
    let r = s.c.placement(1).unwrap();
    assert!(r.right() <= i64::from(W) && r.bottom() <= i64::from(H));
    s.window(10, 10, 1);
    assert_eq!(
        s.c.placement(2).map(|r| (r.x, r.y)),
        Some((ORIGIN.0 + 2 * CASCADE, ORIGIN.1 + 2 * CASCADE))
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
