use super::*;

protocol! {
    /// One of each kind.
    message Everything<'a> = 7, handles 1 {
        small: u8,
        big: u64,
        yes: bool,
        bits: Flags<0b101>,
        data: Data<'a, 16>,
        text: Text<'a, 8>,
        name: Name<'a>,
        list: Seq<'a, Entry<'a>, 3>,
    }

    message Empty = 8, handles 0 {}

    record Entry<'a> {
        cookie: u64,
        name: Name<'a>,
    }
}

fn sample<'a>(entries: &'a [Entry<'a>]) -> Everything<'a> {
    Everything {
        small: 9,
        big: 0x1122_3344_5566_7788,
        yes: true,
        bits: Flags(0b100),
        data: Data(b"abc"),
        text: Text("h\u{e9}llo"),
        name: Name("file.txt"),
        list: Seq::Of(entries),
    }
}

const ENTRIES: [Entry<'static>; 2] = [
    Entry {
        cookie: 1,
        name: Name("a"),
    },
    Entry {
        cookie: 2,
        name: Name("bb"),
    },
];

/// INV-0052-2.
#[test]
fn a_message_round_trips() {
    let mut out = [0; 512];
    let length = encode(&sample(&ENTRIES), 5, &mut out).unwrap();
    let (header, m) = decode::<Everything<'_>>(&out[..length], 1).unwrap();
    assert_eq!((header.ordinal, header.transaction), (7, 5));
    assert_eq!(
        (m.small, m.big, m.yes, m.bits.0),
        (9, sample(&ENTRIES).big, true, 0b100)
    );
    assert_eq!(
        (m.data.0, m.text.0, m.name.0),
        (&b"abc"[..], "h\u{e9}llo", "file.txt")
    );
    let entries: std::vec::Vec<_> = m.list.iter().collect();
    assert_eq!(entries, ENTRIES);
}

/// INV-0052-1.
#[test]
fn the_decoder_is_total() {
    let mut out = [0; 512];
    let length = encode(&sample(&ENTRIES), 5, &mut out).unwrap();
    testing::exercise(&out[..length], |bytes| {
        if let Ok((_, m)) = decode::<Everything<'_>>(bytes, 1) {
            for entry in m.list.iter() {
                assert!(Name::is_valid(entry.name.0));
            }
        }
    });
}

/// INV-0052-2: anything but what the encoder writes is refused.
#[test]
fn padding_and_trailing_bytes_are_refused() {
    let mut out = [0; 512];
    let length = encode(&sample(&ENTRIES), 5, &mut out).unwrap();
    // `small` is at 16, and padding runs to `big` at 24.
    for at in 17..24 {
        let mut copy = out;
        copy[at] = 1;
        assert_eq!(
            decode::<Everything<'_>>(&copy[..length], 1).map(|_| ()),
            Err(WireError::BadPadding),
            "{at}"
        );
    }
    let mut longer = out;
    longer[12] += 1;
    assert_eq!(
        decode::<Everything<'_>>(&longer[..length + 1], 1).map(|_| ()),
        Err(WireError::Trailing)
    );
}

#[test]
fn headers_are_checked() {
    let mut out = [0; 64];
    let length = encode(&Empty {}, 1, &mut out).unwrap();
    assert_eq!(length, HEADER_SIZE);
    let check = |bytes: &[u8]| decode::<Empty>(bytes, 0).map(|_| ());
    assert_eq!(check(&out[..length]), Ok(()));
    assert_eq!(check(&out[..length - 1]), Err(WireError::Truncated));
    let mut bad = out;
    bad[0] ^= 1;
    assert_eq!(check(&bad[..length]), Err(WireError::BadMagic));
    let mut bad = out;
    bad[12] = 1;
    assert_eq!(check(&bad[..length]), Err(WireError::BadSize));
    assert_eq!(
        decode::<Empty>(&out[..length], 1).map(|_| ()),
        Err(WireError::WrongHandles)
    );
    assert_eq!(
        decode::<Everything<'_>>(&out[..length], 1).map(|_| ()),
        Err(WireError::WrongOrdinal)
    );
}

#[test]
fn limits_are_checked_both_ways() {
    let mut out = [0; 512];
    let mut m = sample(&ENTRIES);
    m.data = Data(&[0; 17]);
    assert_eq!(encode(&m, 0, &mut out), Err(WireError::TooLong));
    let mut m = sample(&ENTRIES);
    m.name = Name("..");
    assert_eq!(encode(&m, 0, &mut out), Err(WireError::BadValue));
    let many = [ENTRIES[0]; 4];
    assert_eq!(encode(&sample(&many), 0, &mut out), Err(WireError::TooLong));
    assert_eq!(
        encode(&sample(&ENTRIES), 0, &mut out[..20]),
        Err(WireError::NoRoom)
    );
    assert_eq!(encode(&Empty {}, 0, &mut out[..4]), Err(WireError::NoRoom));
}

#[test]
fn a_flag_out_of_its_mask_is_refused() {
    let mut out = [0; 512];
    let mut m = sample(&ENTRIES);
    m.bits = Flags(0b010);
    let length = encode(&m, 0, &mut out).unwrap();
    assert_eq!(
        decode::<Everything<'_>>(&out[..length], 1).map(|_| ()),
        Err(WireError::BadValue)
    );
}

#[test]
fn names_follow_adr_0013() {
    for bad in [
        "",
        ".",
        "..",
        "a/b",
        "a\u{0}",
        "a\u{7f}",
        "a\u{85}",
        &"x".repeat(256),
    ] {
        assert!(!Name::is_valid(bad), "{bad:?}");
    }
    for good in ["a", "...", "\u{e9}t\u{e9}", &"x".repeat(255)] {
        assert!(Name::is_valid(good), "{good:?}");
    }
}

/// INV-0052-4, the wire half: one reply for an unknown ordinal.
#[test]
fn the_error_reply_is_generic() {
    let mut out = [0; 32];
    let length = encode(&ErrorReply { status: 9 }, 3, &mut out).unwrap();
    let (header, reply) = decode::<ErrorReply>(&out[..length], 0).unwrap();
    assert_eq!((header.ordinal, header.transaction), (ERROR_ORDINAL, 3));
    assert_eq!(reply.status, 9);
}
