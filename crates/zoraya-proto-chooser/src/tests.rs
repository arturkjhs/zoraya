use super::*;
use std::vec::Vec;
use zoraya_protocol::testing::exercise;
use zoraya_protocol::{Message, decode, encode};

/// Encodes `$m`, decodes it back with `$handles` handles, and decodes
/// every prefix and corruption of it without a panic.
macro_rules! round {
    ($t:ty, $m:expr, $handles:expr) => {{
        let m = $m;
        let mut out = [0u8; 8192];
        let n = encode(&m, 7, &mut out).unwrap();
        let (h, back) = decode::<$t>(&out[..n], $handles).unwrap();
        assert_eq!(h.transaction, 7);
        assert_eq!(back, m);
        exercise(&out[..n], |b| {
            let _ = decode::<$t>(b, $handles);
        });
    }};
}

#[test]
fn every_message_round_trips_and_decodes_totally() {
    round!(
        Open<'_>,
        Open {
            title: Data("Відкрити звіт".as_bytes()),
            filters: Data(b"txt md"),
        },
        0
    );
    round!(
        Save<'_>,
        Save {
            title: Data(b""),
            suggested: Data("plán.txt".as_bytes()),
            filters: Data(b""),
        },
        0
    );
    round!(
        PickDirectory<'_>,
        PickDirectory {
            title: Data(b"Export"),
            write: Below(1),
        },
        0
    );
    round!(
        Chosen<'_>,
        Chosen {
            name: Data("plán.txt".as_bytes()),
            kind: Below(kind::READ),
            words: Data("+r ~/project/plán.txt edit".as_bytes()),
        },
        1
    );
    round!(
        Cancelled,
        Cancelled {
            status: Below(status::BUSY),
        },
        0
    );
    round!(
        AddApplication<'_>,
        AddApplication {
            tag: 3,
            name: Data(b"edit"),
        },
        1
    );
    // ADR-0052: a changed message is a new ordinal, so the older
    // `OpenWith` keeps its layout beside `OpenWithApp`.
    round!(
        OpenWith<'_>,
        OpenWith {
            name: Data(b"notes.txt"),
            words: Data(b"+r ~/notes.txt"),
        },
        1
    );
    round!(
        OpenWithApp<'_>,
        OpenWithApp {
            name: Data(b"notes.txt"),
            words: Data(b"+r ~/notes.txt"),
            app: Data(b"edit"),
        },
        1
    );
    round!(
        Opening,
        Opening {
            status: Below(status::BUSY)
        },
        0
    );
    round!(
        OpenWindow<'_>,
        OpenWindow {
            app: Data(b"netsurf"),
            title: Data(b"NetSurf"),
            entries: Data(b"+net example.org:443\n+r /share/netsurf"),
        },
        0
    );
    round!(WindowOpened, WindowOpened {}, 1);
    round!(
        WindowRefused,
        WindowRefused {
            status: Below(window_refusal::BUSY)
        },
        0
    );
    round!(
        OpeningApp,
        OpeningApp {
            status: Below(status::BUSY)
        },
        0
    );
}

#[test]
fn the_launchers_ordinals_are_distinct_and_the_old_layouts_stay() {
    let mut all = LAUNCHER_REQUESTS.to_vec();
    all.sort_unstable();
    all.dedup();
    assert_eq!(all.len(), LAUNCHER_REQUESTS.len());
    assert_eq!((OpenWith::ORDINAL, OpenWithApp::ORDINAL), (32, 35));
    assert_eq!((Opening::ORDINAL, OpeningApp::ORDINAL), (32, 35));
    // The old message with the new one's body is refused: nothing was
    // widened under the old number.
    let new = OpenWithApp {
        name: Data(b"a"),
        words: Data(b"b"),
        app: Data(b"edit"),
    };
    let mut out = [0u8; 128];
    let n = encode(&new, 0, &mut out).unwrap();
    assert!(decode::<OpenWith<'_>>(&out[..n], 1).is_err());
}

#[test]
fn limits_and_ranges_are_refused() {
    let mut out = [0u8; 8192];
    let long = [b'x'; MAX_TITLE + 1];
    let m = Open {
        title: Data(&long),
        filters: Data(b""),
    };
    assert!(encode(&m, 0, &mut out).is_err());
    let m = Chosen {
        name: Data(b"a"),
        kind: Below(kind::COUNT),
        words: Data(b""),
    };
    assert!(encode(&m, 0, &mut out).is_err());
    // A Chosen without its connection does not decode.
    let m = Chosen {
        name: Data(b"a"),
        kind: Below(kind::READ),
        words: Data(b""),
    };
    let n = encode(&m, 0, &mut out).unwrap();
    assert!(decode::<Chosen<'_>>(&out[..n], 0).is_err());
}

/// INV-0167-1: no message of the chooser's or the GUI's protocol carries
/// a path: neither declares a field of the protocol crate's `Path` type,
/// and every ordinal is listed once.
#[test]
fn no_message_carries_a_path() {
    for source in [
        include_str!("lib.rs"),
        include_str!("../../zoraya-proto-gui/src/lib.rs"),
    ] {
        let body = &source[source.find("protocol! {").expect("a protocol")..];
        assert!(!body.contains("Path<"), "a Path field");
        assert!(!body.contains(": Path"), "a Path field");
    }
    let mut all: Vec<u32> = APPLICATION_REQUESTS
        .iter()
        .chain(ANSWERS.iter())
        .chain(PANEL_REQUESTS.iter())
        .copied()
        .collect();
    all.sort_unstable();
    all.dedup();
    assert_eq!(all.len(), 6);
    assert_eq!(Open::ORDINAL, 1);
    assert_eq!(Chosen::ORDINAL, 8);
    assert_eq!(AddApplication::ORDINAL, 16);
}

/// Review W1: an answer of an earlier transaction is stale, one of the
/// request's own is sorted by what it is.
#[test]
fn a_window_reply_is_sorted_by_its_transaction() {
    let mut out = [0u8; 64];
    let n = encode(&WindowOpened {}, 1, &mut out).unwrap();
    assert_eq!(window_reply(&out[..n], 1, 1), WindowReply::Opened);
    assert_eq!(window_reply(&out[..n], 1, 2), WindowReply::Stale);
    assert_eq!(window_reply(&out[..n], 0, 1), WindowReply::Garbled);
    let r = WindowRefused {
        status: zoraya_protocol::Below(window_refusal::BUSY),
    };
    let n = encode(&r, 5, &mut out).unwrap();
    assert_eq!(
        window_reply(&out[..n], 0, 5),
        WindowReply::Refused(window_refusal::BUSY)
    );
    assert_eq!(window_reply(&out[..n], 0, 4), WindowReply::Stale);
    assert_eq!(window_reply(&out[..2], 0, 1), WindowReply::Garbled);
}
