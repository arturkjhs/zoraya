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

fn with_body(body: &[u8], out: &mut [u8]) -> usize {
    out[0..4].copy_from_slice(&MAGIC_VERSION.to_le_bytes());
    out[4..8].copy_from_slice(&Limits::ORDINAL_FOR_TEST.to_le_bytes());
    out[8..12].fill(0);
    out[12..16].copy_from_slice(&(body.len() as u32).to_le_bytes());
    out[16..16 + body.len()].copy_from_slice(body);
    16 + body.len()
}

protocol! {
    message Limits<'a> = 9, handles 0 {
        data: Data<'a, 2>,
    }
    message TextLimit<'a> = 9, handles 0 {
        text: Text<'a, 2>,
    }
    message Records<'a> = 9, handles 0 {
        list: Seq<'a, Entry<'a>, 1>,
    }
    message Ranged = 9, handles 0 {
        kind: Below<3>,
    }
    message Paths<'a> = 9, handles 0 {
        path: Path<'a, 8, false>,
        rest: Path<'a, 8, true>,
    }
}

impl Limits<'_> {
    const ORDINAL_FOR_TEST: u32 = 9;
}

/// INV-0052-1: a decoder refuses a length over the declared maximum, for each
/// kind, even with the bytes present.
#[test]
fn a_decoder_enforces_declared_maxima() {
    let mut out = [0; 64];
    let body = [3, 0, 0, 0, b'a', b'b', b'c', 0];
    let length = with_body(&body[..7], &mut out);
    assert_eq!(
        decode::<Limits<'_>>(&out[..length], 0).map(|_| ()),
        Err(WireError::TooLong)
    );
    assert_eq!(
        decode::<TextLimit<'_>>(&out[..length], 0).map(|_| ()),
        Err(WireError::TooLong)
    );
    let body = [2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
    let length = with_body(&body, &mut out);
    assert_eq!(
        decode::<Records<'_>>(&out[..length], 0).map(|_| ()),
        Err(WireError::TooLong)
    );
}

fn seq_message(out: &mut [u8]) -> usize {
    encode(
        &Records {
            list: Seq::Of(&ENTRIES[..1]),
        },
        0,
        out,
    )
    .unwrap()
}

/// INV-0052-2 inside a sequence: a padding byte between a record's parts, and
/// a byte past the last record but inside the declared size.
#[test]
fn a_sequence_has_no_loose_bytes() {
    let mut out = [0; 128];
    let length = seq_message(&mut out);
    let mut grown = out;
    // The size field, at body offset 8, covers one byte more.
    grown[24] += 1;
    grown[12] += 1;
    assert_eq!(
        decode::<Records<'_>>(&grown[..length + 1], 0).map(|_| ()),
        Err(WireError::Trailing)
    );
}

#[test]
fn padding_between_records_is_refused() {
    let many = [ENTRIES[0], ENTRIES[1]];
    let mut out = [0; 128];
    let length = encode(
        &Everything {
            list: Seq::Of(&many),
            ..sample(&ENTRIES)
        },
        0,
        &mut out,
    )
    .unwrap();
    // The first record ends in `a`, and padding runs to 8-byte alignment.
    let at = out[..length].iter().rposition(|b| *b == b'a').unwrap() + 1;
    assert!(!(at - 16).is_multiple_of(8), "there is padding to spoil");
    assert_eq!(out[at], 0);
    let mut copy = out;
    copy[at] = 1;
    assert_eq!(
        decode::<Everything<'_>>(&copy[..length], 1).map(|_| ()),
        Err(WireError::BadPadding)
    );
}

#[test]
fn an_enumeration_is_range_checked() {
    let mut out = [0; 32];
    let length = encode(&Ranged { kind: Below(2) }, 0, &mut out).unwrap();
    assert!(decode::<Ranged>(&out[..length], 0).is_ok());
    out[16] = 3;
    assert_eq!(
        decode::<Ranged>(&out[..length], 0).map(|_| ()),
        Err(WireError::BadValue)
    );
    assert_eq!(
        encode(&Ranged { kind: Below(3) }, 0, &mut out),
        Err(WireError::BadValue)
    );
}

#[test]
fn a_path_is_relative() {
    let mut out = [0; 64];
    let good = Paths {
        path: Path("a/b"),
        rest: Path(""),
    };
    let length = encode(&good, 0, &mut out).unwrap();
    assert_eq!(decode::<Paths<'_>>(&out[..length], 0).unwrap().1, good);
    let bad = Paths {
        path: Path("/etc"),
        rest: Path(""),
    };
    assert_eq!(encode(&bad, 0, &mut out), Err(WireError::BadValue));
    let empty = Paths {
        path: Path(""),
        rest: Path(""),
    };
    assert_eq!(encode(&empty, 0, &mut out), Err(WireError::BadValue));
    // On the wire too: `a/b` becomes `/a/`.
    out[20] = b'/';
    out[21] = b'a';
    out[22] = b'/';
    assert_eq!(
        decode::<Paths<'_>>(&out[..length], 0).map(|_| ()),
        Err(WireError::BadValue)
    );
}

#[test]
fn a_path_has_no_dots_or_empty_parts() {
    type P<'a> = Path<'a, 64, false>;
    for bad in ["a/../b", "..", ".", "a//b", "a/", "a\u{1}b", "/a", ""] {
        assert!(!P::is_valid(bad), "{bad:?}");
    }
    assert!(P::is_valid("a/b.c/d"));
    assert!(Path::<'_, 64, true>::is_valid(""));
}

/// INV-0052-3: `protocol!` runs this at compile time on every field name.
#[test]
#[should_panic(expected = "an operation field")]
fn an_operation_field_is_refused() {
    no_operation_field(&["offset", "operation"]);
}

#[test]
fn ordinary_fields_pass() {
    no_operation_field(&["offset", "length", "name", "kind"]);
}

#[test]
fn the_error_reply_decodes_totally() {
    let mut out = [0; 32];
    let length = encode(&ErrorReply { status: 1 }, 0, &mut out).unwrap();
    testing::exercise(&out[..length], |bytes| {
        let _ = decode::<ErrorReply>(bytes, 0);
    });
}
