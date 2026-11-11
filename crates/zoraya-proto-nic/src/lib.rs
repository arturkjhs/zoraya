//! The frame protocol between a network service and a NIC driver (ADR-0221
//! item 2, ADR-0052). The service creates one contiguous memory object of
//! `2 * SLOTS` slots of `SLOT_SIZE` bytes, sends it with `Attach`, and the
//! two sides pass slot numbers: slots `0..SLOTS` receive, slots
//! `SLOTS..2 * SLOTS` transmit. A slot holds one bare Ethernet frame from
//! its first byte (ADR-0280). The driver treats every message here as
//! hostile; the service treats every frame as hostile.

#![no_std]
#![forbid(unsafe_code)]

use zoraya_protocol::{Below, Data, Seq, protocol};

/// Slots in each direction (ADR-0221 item 2: 64 `[assumption]`).
pub const SLOTS: u32 = 64;
/// Bytes of one slot.
pub const SLOT_SIZE: u32 = 1536;
/// The bytes of the object: both directions.
pub const OBJECT_SIZE: u64 = 2 * SLOTS as u64 * SLOT_SIZE as u64;
/// The longest frame: 14 bytes of Ethernet header and 1500 of payload.
pub const MAX_FRAME: u32 = 1514;
/// The shortest frame the driver carries: an Ethernet header.
pub const MIN_FRAME: u32 = 14;
/// Slots in one message.
pub const MAX_BATCH: usize = 64;

/// One of `status`.
pub type Status = Below<4>;

/// The status of every reply.
pub mod status {
    pub const OK: u32 = 0;
    /// A memory object is already attached, or the object is not one the
    /// driver can use (not one run of device addresses, or too small).
    pub const BAD_OBJECT: u32 = 1;
    /// No memory object is attached.
    pub const NOT_ATTACHED: u32 = 2;
    /// The driver does not know the ordinal.
    pub const UNSUPPORTED: u32 = 3;
}

protocol! {
    /// A slot and the bytes of its frame (zero where the length means
    /// nothing).
    record Frame {
        slot: u32,
        length: u32,
    }

    /// A refused transmit: the slot and why (one of `reason`).
    record Refusal {
        slot: u32,
        reason: u32,
    }

    /// A slot.
    record Slot {
        slot: u32,
    }

    /// What the device is.
    message Info = 1, handles 0 {}
    message InfoReply<'a> = 1, handles 0 {
        status: Status,
        link: bool,
        mac: Data<'a, 6>,
    }

    /// Attaches the memory object the slots lie in. The driver pins it and
    /// offers every receive slot to the device.
    message Attach = 2, handles 1 {}
    message Done = 2, handles 0 {
        status: Status,
    }

    /// Frames to send, each in a transmit slot. No reply: the driver
    /// answers with `Sent` when the device is done with the slot. A frame
    /// that is not carried out (not attached, not free, of a bad length) is
    /// refused: the driver counts it and answers `Rejected` with the
    /// reason. A rejected frame is never reported in `Sent` for that
    /// request. A slot out of range is counted and not reported.
    message Transmit<'a> = 3, handles 0 {
        frames: Seq<'a, Frame, MAX_BATCH>,
    }

    /// Receive slots the service has read and gives back.
    message Release<'a> = 4, handles 0 {
        slots: Seq<'a, Slot, MAX_BATCH>,
    }

    /// Frames received, pushed by the driver. `at` is the monotonic time
    /// in nanoseconds at which the driver woke for the interrupt. The slots
    /// are the service's until it releases them.
    message Received<'a> = 16, handles 0 {
        at: u64,
        frames: Seq<'a, Frame, MAX_BATCH>,
    }

    /// Transmit slots the driver is done with, pushed.
    message Sent<'a> = 17, handles 0 {
        slots: Seq<'a, Slot, MAX_BATCH>,
    }

    /// Transmit requests the driver refused, pushed. Whether the slot is
    /// the service's again depends on the reason: `reason::STILL_QUEUED`
    /// means that the device still holds it (or its `Sent` is still to
    /// come), and the service waits for that `Sent`; any other reason
    /// leaves the slot with the service, which has not been taken.
    message Rejected<'a> = 19, handles 0 {
        slots: Seq<'a, Refusal, MAX_BATCH>,
    }

    /// The link changed, pushed.
    message LinkChanged = 18, handles 0 {
        link: bool,
    }
}

/// Why a transmit was refused, in `Refusal`.
pub mod reason {
    /// No memory object is attached.
    pub const NOT_ATTACHED: u32 = 1;
    /// The slot is not a transmit slot. Counted by the driver, never sent.
    pub const BAD_SLOT: u32 = 2;
    /// The slot is with the device, or finished and not yet reported in
    /// `Sent`: the service must wait for that `Sent`.
    pub const STILL_QUEUED: u32 = 3;
    /// The length is outside `MIN_FRAME..=MAX_FRAME`.
    pub const BAD_LENGTH: u32 = 4;
}

/// Every ordinal, for a test that none repeats.
pub const ORDINALS: [(&str, u32); 8] = [
    ("info", 1),
    ("attach", 2),
    ("transmit", 3),
    ("release", 4),
    ("received", 16),
    ("sent", 17),
    ("rejected", 19),
    ("link_changed", 18),
];

/// Ethernet and ARP over IPv4 (RFC 826), the only frames N1 reads. Every
/// access is a checked slice, so no frame of any length panics; a frame that
/// is not a well-formed IPv4 ARP over Ethernet is `None`.
pub mod arp {
    /// Bytes of the Ethernet header.
    pub const ETHERNET: usize = 14;
    /// Bytes of an ARP message for IPv4 over Ethernet.
    pub const MESSAGE: usize = 28;
    /// A frame that carries one, without padding.
    pub const FRAME: usize = ETHERNET + MESSAGE;
    pub const BROADCAST: [u8; 6] = [0xff; 6];
    const ETHERTYPE_ARP: [u8; 2] = [0x08, 0x06];
    const HEADER: [u8; 6] = [0, 1, 8, 0, 6, 4];

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum Operation {
        Request,
        Reply,
    }

    /// A parsed ARP message.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct Arp {
        pub operation: Operation,
        pub sender_mac: [u8; 6],
        pub sender_ip: [u8; 4],
        pub target_mac: [u8; 6],
        pub target_ip: [u8; 4],
    }

    fn array<const N: usize>(bytes: &[u8], at: usize) -> Option<[u8; N]> {
        bytes.get(at..at.checked_add(N)?)?.try_into().ok()
    }

    /// The ARP message of an Ethernet frame. Trailing padding is allowed.
    /// Hardware type 1, protocol type 0x0800, sizes 6 and 4 and operation 1
    /// or 2 are required.
    pub fn parse(frame: &[u8]) -> Option<Arp> {
        if frame.get(12..14)? != ETHERTYPE_ARP {
            return None;
        }
        let m = frame.get(ETHERNET..ETHERNET + MESSAGE)?;
        if m[..6] != HEADER {
            return None;
        }
        let operation = match (m[6], m[7]) {
            (0, 1) => Operation::Request,
            (0, 2) => Operation::Reply,
            _ => return None,
        };
        Some(Arp {
            operation,
            sender_mac: array(m, 8)?,
            sender_ip: array(m, 14)?,
            target_mac: array(m, 18)?,
            target_ip: array(m, 24)?,
        })
    }

    /// Writes the frame for `a` to `out`, to `destination`; the length, or
    /// `None` if `out` is shorter than `FRAME`.
    pub fn build(a: &Arp, destination: [u8; 6], out: &mut [u8]) -> Option<usize> {
        let out = out.get_mut(..FRAME)?;
        out[..6].copy_from_slice(&destination);
        out[6..12].copy_from_slice(&a.sender_mac);
        out[12..14].copy_from_slice(&ETHERTYPE_ARP);
        out[14..20].copy_from_slice(&HEADER);
        out[20..22].copy_from_slice(&[
            0,
            match a.operation {
                Operation::Request => 1,
                Operation::Reply => 2,
            },
        ]);
        out[22..28].copy_from_slice(&a.sender_mac);
        out[28..32].copy_from_slice(&a.sender_ip);
        out[32..38].copy_from_slice(&a.target_mac);
        out[38..42].copy_from_slice(&a.target_ip);
        Some(FRAME)
    }

    /// The reply to a request for `ip`, from a host with `mac`: written to
    /// `out`, its length. `None` for anything that is not a request for
    /// `ip` (a probe, with a zero sender address, is answered; a request
    /// whose sender is the broadcast or a multicast address is not).
    pub fn answer(frame: &[u8], mac: [u8; 6], ip: [u8; 4], out: &mut [u8]) -> Option<usize> {
        let request = parse(frame)?;
        if request.operation != Operation::Request
            || request.target_ip != ip
            || request.sender_mac[0] & 1 != 0
        {
            return None;
        }
        let reply = Arp {
            operation: Operation::Reply,
            sender_mac: mac,
            sender_ip: ip,
            target_mac: request.sender_mac,
            target_ip: request.sender_ip,
        };
        build(&reply, request.sender_mac, out)
    }
}

#[cfg(test)]
extern crate std;

#[cfg(test)]
mod tests {
    use super::*;
    use zoraya_protocol::testing::exercise;
    use zoraya_protocol::{Message, decode, encode};

    fn total<M>(message: M)
    where
        M: for<'b> Message<'b> + PartialEq + core::fmt::Debug,
    {
        let mut out = [0; 2048];
        let length = encode(&message, 1, &mut out).unwrap();
        let (_, back) = decode::<M>(&out[..length], M::HANDLES).unwrap();
        assert_eq!(back, message);
        exercise(&out[..length], |bytes| {
            let _ = decode::<M>(bytes, M::HANDLES);
        });
    }

    /// Round trip of a message that borrows: `check` sees the decoded one.
    macro_rules! trip {
        ($ty:ident, $value:expr, $check:expr) => {{
            let mut out = [0; 2048];
            let length = encode(&$value, 1, &mut out).unwrap();
            let (_, back) = decode::<$ty<'_>>(&out[..length], 0).unwrap();
            $check(back);
            exercise(&out[..length], |bytes| {
                let _ = decode::<$ty<'_>>(bytes, 0);
            });
        }};
    }

    #[test]
    fn every_message_round_trips_and_decodes_totally() {
        let frames = [Frame {
            slot: 3,
            length: 60,
        }; 4];
        let slots = [Slot { slot: 70 }; 3];
        total(Info {});
        total(Attach {});
        total(Done { status: Below(1) });
        total(LinkChanged { link: false });
        trip!(
            InfoReply,
            InfoReply {
                status: Below(status::OK),
                link: true,
                mac: Data(&[0x52, 0x54, 0, 0x12, 0x34, 0x56]),
            },
            |m: InfoReply<'_>| {
                assert_eq!(m.mac.0, [0x52, 0x54, 0, 0x12, 0x34, 0x56]);
                assert!(m.link);
            }
        );
        trip!(
            Transmit,
            Transmit {
                frames: Seq::Of(&frames)
            },
            |m: Transmit<'_>| assert!(m.frames.iter().eq(frames.iter().copied()))
        );
        trip!(
            Release,
            Release {
                slots: Seq::Of(&slots)
            },
            |m: Release<'_>| assert_eq!(m.slots.len(), 3)
        );
        trip!(
            Received,
            Received {
                at: 99,
                frames: Seq::Of(&frames)
            },
            |m: Received<'_>| assert_eq!((m.at, m.frames.len()), (99, 4))
        );
        trip!(
            Sent,
            Sent {
                slots: Seq::Of(&slots)
            },
            |m: Sent<'_>| assert_eq!(m.slots.len(), 3)
        );
        let refusals = [
            Refusal {
                slot: 70,
                reason: reason::STILL_QUEUED,
            },
            Refusal {
                slot: 71,
                reason: reason::BAD_LENGTH,
            },
        ];
        trip!(
            Rejected,
            Rejected {
                slots: Seq::Of(&refusals)
            },
            |m: Rejected<'_>| assert!(m.slots.iter().eq(refusals.iter().copied()))
        );
    }

    #[test]
    fn a_batch_past_its_limit_is_refused() {
        let slots = [Slot { slot: 1 }; MAX_BATCH + 1];
        let mut out = [0; 4096];
        let refused = encode(
            &Release {
                slots: Seq::Of(&slots),
            },
            1,
            &mut out,
        );
        assert_eq!(refused, Err(zoraya_protocol::WireError::TooLong));
    }

    #[test]
    fn the_object_holds_both_directions() {
        assert_eq!(OBJECT_SIZE, 196_608);
        const { assert!(SLOT_SIZE >= MAX_FRAME) };
    }

    mod arp_tests {
        use crate::arp::*;
        use zoraya_protocol::testing::exercise;

        const US: [u8; 6] = [0x52, 0x54, 0, 0x12, 0x34, 0x56];
        const THEM: [u8; 6] = [0x52, 0x55, 10, 0, 2, 2];

        fn request(target: [u8; 4]) -> ([u8; 64], usize) {
            let a = Arp {
                operation: Operation::Request,
                sender_mac: THEM,
                sender_ip: [10, 0, 2, 2],
                target_mac: [0; 6],
                target_ip: target,
            };
            let mut f = [0; 64];
            let n = build(&a, BROADCAST, &mut f).unwrap();
            (f, n)
        }

        #[test]
        fn a_request_for_us_is_answered_with_our_address() {
            let (f, n) = request([10, 0, 2, 15]);
            let mut out = [0; 64];
            let m = answer(&f[..n], US, [10, 0, 2, 15], &mut out).unwrap();
            assert_eq!(m, FRAME);
            let r = parse(&out[..m]).unwrap();
            assert_eq!(r.operation, Operation::Reply);
            assert_eq!((r.sender_mac, r.sender_ip), (US, [10, 0, 2, 15]));
            assert_eq!((r.target_mac, r.target_ip), (THEM, [10, 0, 2, 2]));
            assert_eq!(out[..6], THEM);
        }

        #[test]
        fn padding_is_allowed() {
            let (f, _) = request([10, 0, 2, 15]);
            assert!(parse(&f[..60]).is_some());
            assert!(parse(&f).is_some());
        }

        #[test]
        fn a_request_for_someone_else_or_a_reply_is_not_answered() {
            let (f, n) = request([10, 0, 2, 99]);
            let mut out = [0; 64];
            assert_eq!(answer(&f[..n], US, [10, 0, 2, 15], &mut out), None);
            let (mut f, n) = request([10, 0, 2, 15]);
            f[21] = 2;
            assert_eq!(answer(&f[..n], US, [10, 0, 2, 15], &mut out), None);
        }

        #[test]
        fn a_multicast_sender_is_not_answered() {
            let (mut f, n) = request([10, 0, 2, 15]);
            f[22] = 0x01;
            let mut out = [0; 64];
            assert_eq!(answer(&f[..n], US, [10, 0, 2, 15], &mut out), None);
        }

        #[test]
        fn every_truncation_is_refused_without_a_panic() {
            let (f, n) = request([10, 0, 2, 15]);
            for length in 0..n {
                assert_eq!(parse(&f[..length]), None, "length {length}");
            }
        }

        #[test]
        fn wrong_hardware_protocol_or_sizes_are_refused() {
            let (f, n) = request([10, 0, 2, 15]);
            // Offsets 14..20: hardware type, protocol type, sizes.
            for at in 14..20 {
                let mut g = f;
                g[at] ^= 0x01;
                assert_eq!(parse(&g[..n]), None, "byte {at}");
            }
            for operation in [0u8, 3, 255] {
                let mut g = f;
                g[21] = operation;
                assert_eq!(parse(&g[..n]), None);
            }
            let mut g = f;
            g[20] = 1;
            assert_eq!(parse(&g[..n]), None);
            let mut g = f;
            g[12] = 0x08;
            g[13] = 0x00;
            assert_eq!(parse(&g[..n]), None);
        }

        #[test]
        fn a_short_output_buffer_is_refused() {
            let (f, n) = request([10, 0, 2, 15]);
            let mut out = [0; FRAME - 1];
            assert_eq!(answer(&f[..n], US, [10, 0, 2, 15], &mut out), None);
        }

        #[test]
        fn no_input_panics() {
            let (f, n) = request([10, 0, 2, 15]);
            exercise(&f[..n], |bytes| {
                let mut out = [0; 64];
                let _ = parse(bytes);
                let _ = answer(bytes, US, [10, 0, 2, 15], &mut out);
            });
        }
    }

    #[test]
    fn no_ordinal_repeats() {
        for (i, (_, a)) in ORDINALS.iter().enumerate() {
            assert_ne!(*a, zoraya_protocol::ERROR_ORDINAL);
            assert!(ORDINALS[i + 1..].iter().all(|(_, b)| a != b));
        }
    }
}

// A runaway test fails at 512 MiB instead of taking the host (docs/backlog.md).
#[cfg(test)]
zoraya_test_alloc::cap_memory!();
