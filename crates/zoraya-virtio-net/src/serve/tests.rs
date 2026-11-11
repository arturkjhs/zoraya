use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;

use zoraya_proto_nic::{Frame, Slot, Transmit};
use zoraya_protocol::{Header, Seq, decode, encode};
use zoraya_virtio::fake::{Device, FakeRegisters, FakeShared};

use super::*;
use crate::tests::{BASE, FRAMES, deliver, device};
use crate::{SHARED_MEMORY, SLOT_SIZE};

#[derive(Default)]
struct Wire {
    /// Messages the client may still take before the channel is full.
    room: usize,
    written: Vec<Vec<u8>>,
    incoming: VecDeque<(Vec<u8>, Option<u32>)>,
    closed: bool,
    handles_closed: Vec<u32>,
    pins: u32,
    unpins: u32,
    bad_object: bool,
}

#[derive(Clone, Default)]
struct Fake(Rc<RefCell<Wire>>);

impl Host for Fake {
    type Handle = u32;

    fn write(&mut self, bytes: &[u8]) -> Write {
        let mut w = self.0.borrow_mut();
        if w.closed {
            return Write::Closed;
        }
        if w.written.len() >= w.room {
            return Write::Full;
        }
        w.written.push(bytes.to_vec());
        Write::Done
    }

    fn read(&mut self, buffer: &mut [u8]) -> Read<u32> {
        let mut w = self.0.borrow_mut();
        match w.incoming.pop_front() {
            Some((bytes, handle)) => {
                buffer[..bytes.len()].copy_from_slice(&bytes);
                Read::Message {
                    length: bytes.len(),
                    handle,
                }
            }
            None if w.closed => Read::Closed,
            None => Read::Empty,
        }
    }

    fn close(&mut self, handle: u32) {
        self.0.borrow_mut().handles_closed.push(handle);
    }

    fn pin(&mut self, _: u32) -> Option<u64> {
        let mut w = self.0.borrow_mut();
        if w.bad_object {
            return None;
        }
        w.pins += 1;
        Some(FRAMES)
    }

    fn unpin(&mut self) {
        self.0.borrow_mut().unpins += 1;
    }

    fn now(&mut self) -> u64 {
        7
    }
}

type Srv = Server<FakeRegisters, FakeShared, Fake>;

struct Rig {
    d: Device,
    wire: Fake,
    s: Srv,
}

fn send<'a, M: zoraya_protocol::Message<'a>>(wire: &Fake, t: u32, m: &M, handle: Option<u32>) {
    let mut out = [0u8; BUFFER];
    let n = encode(m, t, &mut out).unwrap();
    wire.0
        .borrow_mut()
        .incoming
        .push_back((out[..n].to_vec(), handle));
}

/// A rig with the frames object attached; the channel takes `room` messages.
fn rig(room: usize) -> Rig {
    let d = device();
    let wire = Fake::default();
    wire.0.borrow_mut().room = 1000;
    let net = Net::new(d.registers(), d.shared(), BASE).unwrap();
    let mut s = Server::new(net, wire.clone());
    send(&wire, 1, &Attach {}, Some(9));
    assert_eq!(s.serve(), Flow::Go);
    assert_eq!(done_status(&wire), status::OK);
    wire.0.borrow_mut().written.clear();
    wire.0.borrow_mut().room = room;
    Rig { d, wire, s }
}

fn done_status(wire: &Fake) -> u32 {
    let w = wire.0.borrow();
    let (_, m) = decode::<Done>(w.written.last().unwrap(), 0).unwrap();
    m.status.0
}

fn ordinals(wire: &Fake) -> Vec<u32> {
    wire.0
        .borrow()
        .written
        .iter()
        .map(|b| Header::decode(b).unwrap().0.ordinal)
        .collect()
}

/// Every slot named by messages of kind `T`, in order.
fn sent_slots(wire: &Fake) -> Vec<u32> {
    let w = wire.0.borrow();
    let mut all = Vec::new();
    for b in &w.written {
        if let Ok((_, m)) = decode::<Sent>(b, 0) {
            all.extend(m.slots.iter().map(|s| s.slot));
        }
    }
    all
}

fn transmit(r: &mut Rig, slots: &[u32]) {
    let frames: Vec<Frame> = slots
        .iter()
        .map(|s| Frame {
            slot: *s,
            length: 60,
        })
        .collect();
    send(
        &r.wire,
        0,
        &Transmit {
            frames: Seq::Of(&frames),
        },
        None,
    );
    assert_eq!(r.s.serve(), Flow::Go);
}

/// The device completes every transmit chain.
fn finish_all(r: &Rig) {
    let mut f = r.d.state.borrow_mut();
    while let Some(head) = f.next_available(1) {
        f.complete(1, u32::from(head), 0);
    }
}

#[test]
fn a_full_channel_loses_no_sent_and_no_slot_is_reused_before_it_is_reported() {
    let mut r = rig(0);
    let all: Vec<u32> = (SLOTS as u32..2 * SLOTS as u32).collect();
    transmit(&mut r, &all);
    finish_all(&r);
    // Many interrupts while the client's queue is full.
    for _ in 0..20 {
        assert_eq!(r.s.device_event(), Flow::Go);
        assert!(r.s.pending());
    }
    assert!(r.wire.0.borrow().written.is_empty());
    // Finished is not free: every one of the 64 is still refused, and the
    // refusal says that the slot is with the driver.
    transmit(&mut r, &all);
    // The client reads: the channel takes messages again.
    r.wire.0.borrow_mut().room = 1000;
    assert_eq!(r.s.flush(), Flow::Go);
    let mut got = sent_slots(&r.wire);
    got.sort_unstable();
    assert_eq!(got, all, "each slot exactly once");
    assert_eq!(
        ordinals(&r.wire),
        vec![19, 17],
        "the refusals, then the Sent"
    );
    assert!(!r.s.pending());
    // Nothing more, however often it is asked.
    let before = r.wire.0.borrow().written.len();
    for _ in 0..5 {
        r.s.flush();
        r.s.device_event();
    }
    assert_eq!(r.wire.0.borrow().written.len(), before);
    // Only now is a slot free.
    r.wire.0.borrow_mut().written.clear();
    transmit(&mut r, &all[..1]);
    assert_eq!(r.s.flush(), Flow::Go);
    assert!(ordinals(&r.wire).is_empty(), "accepted without a reply");
}

#[test]
fn a_slot_is_refused_while_the_device_holds_it_and_not_reported_sent() {
    let mut r = rig(1000);
    transmit(&mut r, &[70]);
    transmit(&mut r, &[70]);
    // A rejection with the reason, and no Sent.
    let w = r.wire.0.borrow().written.clone();
    assert_eq!(w.len(), 1);
    let (_, m) = decode::<Rejected>(&w[0], 0).unwrap();
    let list: Vec<(u32, u32)> = m.slots.iter().map(|x| (x.slot, x.reason)).collect();
    assert_eq!(list, vec![(70, reason::STILL_QUEUED)]);
    assert!(sent_slots(&r.wire).is_empty());
    // The device completes: exactly one Sent for it.
    finish_all(&r);
    assert_eq!(r.s.device_event(), Flow::Go);
    assert_eq!(sent_slots(&r.wire), vec![70]);
}

#[test]
fn refusals_for_a_bad_length_or_slot_say_so() {
    let mut r = rig(1000);
    let frames = [
        Frame {
            slot: 71,
            length: 5,
        },
        Frame {
            slot: 3,
            length: 60,
        },
        Frame {
            slot: 4000,
            length: 60,
        },
    ];
    send(
        &r.wire,
        0,
        &Transmit {
            frames: Seq::Of(&frames),
        },
        None,
    );
    assert_eq!(r.s.serve(), Flow::Go);
    let w = r.wire.0.borrow().written.clone();
    let (_, m) = decode::<Rejected>(&w[0], 0).unwrap();
    let list: Vec<(u32, u32)> = m.slots.iter().map(|x| (x.slot, x.reason)).collect();
    // Slot 3 and 4000 are not transmit slots: counted, not reported.
    assert_eq!(list, vec![(71, reason::BAD_LENGTH)]);
    assert_eq!(r.s.net.refused(), 3);
}

#[test]
fn a_link_change_and_replies_are_kept_while_the_channel_is_full() {
    let mut r = rig(0);
    for t in 10..40 {
        send(&r.wire, t, &Info {}, None);
    }
    assert_eq!(r.s.serve(), Flow::Go);
    // The link goes down and up and down while nothing can be written.
    for status in [0u8, 1, 0] {
        r.d.state.borrow_mut().config[6] = status;
        assert_eq!(r.s.device_event(), Flow::Go);
    }
    assert!(r.wire.0.borrow().written.is_empty());
    r.wire.0.borrow_mut().room = 1000;
    assert_eq!(r.s.flush(), Flow::Go);
    let w = r.wire.0.borrow().written.clone();
    // One reply, one link message with the state of the moment.
    assert_eq!(ordinals(&r.wire), vec![1, 18]);
    let (_, reply) = decode::<InfoReply>(&w[0], 0).unwrap();
    assert!(!reply.link);
    let (_, link) = decode::<LinkChanged>(&w[1], 0).unwrap();
    assert!(!link.link);
}

#[test]
fn frames_that_cannot_be_told_are_undone_counted_and_offered_again() {
    let mut r = rig(0);
    deliver(&r.d, &[0u8; 60]);
    deliver(&r.d, &[0u8; 61]);
    assert_eq!(r.s.device_event(), Flow::Go);
    assert_eq!(r.s.net.dropped(), 2);
    // The slots are back with the device: it can deliver 64 frames again.
    for _ in 0..SLOTS {
        deliver(&r.d, &[0u8; 60]);
    }
    r.wire.0.borrow_mut().room = 1000;
    assert_eq!(r.s.device_event(), Flow::Go);
    let w = r.wire.0.borrow().written.clone();
    let (_, m) = decode::<Received>(&w[0], 0).unwrap();
    assert_eq!(m.frames.len(), SLOTS);
}

#[test]
fn hostile_messages_end_the_driver_or_are_refused() {
    // An unknown ordinal, with a handle: the handle is closed, the driver stops.
    let mut r = rig(1000);
    let mut bytes = [0u8; 16];
    let n = encode(&Info {}, 1, &mut bytes).unwrap();
    // Corrupt the ordinal.
    bytes[4] = 99;
    r.wire
        .0
        .borrow_mut()
        .incoming
        .push_back((bytes[..n].to_vec(), Some(5)));
    assert_eq!(r.s.serve(), Flow::Stop);
    assert_eq!(r.wire.0.borrow().handles_closed, vec![9, 5]);
    // Garbage that is not even a header.
    let mut r = rig(1000);
    r.wire.0.borrow_mut().incoming.push_back((vec![1, 2], None));
    assert_eq!(r.s.serve(), Flow::Stop);
    // A second attach is refused and its handle closed; one pin only.
    let mut r = rig(1000);
    send(&r.wire, 3, &Attach {}, Some(11));
    assert_eq!(r.s.serve(), Flow::Go);
    assert_eq!(done_status(&r.wire), status::BAD_OBJECT);
    assert_eq!(r.wire.0.borrow().handles_closed, vec![9, 11]);
    assert_eq!(r.wire.0.borrow().pins, 1);
    // An attach without a handle; and a handle with the wrong message.
    send(&r.wire, 4, &Attach {}, None);
    send(&r.wire, 5, &Info {}, Some(12));
    assert_eq!(r.s.serve(), Flow::Go);
    assert_eq!(r.wire.0.borrow().handles_closed, vec![9, 11, 12]);
    // The client went away.
    r.wire.0.borrow_mut().closed = true;
    assert_eq!(r.s.serve(), Flow::Stop);
}

#[test]
fn an_object_the_device_cannot_use_is_refused_and_unpinned() {
    let d = device();
    let wire = Fake::default();
    wire.0.borrow_mut().room = 10;
    wire.0.borrow_mut().bad_object = true;
    let net = Net::new(d.registers(), d.shared(), BASE).unwrap();
    let mut s = Server::new(net, wire.clone());
    send(&wire, 1, &Attach {}, Some(9));
    assert_eq!(s.serve(), Flow::Go);
    assert_eq!(done_status(&wire), status::BAD_OBJECT);
    assert_eq!(wire.0.borrow().handles_closed, vec![9]);
}

#[test]
fn a_release_returns_a_receive_slot_and_a_flood_of_messages_is_bounded() {
    let mut r = rig(1000);
    deliver(&r.d, &[0u8; 60]);
    assert_eq!(r.s.device_event(), Flow::Go);
    let slots = [Slot { slot: 0 }];
    for _ in 0..MAX_MESSAGES + 10 {
        send(
            &r.wire,
            0,
            &Release {
                slots: Seq::Of(&slots),
            },
            None,
        );
    }
    assert_eq!(r.s.serve(), Flow::Go);
    // The rest waits for the next call.
    assert_eq!(r.wire.0.borrow().incoming.len(), 10);
}

#[test]
fn the_frames_object_is_the_clients_address_not_a_mapping_of_the_driver() {
    // The slots lie at FRAMES; the queues are below it.
    assert!(FRAMES >= BASE + SHARED_MEMORY as u64);
    assert_eq!(SLOT_SIZE, u64::from(zoraya_proto_nic::SLOT_SIZE));
}
