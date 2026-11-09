use super::*;
use zoraya_protocol::testing::exercise;
use zoraya_protocol::{Message, decode, encode};

fn total<M>(message: M)
where
    M: for<'b> Message<'b> + PartialEq + core::fmt::Debug,
{
    let mut out = [0; 256];
    let length = encode(&message, 1, &mut out).unwrap();
    let (_, back) = decode::<M>(&out[..length], M::HANDLES).unwrap();
    assert_eq!(back, message);
    exercise(&out[..length], |bytes| {
        let _ = decode::<M>(bytes, M::HANDLES);
    });
}

#[test]
fn every_message_round_trips_and_decodes_totally() {
    let ok = Below(status::OK);
    total(Display {});
    total(DisplayReply {
        status: ok,
        width: 1024,
        height: 768,
    });
    total(Create {
        width: 640,
        height: 480,
    });
    total(Done { status: ok });
    total(Commit {
        x: 0,
        y: 0,
        width: 640,
        height: 480,
    });
    total(FrameDone { status: ok });
    total(Key {
        dropped: 1,
        code: 30,
        pressed: true,
        modifiers: Flags(modifiers::SHIFT),
        text: Below('A' as u32),
    });
    total(Pointer {
        dropped: 0,
        x: 5,
        y: 6,
        buttons: Flags(buttons::LEFT),
    });
    total(Focus {
        dropped: 0,
        focused: true,
    });
    total(DisplayScale {});
    total(DisplayScaleReply {
        status: ok,
        width: 1024,
        height: 768,
        scale: 2,
    });
    total(CreateToplevel {
        width: 640,
        height: 480,
    });
    total(Created {
        status: ok,
        window: 3,
    });
    total(CreatePopup {
        parent: 1,
        x: 10,
        y: 20,
        width: 100,
        height: 50,
    });
    total(PopupCreated {
        status: ok,
        window: 4,
    });
    total(CommitTo {
        window: 1,
        x: 0,
        y: 0,
        width: 10,
        height: 10,
    });
    total(CommitDone { status: ok });
    total(SetBuffer {
        window: 1,
        width: 10,
        height: 10,
    });
    total(BufferDone { status: ok });
    let title = SetTitle {
        window: 1,
        text: zoraya_protocol::Data("zosh — ~/проект".as_bytes()),
    };
    let mut out = [0; 512];
    let length = encode(&title, 1, &mut out).unwrap();
    let (_, back) = decode::<SetTitle<'_>>(&out[..length], 0).unwrap();
    assert_eq!(back, title);
    exercise(&out[..length], |bytes| {
        let _ = decode::<SetTitle<'_>>(bytes, 0);
    });
    let layout = Layout {
        name: zoraya_protocol::Data(b"ua"),
    };
    let length = encode(&layout, 0, &mut out).unwrap();
    assert_eq!(decode::<Layout<'_>>(&out[..length], 0).unwrap().1, layout);
    let long = [b'x'; MAX_TITLE + 1];
    let too_long = SetTitle {
        window: 1,
        text: zoraya_protocol::Data(&long),
    };
    assert!(encode(&too_long, 1, &mut out).is_err());
    total(TitleDone { status: ok });
    total(Close { window: 1 });
    total(CloseDone { status: ok });
    total(Configure {
        window: 1,
        width: 640,
        height: 480,
        scale: 1,
        dark: false,
        motion: false,
    });
    total(CloseRequested { window: 1 });
    total(Minimised {
        window: 1,
        hidden: true,
    });
    total(WindowFocus {
        dropped: 0,
        window: 1,
        focused: true,
    });
    total(WindowPointer {
        dropped: 2,
        window: 1,
        x: 3,
        y: 4,
        buttons: Flags(buttons::RIGHT),
        serial: 5,
    });
    total(AddClient {});
    total(ManagerDone { status: ok });
    total(AttachDisplay {});
    total(AttachInput {});
    total(Stats {});
    total(DisplayHung {});
    total(StatsReply {
        frames: 1,
        commits: 2,
        compose_ns: 3,
        flush_ns: 4,
        flushes: 5,
        hangs: 6,
    });
}

#[test]
fn bad_fields_are_refused() {
    let mut out = [0; 256];
    let key = Key {
        dropped: 0,
        code: 0,
        pressed: false,
        modifiers: Flags(0),
        text: Below(0),
    };
    let length = encode(&key, 0, &mut out).unwrap();
    // modifiers sit after dropped, code and the padded bool.
    out[16 + 12] = 0x10;
    assert!(decode::<Key>(&out[..length], 0).is_err());
    let length = encode(&key, 0, &mut out).unwrap();
    out[16 + 16..16 + 20].copy_from_slice(&TEXT_LIMIT.to_le_bytes());
    assert!(decode::<Key>(&out[..length], 0).is_err());
}

/// INV-0093-2, the wire half: the ordinals of the three directions do not
/// meet, nothing the compositor sends a client carries a handle or a byte
/// array, and no client request is an input or manager ordinal.
#[test]
fn the_client_protocol_has_no_path_for_pixels_or_input() {
    for ordinal in CLIENT_REQUESTS {
        assert!(!CLIENT_EVENTS.contains(&ordinal));
        assert!(!MANAGER_REQUESTS.contains(&ordinal));
        assert_ne!(ordinal, zoraya_protocol::ERROR_ORDINAL);
    }
    for ordinal in CLIENT_EVENTS {
        assert!(!MANAGER_REQUESTS.contains(&ordinal));
    }
    for ordinal in MANAGER_EVENTS {
        assert!(!CLIENT_REQUESTS.contains(&ordinal) && !CLIENT_EVENTS.contains(&ordinal));
        assert!(!MANAGER_REQUESTS.contains(&ordinal));
    }
    for ordinal in CLIENT_MANAGER_REQUESTS {
        assert!(!CLIENT_REQUESTS.contains(&ordinal) && !CLIENT_EVENTS.contains(&ordinal));
        assert!(!MANAGER_REQUESTS.contains(&ordinal) && !MANAGER_EVENTS.contains(&ordinal));
    }
    // Everything a client receives: no handles.
    assert_eq!(<DisplayReply as Message>::HANDLES, 0);
    assert_eq!(<Done as Message>::HANDLES, 0);
    assert_eq!(<FrameDone as Message>::HANDLES, 0);
    assert_eq!(<Key as Message>::HANDLES, 0);
    assert_eq!(<Pointer as Message>::HANDLES, 0);
    assert_eq!(<Focus as Message>::HANDLES, 0);
    // And fixed sizes only: no variable part could carry pixels.
    let mut out = [0; 256];
    let sizes = [
        encode(
            &DisplayReply {
                status: Below(0),
                width: 0,
                height: 0,
            },
            0,
            &mut out,
        )
        .unwrap(),
        encode(&FrameDone { status: Below(0) }, 0, &mut out).unwrap(),
        encode(
            &Key {
                dropped: u32::MAX,
                code: u32::MAX,
                pressed: true,
                modifiers: Flags(modifiers::ALL),
                text: Below(TEXT_LIMIT - 1),
            },
            0,
            &mut out,
        )
        .unwrap(),
        encode(
            &Pointer {
                dropped: 0,
                x: 0,
                y: 0,
                buttons: Flags(0),
            },
            0,
            &mut out,
        )
        .unwrap(),
        encode(
            &Focus {
                dropped: 0,
                focused: false,
            },
            0,
            &mut out,
        )
        .unwrap(),
    ];
    assert!(sizes.iter().all(|s| *s <= 16 + 24));
}

/// The client manager's messages (ADR-0168, ADR-0172) round-trip and
/// decode any bytes without panicking.
#[test]
fn the_client_managers_messages() {
    let mut out = [0; 8192];
    let add = AddRoomClient {
        untrusted: 1,
        colour: 0,
        tag: 5,
        room: zoraya_protocol::Data(b"agent-1"),
        app: zoraya_protocol::Data(b"term"),
        entries: zoraya_protocol::Data("+w ~/sandbox/agent-1\n+r ~/Документи".as_bytes()),
        line: zoraya_protocol::Data(b"room agent-1 term"),
    };
    let length = encode(&add, 3, &mut out).unwrap();
    assert_eq!(
        decode::<AddRoomClient<'_>>(&out[..length], 1).unwrap().1,
        add
    );
    exercise(&out[..length], |bytes| {
        let _ = decode::<AddRoomClient<'_>>(bytes, 1);
    });
    let rooms = SetRooms {
        current: zoraya_protocol::Data(b"project"),
        rooms: zoraya_protocol::Data(b"project --colour 1 +w ~/project\n"),
    };
    let length = encode(&rooms, 4, &mut out).unwrap();
    assert_eq!(decode::<SetRooms<'_>>(&out[..length], 0).unwrap().1, rooms);
    let dock = SetDock {
        apps: zoraya_protocol::Data(b"term\tterm\nedit\tedit FILE\n"),
    };
    let length = encode(&dock, 5, &mut out).unwrap();
    assert_eq!(decode::<SetDock<'_>>(&out[..length], 0).unwrap().1, dock);
    let long = [b'x'; MAX_ROOMS_TEXT + 1];
    let too_long = SetRooms {
        current: zoraya_protocol::Data(b""),
        rooms: zoraya_protocol::Data(&long),
    };
    assert!(encode(&too_long, 1, &mut out).is_err());
    total(DockPressed {
        kind: dock::ROOM,
        index: 2,
    });
    total(AttachClientManager {});
}

/// The sheet, the grant pill and the system mark (proposed ADR-0245):
/// round trips, total decoding, and the kinds' range.
#[test]
fn the_system_clients_messages() {
    let mut out = [0; 8192];
    let sheet = CreateSheet {
        tag: 4,
        kind: Below(2),
        width: 520,
        height: 420,
    };
    let n = encode(&sheet, 3, &mut out).unwrap();
    assert_eq!(decode::<CreateSheet>(&out[..n], 1).unwrap().1, sheet);
    zoraya_protocol::testing::exercise(&out[..n], |b| {
        let _ = decode::<CreateSheet>(b, 1);
    });
    assert!(
        encode(
            &CreateSheet {
                kind: Below(3),
                ..sheet
            },
            3,
            &mut out
        )
        .is_err()
    );
    let g = Granted {
        tag: 4,
        words: zoraya_protocol::Data("+r ~/project/plán.txt".as_bytes()),
    };
    let n = encode(&g, 3, &mut out).unwrap();
    assert_eq!(decode::<Granted<'_>>(&out[..n], 0).unwrap().1, g);
    zoraya_protocol::testing::exercise(&out[..n], |b| {
        let _ = decode::<Granted<'_>>(b, 0);
    });
    let n = encode(&AddSystemClient {}, 3, &mut out).unwrap();
    assert!(decode::<AddSystemClient>(&out[..n], 1).is_ok());
    assert!(decode::<AddSystemClient>(&out[..n], 0).is_err());
    for o in SYSTEM_REQUESTS {
        assert!(CLIENT_REQUESTS.contains(&o));
    }
    assert!(MANAGER_REQUESTS.contains(&42));
}
