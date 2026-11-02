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
