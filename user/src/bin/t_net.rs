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
    ChannelEntry, Handle, MapFlags, MemoryFlags, MemoryObject, Mode, ObjectType, Packet, Status,
    channel_signals,
};
use zoraya_proto_nic::{
    Attach, Done, Frame, Info, InfoReply, OBJECT_SIZE, Received, Release, SLOT_SIZE, SLOTS, Sent,
    Slot, Transmit, arp, status,
};
use zoraya_protocol::{Header, Message, NO_REPLY, Seq, decode, encode};
use zoraya_user::gui::{scan, start_driver};
use zoraya_user::{
    channel_read, channel_write, clock_monotonic, handle_close, memory_create, memory_map,
    port_create, port_subscribe, port_wait, println,
};

const OUR_IP: [u8; 4] = [10, 0, 2, 15];
const GATEWAY_IP: [u8; 4] = [10, 0, 2, 2];
/// Every wait ends: TCG is slow, so seconds.
const WAIT_NS: u64 = 10_000_000_000;
const ROUNDS: usize = 20;

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
    match nic.next(WAIT_NS) {
        Some(Push::Sent(s)) => assert_eq!(s, [64], "a refused transmit is reported at once"),
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
    match nic.next(WAIT_NS) {
        Some(Push::Sent(s)) => assert_eq!(s.len(), hostile.len()),
        _ => panic!("expected the refusals"),
    }

    // ARP who-has for the gateway, ROUNDS times; requests for us are answered.
    let who_has = arp::Arp {
        operation: arp::Operation::Request,
        sender_mac: mac,
        sender_ip: OUR_IP,
        target_mac: [0; 6],
        target_ip: GATEWAY_IP,
    };
    let mut request = [0u8; 60];
    arp::build(&who_has, arp::BROADCAST, &mut request).expect("fits");
    let (mut rtts, mut deliveries) = (alloc::vec::Vec::new(), alloc::vec::Vec::new());
    let mut gateway_mac = None;
    let mut answered = 0u32;
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
                }
                Push::Received(at, list) => {
                    deliveries.push(clock_monotonic() - at);
                    for f in list {
                        assert!(f.slot < SLOTS && f.length >= arp::ETHERNET as u32);
                        let frame = nic.read_slot(f.slot, f.length as usize);
                        let mut out = [0u8; 64];
                        if let Some(n) = arp::answer(&frame, mac, OUR_IP, &mut out) {
                            // Answered by hand through the second transmit slot.
                            nic.transmit(SLOTS + 1, &out[..n]);
                            answered += 1;
                        } else if let Some(a) = arp::parse(&frame)
                            && a.operation == arp::Operation::Reply
                            && a.sender_ip == GATEWAY_IP
                            && a.target_ip == OUR_IP
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

    // The client goes away: the driver resets the device and ends.
    let _ = handle_close(end);
    let _ = handle_close(memory);
    running.end();
    println!("t_net: all passed");
    0
}
