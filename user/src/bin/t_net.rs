#![no_std]
#![no_main]

//! N1's acceptance test (docs/research/network/plan.md, ADR-0221, ADR-0230):
//! the network driver on a real virtio-net device under `--net`. This
//! program plays `netd`: it starts `ethd`, creates the frames object,
//! attaches it, sends an ARP who-has for SLIRP's gateway through the
//! transmit path, and reads the reply through the receive path; any ARP
//! request for `10.0.2.15` that arrives is answered by hand. It also sends
//! the driver hostile requests. Without a network device it passes having
//! done nothing, as `t_gui` does without a GPU.

extern crate alloc;

use zoraya_abi::spawn::Transfer;
use zoraya_abi::{
    ChannelEntry, ExitReason, ExitRecord, Handle, MapFlags, MemoryFlags, MemoryObject, Mode,
    ObjectType, Packet, Status, channel_signals, process_signals,
};
use zoraya_proto_nic::{
    Attach, Done, Frame, Info, InfoReply, LinkChanged, OBJECT_SIZE, Received, Rejected, Release,
    SLOT_SIZE, SLOTS, Sent, Slot, Transmit, arp, reason, status,
};
use zoraya_protocol::{Header, Message, NO_REPLY, Seq, decode, encode};
use zoraya_user::gui::{scan, start_driver};
use zoraya_user::{
    channel_read, channel_write, clock_monotonic, dma_release_quarantine, handle_close,
    memory_create, memory_map, port_create, port_subscribe, port_wait, println,
    process_exit_record,
};

const OUR_IP: [u8; 4] = [10, 0, 2, 15];
const GATEWAY_IP: [u8; 4] = [10, 0, 2, 2];
/// The sender address of our who-has. Not 10.0.2.15: SLIRP learns the guest's
/// MAC address from the sender of an ARP request, and asks who has 10.0.2.15
/// (the thing `make test-net` wants to see answered) only while it does not
/// know it.
const PROBE_IP: [u8; 4] = [10, 0, 2, 30];
/// Every wait ends: TCG is slow, so seconds.
const WAIT_NS: u64 = 10_000_000_000;
const ROUNDS: usize = 20;
/// How long to wait for an ARP request for us after the rounds.
const HOLD_NS: u64 = 8_000_000_000;

/// Waits up to `wait` ns for `process` to end; its record, or `None`.
fn wait_exit_within(process: Handle, wait: u64) -> Option<ExitRecord> {
    let port = port_create().expect("port");
    port_subscribe(port, process, process_signals::TERMINATED, 1, Mode::Level).expect("subscribe");
    let mut packets = [Packet::default()];
    let waited = port_wait(port, clock_monotonic() + wait, &mut packets);
    let _ = handle_close(port);
    waited.ok()?;
    process_exit_record(process).ok()
}

struct Nic {
    end: Handle,
    port: Handle,
    /// The mapped frames object.
    frames: u64,
}

/// What came from the driver.
enum Push {
    Received(u64, alloc::vec::Vec<Frame>),
    Sent(alloc::vec::Vec<u32>),
    /// Refused transmits: the slot and the reason.
    Rejected(alloc::vec::Vec<(u32, u32)>),
    Link,
    Reply(alloc::vec::Vec<u8>),
}

impl Nic {
    fn send<'a, M: Message<'a>>(&self, transaction: u32, m: &M, handle: Option<Handle>) {
        let mut out = [0u8; 1024];
        let n = encode(m, transaction, &mut out).expect("fits");
        let entries: alloc::vec::Vec<ChannelEntry> = handle
            .map(|h| ChannelEntry {
                handle: h,
                rights: MemoryObject::RIGHTS,
                transfer: Transfer::Duplicate,
            })
            .into_iter()
            .collect();
        channel_write(self.end, &out[..n], &entries).expect("write to the driver");
    }

    /// The next message that is not a received frame (an unsolicited frame
    /// from the network is given back and skipped).
    fn next_not_received(&self, wait: u64) -> Push {
        loop {
            match self.next(wait).expect("the driver pushes") {
                Push::Received(_, list) => {
                    for f in list {
                        self.release(f.slot);
                    }
                }
                Push::Link => {}
                other => return other,
            }
        }
    }

    /// The next message, if one comes before the deadline.
    fn next(&self, wait: u64) -> Option<Push> {
        let mut bytes = [0u8; 2048];
        let sizes = loop {
            match channel_read(self.end, &mut bytes, &mut []) {
                Ok(s) => break s,
                Err((Status::WouldBlock, _)) => {
                    let mut packets = [Packet::default()];
                    port_wait(self.port, clock_monotonic() + wait, &mut packets).ok()?;
                }
                Err(_) => return None,
            }
        };
        let m = &bytes[..sizes.bytes as usize];
        let (header, _) = Header::decode(m).ok()?;
        Some(match header.ordinal {
            16 => {
                let (_, r) = decode::<Received>(m, 0).expect("received");
                Push::Received(r.at, r.frames.iter().collect())
            }
            17 => {
                let (_, s) = decode::<Sent>(m, 0).expect("sent");
                Push::Sent(s.slots.iter().map(|s| s.slot).collect())
            }
            19 => {
                let (_, r) = decode::<Rejected>(m, 0).expect("rejected");
                Push::Rejected(r.slots.iter().map(|r| (r.slot, r.reason)).collect())
            }
            18 => {
                decode::<LinkChanged>(m, 0).expect("link");
                Push::Link
            }
            _ => Push::Reply(m.to_vec()),
        })
    }

    fn slot_ptr(&self, slot: u32, length: usize) -> *mut u8 {
        assert!(length <= SLOT_SIZE as usize && slot < 2 * SLOTS);
        (self.frames + u64::from(slot) * u64::from(SLOT_SIZE)) as *mut u8
    }

    /// A copy of a received frame, taken once (the slot is hostile input).
    fn read_slot(&self, slot: u32, length: usize) -> alloc::vec::Vec<u8> {
        let mut copy = alloc::vec![0u8; length];
        // SAFETY: inside the mapping of OBJECT_SIZE bytes this process made
        // (`slot_ptr` checks the range); a receive slot is read only while
        // the service, this program, holds it (until released).
        unsafe {
            core::ptr::copy_nonoverlapping(self.slot_ptr(slot, length), copy.as_mut_ptr(), length)
        };
        copy
    }

    fn transmit(&self, slot: u32, frame: &[u8]) {
        // SAFETY: as in `read_slot`; a transmit slot is written only while
        // the driver does not hold it (not yet sent, or reported sent).
        unsafe {
            core::ptr::copy_nonoverlapping(
                frame.as_ptr(),
                self.slot_ptr(slot, frame.len()),
                frame.len(),
            )
        };
        let frames = [Frame {
            slot,
            length: frame.len() as u32,
        }];
        self.send(
            NO_REPLY,
            &Transmit {
                frames: Seq::Of(&frames),
            },
            None,
        );
    }

    fn release(&self, slot: u32) {
        let slots = [Slot { slot }];
        self.send(
            NO_REPLY,
            &Release {
                slots: Seq::Of(&slots),
            },
            None,
        );
    }

    fn reply<'a, M: Message<'a>>(&self, transaction: u32, bytes: &'a mut alloc::vec::Vec<u8>) -> M {
        loop {
            match self.next(WAIT_NS).expect("the driver answers") {
                Push::Reply(r) => {
                    *bytes = r;
                    break;
                }
                _ => continue,
            }
        }
        let (h, m) = decode::<M>(bytes, 0).expect("a reply of the right kind");
        assert_eq!(h.transaction, transaction);
        m
    }
}

fn mac_text(m: [u8; 6]) -> alloc::string::String {
    alloc::format!(
        "{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
        m[0],
        m[1],
        m[2],
        m[3],
        m[4],
        m[5]
    )
}

#[unsafe(no_mangle)]
fn main() -> i32 {
    let devices = scan();
    let Some(device) = devices.net else {
        println!("t_net: no network device attached: skipped");
        return 0;
    };
    let (running, end) = start_driver("ethd", device).expect("ethd starts");
    let port = port_create().expect("port");
    let wanted = channel_signals::READABLE | channel_signals::PEER_CLOSED;
    port_subscribe(port, end, wanted, 1, Mode::Level).expect("subscribe");
    let memory = memory_create(OBJECT_SIZE, MemoryFlags::CONTIGUOUS).expect("frames object");
    let frames = memory_map(memory, 0, OBJECT_SIZE, MapFlags::WRITE).expect("map");
    let nic = Nic { end, port, frames };
    let mut keep = alloc::vec::Vec::new();

    // The device: an address, a link.
    nic.send(1, &Info {}, None);
    let (mac, link) = {
        let r: InfoReply<'_> = nic.reply(1, &mut keep);
        assert_eq!(r.status.0, status::OK);
        let mut mac = [0u8; 6];
        assert_eq!(r.mac.0.len(), 6);
        mac.copy_from_slice(r.mac.0);
        (mac, r.link)
    };
    println!("t_net: mac {} link {}", mac_text(mac), link);
    assert!(link);
    assert_eq!(mac[0] & 1, 0, "a unicast address");
    assert_ne!(mac, [0; 6]);

    // Hostile requests before the attach: nothing is carried out.
    nic.send(
        NO_REPLY,
        &Release {
            slots: Seq::Of(&[Slot { slot: 0 }, Slot { slot: 9999 }]),
        },
        None,
    );
    let bad = [Frame {
        slot: 64,
        length: 60,
    }];
    nic.send(
        NO_REPLY,
        &Transmit {
            frames: Seq::Of(&bad),
        },
        None,
    );
    match nic.next_not_received(WAIT_NS) {
        Push::Rejected(r) => assert_eq!(r, [(64, reason::NOT_ATTACHED)], "refused, not sent"),
        _ => panic!("expected a refusal"),
    }
    // A reply that is not an attach: Attach without a handle is refused.
    nic.send(2, &Attach {}, None);
    {
        let r: Done = nic.reply(2, &mut keep);
        assert_eq!(r.status.0, status::BAD_OBJECT);
    }

    // The attach.
    nic.send(3, &Attach {}, Some(memory));
    {
        let r: Done = nic.reply(3, &mut keep);
        assert_eq!(r.status.0, status::OK, "attach");
    }
    nic.send(4, &Attach {}, Some(memory));
    {
        let r: Done = nic.reply(4, &mut keep);
        assert_eq!(r.status.0, status::BAD_OBJECT, "a second attach");
    }

    // Hostile transmits after it: out-of-range slots, a receive slot, bad lengths.
    let hostile = [
        Frame {
            slot: 0,
            length: 60,
        },
        Frame {
            slot: 128,
            length: 60,
        },
        Frame {
            slot: u32::MAX,
            length: 60,
        },
        Frame {
            slot: 64,
            length: 0,
        },
        Frame {
            slot: 64,
            length: 13,
        },
        Frame {
            slot: 64,
            length: 1515,
        },
        Frame {
            slot: 64,
            length: u32::MAX,
        },
    ];
    nic.send(
        NO_REPLY,
        &Transmit {
            frames: Seq::Of(&hostile),
        },
        None,
    );
    // Slots 0, 128 and u32::MAX are no transmit slots: counted, not
    // reported. Slot 64 is refused for its length, never reported as sent.
    match nic.next_not_received(WAIT_NS) {
        Push::Rejected(r) => assert_eq!(r, [(64, reason::BAD_LENGTH)]),
        _ => panic!("expected the refusals"),
    }

    // ARP who-has for the gateway, ROUNDS times; requests for us are answered.
    let who_has = arp::Arp {
        operation: arp::Operation::Request,
        sender_mac: mac,
        sender_ip: PROBE_IP,
        target_mac: [0; 6],
        target_ip: GATEWAY_IP,
    };
    let mut request = [0u8; 60];
    arp::build(&who_has, arp::BROADCAST, &mut request).expect("fits");
    let (mut rtts, mut deliveries) = (alloc::vec::Vec::new(), alloc::vec::Vec::new());
    let mut gateway_mac = None;
    let mut answered = 0u32;
    let mut answer_busy = false;
    for _ in 0..ROUNDS {
        let started = clock_monotonic();
        nic.transmit(SLOTS, &request);
        let (mut sent, mut got) = (false, false);
        let until = started + WAIT_NS;
        while !(sent && got) {
            let left = until
                .checked_sub(clock_monotonic())
                .expect("a reply in time");
            match nic.next(left).expect("the driver pushes") {
                Push::Sent(s) => {
                    // The answer's slot (SLOTS + 1) may be reported too.
                    sent |= s.contains(&SLOTS);
                    answer_busy &= !s.contains(&(SLOTS + 1));
                }
                Push::Rejected(r) => panic!("a refused transmit: {r:?}"),
                Push::Link => {}
                Push::Received(at, list) => {
                    deliveries.push(clock_monotonic() - at);
                    for f in list {
                        assert!(f.slot < SLOTS && f.length >= arp::ETHERNET as u32);
                        let frame = nic.read_slot(f.slot, f.length as usize);
                        let mut out = [0u8; 64];
                        if let Some(n) = arp::answer(&frame, mac, OUR_IP, &mut out) {
                            // Answered by hand through the second transmit
                            // slot, once it is reported free again.
                            if !answer_busy {
                                nic.transmit(SLOTS + 1, &out[..n]);
                                answer_busy = true;
                                answered += 1;
                            }
                        } else if let Some(a) = arp::parse(&frame)
                            && a.operation == arp::Operation::Reply
                            && a.sender_ip == GATEWAY_IP
                            && a.target_ip == PROBE_IP
                        {
                            assert_eq!(a.target_mac, mac);
                            gateway_mac = Some(a.sender_mac);
                            rtts.push(clock_monotonic() - started);
                            got = true;
                        }
                        nic.release(f.slot);
                    }
                }
                Push::Reply(_) => panic!("an unasked reply"),
            }
        }
    }
    let gateway = gateway_mac.expect("an ARP reply from the gateway");
    println!(
        "t_net: arp reply from 10.0.2.2 at {} ({} rounds, {} requests for us answered)",
        mac_text(gateway),
        ROUNDS,
        answered
    );
    rtts.sort_unstable();
    deliveries.sort_unstable();
    let us = |v: &[u64], q: usize| v[(v.len() - 1) * q / 100] / 1000;
    println!(
        "t_net: measure: arp round trip median {} us, min {} us, max {} us; interrupt to message read median {} us, max {} us",
        us(&rtts, 50),
        us(&rtts, 0),
        us(&rtts, 100),
        us(&deliveries, 50),
        us(&deliveries, 100)
    );

    // The host connects to a forwarded port (`make test-net` does, repeatedly),
    // so SLIRP asks who has 10.0.2.15: wait a while for it and answer
    // through the transmit path. The step greps the pcap for the reply.
    let hold = clock_monotonic() + HOLD_NS;
    while answered == 0 || answer_busy {
        let Some(left) = hold.checked_sub(clock_monotonic()) else {
            break;
        };
        match nic.next(left) {
            Some(Push::Received(_, list)) => {
                for f in list {
                    let frame = nic.read_slot(f.slot, f.length as usize);
                    let mut out = [0u8; 64];
                    if !answer_busy && let Some(n) = arp::answer(&frame, mac, OUR_IP, &mut out) {
                        nic.transmit(SLOTS + 1, &out[..n]);
                        answer_busy = true;
                        answered += 1;
                    }
                    nic.release(f.slot);
                }
            }
            Some(Push::Sent(s)) => answer_busy &= !s.contains(&(SLOTS + 1)),
            Some(_) => {}
            None => break,
        }
    }
    println!("t_net: requests for 10.0.2.15 answered: {answered}");

    // The client goes away. The driver must notice, reset the device and
    // unpin both memories, and end with code 0; a driver that is killed
    // instead leaves them quarantined (ADR-0054), which this does not test.
    let _ = handle_close(end);
    let _ = handle_close(memory);
    let ended = wait_exit_within(running.process, WAIT_NS);
    match ended {
        Some(record) => {
            assert_eq!(record.reason, ExitReason::Exited, "ethd ended by itself");
            assert_eq!(record.code, 0, "ethd reset the device and unpinned");
            println!("t_net: ethd ended by itself: device reset, memories unpinned");
        }
        None => panic!(
            "ethd did not end after its client closed the channel (it would be killed, quarantine path)"
        ),
    }
    running.end();
    // Nothing is mapped, pinned or quarantined any more.
    dma_release_quarantine(device.0).expect("nothing left to release");
    println!("t_net: all passed");
    0
}
