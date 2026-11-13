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
    for name in ["", "google.com", "a"] {
        let ask = Ping {
            address: 0x0a00_0202,
            count: Below(4),
            timeout_ms: Below(2000),
            name: Data(name.as_bytes()),
        };
        let mut out = [0; 512];
        let length = encode(&ask, 1, &mut out).unwrap();
        let (_, back) = decode::<Ping<'_>>(&out[..length], 0).unwrap();
        assert_eq!(
            (back.address, back.count.0, back.name.0),
            (0x0a00_0202, 4, name.as_bytes())
        );
        exercise(&out[..length], |bytes| {
            let _ = decode::<Ping<'_>>(bytes, 0);
        });
    }
    let rtts = [Rtt { us: 10 }, Rtt { us: NO_REPLY }];
    let ping = PingReply {
        status: Below(status::OK),
        address: 0x0a00_0202,
        sent: 4,
        received: 4,
        rtt_min_us: 10,
        rtt_avg_us: 20,
        rtt_max_us: 30,
        rtts: Seq::Of(&rtts),
    };
    let mut out = [0; 2048];
    let length = encode(&ping, 1, &mut out).unwrap();
    let (_, back) = decode::<PingReply<'_>>(&out[..length], 0).unwrap();
    assert_eq!(
        back.rtts
            .iter()
            .map(|r| r.us)
            .collect::<alloc::vec::Vec<_>>(),
        [10, NO_REPLY]
    );
    assert_eq!((back.sent, back.rtt_max_us), (4, 30));
    exercise(&out[..length], |bytes| {
        let _ = decode::<PingReply<'_>>(bytes, 0);
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
    total(Failed {
        reason: Below(reason::OVER_QUOTA),
    });
    assert!(encode(&Failed { reason: Below(7) }, 1, &mut out).is_err());
    // N4: the account's budget, with its handle.
    let scope = zoraya_abi::net::Scope::EMPTY.as_bytes();
    let carve = NarrowAccount {
        rights: 1,
        budget: Below(4),
        scope: Data(scope),
    };
    let length = encode(&carve, 1, &mut out).unwrap();
    assert!(decode::<NarrowAccount<'_>>(&out[..length], 1).is_ok());
    assert_eq!(
        decode::<NarrowAccount<'_>>(&out[..length], 0).err(),
        Some(WireError::WrongHandles)
    );
    exercise(&out[..length], |bytes| {
        let _ = decode::<NarrowAccount<'_>>(bytes, 1);
    });
    let too_many = NarrowAccount {
        budget: Below(17),
        ..carve
    };
    assert!(encode(&too_many, 1, &mut out).is_err());
    total(Narrowed {
        status: Below(status::OVER_QUOTA),
    });
    assert!(encode(&Narrowed { status: Below(8) }, 1, &mut out).is_err());
    // N4: `resolve` and its answer.
    let length = encode(
        &Resolve {
            name: Data(b"test.zoraya"),
        },
        1,
        &mut out,
    )
    .unwrap();
    assert_eq!(
        decode::<Resolve<'_>>(&out[..length], 0).unwrap().1.name.0,
        b"test.zoraya"
    );
    exercise(&out[..length], |bytes| {
        let _ = decode::<Resolve<'_>>(bytes, 0);
    });
    let long = [b'a'; 254];
    assert!(encode(&Resolve { name: Data(&long) }, 1, &mut out).is_err());
    let found = [Address { address: 1 }, Address { address: 2 }];
    let answer = Resolved {
        status: Below(status::OK),
        addresses: Seq::Of(&found),
    };
    let length = encode(&answer, 1, &mut out).unwrap();
    let (_, back) = decode::<Resolved<'_>>(&out[..length], 0).unwrap();
    assert_eq!(back.addresses.iter().count(), 2);
    exercise(&out[..length], |bytes| {
        let _ = decode::<Resolved<'_>>(bytes, 0);
    });
    let nine = [Address { address: 1 }; 9];
    assert!(
        encode(
            &Resolved {
                status: Below(status::OK),
                addresses: Seq::Of(&nine),
            },
            1,
            &mut out
        )
        .is_err()
    );
    assert!(
        encode(
            &Resolved {
                status: Below(status::LOCAL),
                addresses: Seq::Of(&[]),
            },
            1,
            &mut out
        )
        .is_ok()
    );
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
        name: Data(b""),
    };
    assert!(encode(&ping, 1, &mut out).is_err());
    let ping = Ping {
        address: 1,
        count: Below(1),
        timeout_ms: Below(30001),
        name: Data(b""),
    };
    assert!(encode(&ping, 1, &mut out).is_err());
    // A name has the bound of connect's: 253 bytes encode, 254 do not.
    let long = [b'a'; 254];
    let mut big = [0; 1024];
    let ping = |name| Ping {
        address: 0,
        count: Below(1),
        timeout_ms: Below(1),
        name: Data(name),
    };
    assert!(encode(&ping(&long[..253]), 1, &mut big).is_ok());
    assert!(encode(&ping(&long), 1, &mut big).is_err());
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

fn view(rtts: &[u32], sent: u32, received: u32) -> PingView<'_> {
    PingView {
        host: "10.0.2.2",
        address: [10, 0, 2, 2],
        count: sent,
        sent,
        received,
        rtt_min_us: 412,
        rtt_avg_us: 1500,
        rtt_max_us: 12_345,
        rtts,
    }
}

#[test]
fn ping_prints_a_line_per_reply_and_the_summary() {
    let text = render_ping(&view(&[412, NO_REPLY, 1000, 12_345], 4, 3));
    assert_eq!(
        text,
        "PING 10.0.2.2: 4 requests\n\
         40 bytes from 10.0.2.2: seq=0 time=0.412 ms\n\
         40 bytes from 10.0.2.2: seq=2 time=1.000 ms\n\
         40 bytes from 10.0.2.2: seq=3 time=12.345 ms\n\
         --- 10.0.2.2 ping statistics ---\n\
         4 sent, 3 received, 25% loss\n\
         rtt min/avg/max = 0.412/1.500/12.345 ms\n"
    );
}

#[test]
fn ping_with_no_reply_has_no_lines_and_no_rtt() {
    let text = render_ping(&view(&[NO_REPLY, NO_REPLY], 2, 0));
    assert_eq!(
        text,
        "PING 10.0.2.2: 2 requests\n--- 10.0.2.2 ping statistics ---\n2 sent, 0 received, 100% loss\n"
    );
    // Nothing sent is all lost, not a division by zero.
    assert!(render_ping(&view(&[], 0, 0)).contains("100% loss"));
}

#[test]
fn ping_by_name_shows_the_name_and_the_address_it_resolved_to() {
    let rtts = [250];
    let text = render_ping(&PingView {
        host: "example.org",
        address: [93, 184, 216, 34],
        count: 1,
        sent: 1,
        received: 1,
        rtt_min_us: 250,
        rtt_avg_us: 250,
        rtt_max_us: 250,
        rtts: &rtts,
    });
    assert!(text.starts_with(
        "PING example.org (93.184.216.34): 1 requests\n40 bytes from 93.184.216.34: seq=0"
    ));
    assert!(text.contains("--- example.org ping statistics ---"));
}
