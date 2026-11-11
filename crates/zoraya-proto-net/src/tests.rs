use super::*;
use std::collections::HashSet;
use zoraya_protocol::testing::exercise;
use zoraya_protocol::{Message, WireError, decode, encode};

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

fn reply(dns: &[Address]) -> StatusReply<'_> {
    StatusReply {
        status: Below(status::OK),
        link: true,
        address_source: Below(source::DHCP),
        dns_source: Below(source::DHCP),
        prefix: Below(24),
        address: u32::from_be_bytes([10, 0, 2, 15]),
        gateway: u32::from_be_bytes([10, 0, 2, 2]),
        lease_left: 7140,
        frames_in: 9,
        frames_out: 8,
        frames_dropped: 1,
        mac: Data(&[0x52, 0x54, 0, 0x12, 0x34, 0x56]),
        dns: Seq::Of(dns),
    }
}

#[test]
fn every_message_round_trips_and_decodes_totally() {
    total(GetStatus {});
    total(Ping {
        address: 0x0a00_0202,
        count: Below(4),
        timeout_ms: Below(2000),
    });
    total(PingReply {
        status: Below(status::OK),
        sent: 4,
        received: 4,
        rtt_min_us: 10,
        rtt_avg_us: 20,
        rtt_max_us: 30,
    });
    total(Narrowed {
        status: Below(status::NOT_GRANTED),
    });
    let dns = [Address { address: 1 }, Address { address: 2 }];
    let r = reply(&dns);
    let mut out = [0; 2048];
    let length = encode(&r, 1, &mut out).unwrap();
    let (_, back) = decode::<StatusReply<'_>>(&out[..length], 0).unwrap();
    assert_eq!(back.dns.iter().count(), 2);
    assert_eq!(back.address, r.address);
    exercise(&out[..length], |bytes| {
        let _ = decode::<StatusReply<'_>>(bytes, 0);
    });
    let scope = zoraya_abi::net::Scope::EMPTY.as_bytes();
    let n = Narrow {
        rights: zoraya_abi::net::rights::STATUS,
        scope: Data(scope),
    };
    let length = encode(&n, 1, &mut out).unwrap();
    let (_, back) = decode::<Narrow<'_>>(&out[..length], 1).unwrap();
    assert_eq!(back.scope.0, scope);
    exercise(&out[..length], |bytes| {
        let _ = decode::<Narrow<'_>>(bytes, 1);
    });
}

#[test]
fn connect_and_the_connection_channel_messages_round_trip_and_are_total() {
    let name = "test.zoraya";
    let c = Connect {
        name: Data(name.as_bytes()),
        address: 0,
        port: Below(8080),
    };
    let mut out = [0; 2048];
    let length = encode(&c, 1, &mut out).unwrap();
    let (_, back) = decode::<Connect<'_>>(&out[..length], 3).unwrap();
    assert_eq!(
        (back.name.0, back.address, back.port.0),
        (name.as_bytes(), 0, 8080)
    );
    exercise(&out[..length], |bytes| {
        let _ = decode::<Connect<'_>>(bytes, 3);
    });
    // Exactly three handles.
    for wrong in [0, 1, 2, 4] {
        assert_eq!(
            decode::<Connect<'_>>(&out[..length], wrong).err(),
            Some(WireError::WrongHandles)
        );
    }
    // A port above 65535 and a name over 253 bytes do not encode.
    let bad = Connect {
        name: Data(&[]),
        address: 1,
        port: Below(65536),
    };
    assert!(encode(&bad, 1, &mut out).is_err());
    let long = [b'a'; 254];
    let bad = Connect {
        name: Data(&long),
        address: 0,
        port: Below(80),
    };
    assert!(encode(&bad, 1, &mut out).is_err());
    total(Connecting {
        status: Below(status::NOT_GRANTED),
    });
    total(Connected {
        address: 0x0a00_0202,
        port: Below(80),
    });
    total(Failed {
        reason: Below(reason::OVER_LIMIT),
    });
    total(Reset {
        why: Below(reset::ABORTED),
    });
    assert!(encode(&Failed { reason: Below(6) }, 1, &mut out).is_err());
}

#[test]
fn extra_or_missing_handles_and_values_out_of_range_are_refused() {
    let mut out = [0; 256];
    let n = Narrow {
        rights: 1,
        scope: Data(&[1, 0, 0, 0]),
    };
    let length = encode(&n, 1, &mut out).unwrap();
    assert_eq!(
        decode::<Narrow<'_>>(&out[..length], 0).err(),
        Some(WireError::WrongHandles)
    );
    assert_eq!(
        decode::<Narrow<'_>>(&out[..length], 2).err(),
        Some(WireError::WrongHandles)
    );
    let length = encode(&GetStatus {}, 1, &mut out).unwrap();
    assert!(decode::<GetStatus>(&out[..length], 1).is_err());
    // Counts above the bound do not encode.
    let ping = Ping {
        address: 1,
        count: Below(17),
        timeout_ms: Below(1),
    };
    assert!(encode(&ping, 1, &mut out).is_err());
    let ping = Ping {
        address: 1,
        count: Below(1),
        timeout_ms: Below(30001),
    };
    assert!(encode(&ping, 1, &mut out).is_err());
}

#[test]
fn ordinals_are_distinct() {
    let ordinals: HashSet<u32> = ORDINALS.iter().map(|o| o.1).collect();
    assert_eq!(ordinals.len(), ORDINALS.len());
    assert_eq!(<GetStatus as Message>::ORDINAL, 1);
    assert_eq!(<Ping as Message>::ORDINAL, 2);
    assert_eq!(<Narrow<'_> as Message>::ORDINAL, 6);
}

#[test]
fn status_is_printed_in_words() {
    let dns = [Address {
        address: u32::from_be_bytes([10, 0, 2, 3]),
    }];
    let text = render(&reply(&dns));
    assert_eq!(
        text,
        "net0  up  52:54:00:12:34:56\n\
         \x20 address   10.0.2.15/24 (dhcp, lease 1h 59m left)\n\
         \x20 gateway   10.0.2.2\n\
         \x20 dns       10.0.2.3 (dhcp)\n\
         \x20 frames    9 in, 8 out, 1 dropped\n"
    );
    let mut none = reply(&[]);
    none.address_source = Below(source::NONE);
    none.address = 0;
    none.gateway = 0;
    none.link = false;
    let text = render(&none);
    assert!(text.contains("down"));
    assert!(text.contains("address   none"));
    assert!(!text.contains("gateway"));
    let mut stat = reply(&[]);
    stat.address_source = Below(source::STATIC);
    stat.lease_left = NO_LEASE;
    assert!(render(&stat).contains("10.0.2.15/24 (static)"));
}
