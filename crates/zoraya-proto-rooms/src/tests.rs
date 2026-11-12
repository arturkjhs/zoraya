use super::*;
use std::vec::Vec;
use zoraya_protocol::testing::exercise;
use zoraya_protocol::{Message, decode, encode};

macro_rules! round {
    ($t:ty, $m:expr, $handles:expr) => {{
        let m = $m;
        let mut out = std::vec![0u8; 16384];
        let n = encode(&m, 7, &mut out).unwrap();
        let (h, back) = decode::<$t>(&out[..n], $handles).unwrap();
        assert_eq!(h.transaction, 7);
        assert_eq!(back, m);
        if n <= 4096 {
            exercise(&out[..n], |b| {
                let _ = decode::<$t>(b, $handles);
            });
        } else {
            // `exercise` takes up to 4096 bytes: cut the long one by hand.
            for cut in (0..n).step_by(97) {
                let _ = decode::<$t>(&out[..cut], $handles);
            }
        }
    }};
}

#[test]
fn every_message_round_trips_and_decodes_totally() {
    round!(
        Launch<'_>,
        Launch {
            room: Text("agent-1")
        },
        0
    );
    round!(Leave<'_>, Leave { room: Text("") }, 0);
    round!(
        Set<'_>,
        Set {
            room: Text("project")
        },
        0
    );
    round!(Usage<'_>, Usage { room: Text("") }, 0);
    round!(
        Report<'_>,
        Report {
            service: Below(3),
            used: 4,
            limit: 128,
            refused: 1,
            room: Text("agent-1"),
        },
        0
    );
    round!(
        Stop<'_>,
        Stop {
            room: Text("agent-1")
        },
        0
    );
    round!(
        Open<'_>,
        Open {
            kind: Below(0),
            room: Text("agent-1"),
        },
        0
    );
    round!(Opened, Opened {}, 1);
    round!(Launched, Launched {}, 1);
    round!(Done, Done {}, 0);
    round!(
        Denied<'_>,
        Denied {
            status: Below(status::BUSY),
            text: Text("agent-1: running from the desktop"),
        },
        0
    );
    round!(
        Refused<'_>,
        Refused {
            field: Below(field::MEMORY),
            used: 64 << 20,
            limit: 64 << 20,
            room: Text("agent-1"),
        },
        0
    );
    round!(
        Keep<'_>,
        Keep {
            untrusted: Below(0),
            room_untrusted: Below(1),
            launcher: Below(2),
            room: Text("agent-1"),
        },
        1
    );
    round!(
        Forget<'_>,
        Forget {
            room: Text("agent-1")
        },
        0
    );
    round!(
        Attach<'_>,
        Attach {
            kind: Below(attach::ROOM),
            room: Text("agent-1"),
        },
        1
    );
    let mut records = Vec::new();
    for i in 0..MAX_ROOMS {
        let mut r = UsageRecord::default();
        r.set_name(&std::format!("r{i}"));
        r.mem_used = i as u64;
        let mut b = [0u8; RECORD_SIZE];
        r.encode(&mut b).unwrap();
        records.extend_from_slice(&b);
    }
    round!(
        UsageList<'_>,
        UsageList {
            count: Below(MAX_ROOMS as u32),
            records: Data(&records),
        },
        0
    );
}

#[test]
fn wrong_handles_and_out_of_range_values_are_refused() {
    let mut out = [0u8; 256];
    let n = encode(&Launch { room: Text("x") }, 1, &mut out).unwrap();
    assert!(decode::<Launch<'_>>(&out[..n], 1).is_err());
    let n = encode(&Launched {}, 1, &mut out).unwrap();
    assert!(decode::<Launched>(&out[..n], 0).is_err());
    // A room name over its bound is refused on the wire.
    let long = "a".repeat(MAX_ROOM + 1);
    assert!(encode(&Launch { room: Text(&long) }, 1, &mut out).is_err());
    // A status past the enumeration does not decode.
    let n = encode(
        &Denied {
            status: Below(status::COUNT - 1),
            text: Text(""),
        },
        1,
        &mut out,
    )
    .unwrap();
    out[16] = status::COUNT as u8;
    assert!(decode::<Denied<'_>>(&out[..n], 0).is_err());
    assert_eq!(<Launch<'_> as Message>::ORDINAL, ordinal::LAUNCH);
}

#[test]
fn the_ordinal_sets_are_disjoint_by_direction() {
    let all = [
        <Launch<'_> as Message>::ORDINAL,
        <Leave<'_> as Message>::ORDINAL,
        <Set<'_> as Message>::ORDINAL,
        <Usage<'_> as Message>::ORDINAL,
        <Report<'_> as Message>::ORDINAL,
        <Stop<'_> as Message>::ORDINAL,
        <Open<'_> as Message>::ORDINAL,
        <Opened as Message>::ORDINAL,
        <Launched as Message>::ORDINAL,
        <Done as Message>::ORDINAL,
        <Denied<'_> as Message>::ORDINAL,
        <UsageList<'_> as Message>::ORDINAL,
        <Refused<'_> as Message>::ORDINAL,
        <Keep<'_> as Message>::ORDINAL,
        <Attach<'_> as Message>::ORDINAL,
        <Forget<'_> as Message>::ORDINAL,
    ];
    for (i, a) in all.iter().enumerate() {
        assert!(!all[i + 1..].contains(a), "{a} twice");
    }
    for r in MANAGE_REQUESTS {
        assert!(all.contains(&r));
    }
    assert!(ROOM_REQUESTS.iter().all(|r| MANAGE_REQUESTS.contains(r)));
    assert!(USAGE_REQUESTS.iter().all(|r| ROOM_REQUESTS.contains(r)));
}

#[test]
fn a_usage_record_round_trips_and_any_bytes_decode() {
    let mut r = UsageRecord {
        flags: flags::RUNNING | flags::UNTRUSTED,
        launcher: Launcher::Console.raw(),
        mem_used: 1,
        mem_limit: 2,
        mem_peak: 3,
        mem_refused: 4,
        procs_used: 5,
        procs_limit: 6,
        procs_refused: 7,
        cpu_limit: 250,
        cpu_used_ns: 9,
        disk_limit: NONE,
        conns_limit: 16,
        tmp_limit: 17,
        reports: [[1, 2, 3, 4], [0; 4], [2, 5, 6, 7], [0; 4]],
        ..UsageRecord::default()
    };
    r.set_name("agent-1");
    let mut b = [0u8; RECORD_SIZE];
    r.encode(&mut b).unwrap();
    assert_eq!(UsageRecord::decode(&b), Some(r));
    assert_eq!(r.name(), "agent-1");
    assert_eq!(UsageRecord::decode(&b[..RECORD_SIZE - 1]), None);
    assert!(r.encode(&mut [0u8; 10]).is_none());
    for n in 0..=255u8 {
        let junk = [n; RECORD_SIZE];
        let d = UsageRecord::decode(&junk).unwrap();
        let _ = d.name();
    }
    assert_eq!(Launcher::from_raw(1), Some(Launcher::Desktop));
    assert_eq!(Launcher::from_raw(0), None);
    assert_eq!(Launcher::Desktop.phrase(), "from the desktop");
}

#[test]
fn room_names_are_bounded() {
    use alloc_free::RoomName;
    assert!(RoomName::new("").is_none());
    assert!(RoomName::new(&"a".repeat(MAX_ROOM + 1)).is_none());
    assert_eq!(RoomName::new("agent-1").unwrap().as_str(), "agent-1");
}
