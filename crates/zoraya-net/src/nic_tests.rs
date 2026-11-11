//! The driver's end of the frame protocol (INV-0221-2: the service treats the
//! driver's messages and frames as untrusted).

use super::*;
use crate::nic::{Nic, Read, SlotMemory, Wire, Write};
use zoraya_proto_nic::{
    Frame, LinkChanged, Received, Refusal, Rejected, Release, SLOT_SIZE, SLOTS, Sent, Slot,
    Transmit, reason,
};
use zoraya_protocol::{Seq, decode, encode};

#[derive(Default)]
struct FakeWire {
    incoming: VecDeque<Vec<u8>>,
    written: Vec<Vec<u8>>,
    full: bool,
    closed: bool,
}

impl Wire for FakeWire {
    fn read(&mut self, buf: &mut [u8]) -> Read {
        match self.incoming.pop_front() {
            Some(m) => {
                let n = m.len().min(buf.len());
                buf[..n].copy_from_slice(&m[..n]);
                Read::Message(n)
            }
            None if self.closed => Read::Closed,
            None => Read::Empty,
        }
    }

    fn write(&mut self, bytes: &[u8]) -> Write {
        if self.closed {
            return Write::Closed;
        }
        if self.full {
            return Write::Full;
        }
        self.written.push(bytes.to_vec());
        Write::Done
    }
}

struct FakeMemory(Vec<u8>);

impl SlotMemory for FakeMemory {
    fn read_slot(&self, slot: u32, out: &mut [u8]) {
        let at = slot as usize * SLOT_SIZE as usize;
        out.copy_from_slice(&self.0[at..at + out.len()]);
    }

    fn write_slot(&mut self, slot: u32, data: &[u8]) {
        let at = slot as usize * SLOT_SIZE as usize;
        self.0[at..at + data.len()].copy_from_slice(data);
    }
}

type FakeNic = Nic<FakeWire, FakeMemory>;

fn fresh() -> FakeNic {
    let memory = FakeMemory(vec![0; 2 * SLOTS as usize * SLOT_SIZE as usize]);
    Nic::new(FakeWire::default(), memory, true)
}

fn received(frames: &[Frame]) -> Vec<u8> {
    let mut out = vec![0u8; 2048];
    let message = Received {
        at: 5,
        frames: Seq::Of(frames),
    };
    let n = encode(&message, 0, &mut out).unwrap();
    out.truncate(n);
    out
}

fn sent(slots: &[u32]) -> Vec<u8> {
    let list: Vec<Slot> = slots.iter().map(|&slot| Slot { slot }).collect();
    let mut out = vec![0u8; 2048];
    let n = encode(
        &Sent {
            slots: Seq::Of(&list),
        },
        0,
        &mut out,
    )
    .unwrap();
    out.truncate(n);
    out
}

fn rejected(slot: u32, why: u32) -> Vec<u8> {
    let list = [Refusal { slot, reason: why }];
    let mut out = vec![0u8; 2048];
    let n = encode(
        &Rejected {
            slots: Seq::Of(&list),
        },
        0,
        &mut out,
    )
    .unwrap();
    out.truncate(n);
    out
}

fn feed(nic: &mut FakeNic, message: Vec<u8>) {
    nic.wire().incoming.push_back(message);
    nic.pump();
}

fn released(nic: &mut FakeNic) -> Vec<u32> {
    let mut all = Vec::new();
    for m in &nic.wire().written {
        if let Ok((_, r)) = decode::<Release>(m, 0) {
            all.extend(r.slots.iter().map(|s| s.slot));
        }
    }
    all
}

fn frame(slot: u32, length: u32) -> Frame {
    Frame { slot, length }
}

#[test]
fn a_received_frame_is_copied_out_and_its_slot_given_back() {
    let mut nic = fresh();
    let at = 3 * SLOT_SIZE as usize;
    nic.mem_mut().0[at..at + 60].copy_from_slice(&[0x5a; 60]);
    feed(&mut nic, received(&[frame(3, 60)]));
    assert!(nic.has_input());
    let mut buf = [0u8; 1514];
    assert_eq!(nic.recv(&mut buf), Some(60));
    assert_eq!(&buf[..60], &[0x5a; 60]);
    assert_eq!(nic.recv(&mut buf), None);
    // The driver changes the slot after our copy: nothing of ours moves.
    nic.mem_mut().0[at] = 0;
    assert_eq!(buf[0], 0x5a);
    nic.flush();
    assert_eq!(released(&mut nic), vec![3]);
    // The slot is the driver's again: the same slot can arrive again.
    feed(&mut nic, received(&[frame(3, 60)]));
    assert!(nic.has_input());
    assert_eq!(nic.faults.bad_entries, 0);
}

#[test]
fn frames_with_impossible_slots_or_lengths_are_counted_and_never_read() {
    let mut nic = fresh();
    let bad = [
        frame(SLOTS, 60),
        frame(u32::MAX, 60),
        frame(9, 0),
        frame(10, 13),
        frame(11, 1515),
        frame(12, u32::MAX),
    ];
    feed(&mut nic, received(&bad));
    assert!(!nic.closed());
    assert_eq!(nic.faults.bad_entries, 6);
    assert!(!nic.has_input());
    nic.flush();
    // The four with a real slot go straight back; the two out of range are
    // not ours to give back.
    let mut back = released(&mut nic);
    back.sort_unstable();
    assert_eq!(back, vec![9, 10, 11, 12]);
}

#[test]
fn a_slot_the_driver_offers_twice_is_taken_once() {
    let mut nic = fresh();
    let f = frame(4, 60);
    feed(&mut nic, received(&[f, f]));
    assert_eq!(nic.faults.bad_entries, 1);
    let mut buf = [0u8; 1514];
    assert_eq!(nic.recv(&mut buf), Some(60));
    assert_eq!(nic.recv(&mut buf), None);
    nic.flush();
    assert_eq!(released(&mut nic), vec![4]);
}

#[test]
fn transmit_slots_are_used_once_and_come_back_with_sent_or_a_refusal() {
    let mut nic = fresh();
    let data = [7u8; 100];
    for _ in 0..SLOTS {
        assert!(nic.send(&data));
        nic.flush();
    }
    assert!(!nic.can_send(), "all 64 are in flight");
    assert!(!nic.send(&data));
    // The driver was told of every slot once.
    let mut announced: Vec<u32> = nic
        .wire()
        .written
        .iter()
        .filter_map(|m| decode::<Transmit>(m, 0).ok())
        .flat_map(|(_, t)| t.frames.iter().map(|f| f.slot).collect::<Vec<_>>())
        .collect();
    announced.sort_unstable();
    assert_eq!(announced, (SLOTS..2 * SLOTS).collect::<Vec<_>>());
    // Sent frees one; a repeat of it, a receive slot and a slot out of
    // range are faults.
    feed(&mut nic, sent(&[SLOTS + 5]));
    assert!(nic.can_send());
    feed(&mut nic, sent(&[SLOTS + 5, 3, 2 * SLOTS, u32::MAX]));
    assert_eq!(nic.faults.bad_entries, 4);
    assert!(nic.send(&data));
    nic.flush();
    assert!(!nic.can_send());
    // A refusal that did not take the slot frees it, STILL_QUEUED does not.
    feed(&mut nic, rejected(SLOTS + 6, reason::STILL_QUEUED));
    assert!(!nic.can_send());
    feed(&mut nic, rejected(SLOTS + 6, reason::BAD_LENGTH));
    assert!(nic.can_send());
    assert!(!nic.closed());
}

#[test]
fn frames_that_are_no_frames_are_not_sent() {
    let mut nic = fresh();
    assert!(!nic.send(&[0; 13]));
    assert!(!nic.send(&[0; 1515]));
    assert!(nic.send(&[0; 14]));
    assert!(nic.send(&[0; 1514]));
}

#[test]
fn a_full_channel_keeps_what_it_could_not_take_for_the_next_flush() {
    let mut nic = fresh();
    feed(&mut nic, received(&[frame(1, 60)]));
    let mut buf = [0u8; 1514];
    nic.recv(&mut buf).unwrap();
    assert!(nic.send(&[0; 60]));
    nic.wire().full = true;
    nic.flush();
    assert!(nic.pending());
    assert!(nic.wire().written.is_empty());
    nic.wire().full = false;
    nic.flush();
    assert!(!nic.pending());
    assert_eq!(nic.wire().written.len(), 2);
    assert_eq!(released(&mut nic), vec![1]);
}

#[test]
fn a_closed_wire_or_a_message_that_is_not_ours_ends_the_service() {
    let mut nic = fresh();
    nic.wire().closed = true;
    nic.pump();
    assert!(nic.closed());
    // A well-formed message that a driver never sends: an ordinal we do not
    // know.
    let mut unknown = vec![0u8; 64];
    let n = encode(&zoraya_proto_nic::Info {}, 0, &mut unknown).unwrap();
    unknown.truncate(n);
    for junk in [vec![], vec![1, 2, 3], vec![0xff; 40], unknown] {
        let mut nic = fresh();
        feed(&mut nic, junk);
        assert!(nic.closed());
        assert_eq!(nic.faults.bad_messages, 1);
        assert!(!nic.send(&[0; 60]), "no more frames to a broken driver");
    }
}

#[test]
fn link_changes_are_followed() {
    let mut nic = fresh();
    assert!(nic.is_up());
    let mut out = vec![0u8; 64];
    let n = encode(&LinkChanged { link: false }, 0, &mut out).unwrap();
    feed(&mut nic, out[..n].to_vec());
    assert!(!nic.is_up());
}

/// Random messages and frames, many of them near-valid: no panic, no index
/// out of range.
#[test]
fn random_driver_traffic_never_panics() {
    let mut rng = Rng(0x0ddc_0ffe_e123);
    let mut nic = fresh();
    let mut buf = [0u8; 1514];
    let mut taken = 0usize;
    for _ in 0..20_000 {
        let message = match rng.next() % 5 {
            0 => {
                let n = 1 + rng.next() % 6;
                let frames: Vec<Frame> = (0..n)
                    .map(|_| {
                        let length = [0, 13, 14, 60, 1514, 1515, 9999][(rng.next() % 7) as usize];
                        frame((rng.next() % 80) as u32, length)
                    })
                    .collect();
                received(&frames)
            }
            1 => sent(&[SLOTS + (rng.next() % 80) as u32, (rng.next() % 130) as u32]),
            2 => rejected(SLOTS + (rng.next() % 70) as u32, (rng.next() % 6) as u32),
            3 => (0..(rng.next() % 90)).map(|_| rng.next() as u8).collect(),
            _ => {
                let mut m = received(&[frame(2, 60)]);
                let at = (rng.next() as usize) % m.len();
                m[at] = rng.next() as u8;
                m
            }
        };
        nic.wire().incoming.push_back(message);
        nic.pump();
        if nic.closed() {
            nic = fresh();
        }
        while nic.recv(&mut buf).is_some() {
            taken += 1;
        }
        if rng.next().is_multiple_of(3) {
            nic.send(&[1; 80]);
        }
        nic.flush();
        nic.wire().written.clear();
    }
    assert!(taken > 100, "frames flowed: {taken}");
}
