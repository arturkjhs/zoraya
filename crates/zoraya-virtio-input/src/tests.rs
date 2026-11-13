use std::cell::RefCell;
use std::rc::Rc;

use zoraya_virtio::fake::{Device, Event, Fake};
use zoraya_virtio::{DEVICE_INPUT, Shared};

use super::*;

const BASE: u64 = 0x4000_0000;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Profile {
    Keyboard,
    Tablet { min: u32, max: u32 },
    Nothing,
}

/// The configuration a device of `profile` shows for the current select.
fn configure(f: &mut Fake, profile: Profile) {
    let (which, sub) = (f.config[0], f.config[1]);
    let mut data = [0u8; 128];
    let size = match (which, sub, profile) {
        (0x11, 3, Profile::Tablet { .. }) => {
            data[0] = 0b11;
            1
        }
        (0x11, 1, Profile::Keyboard) => {
            data[0] = 0xfe;
            96
        }
        (0x11, 1, Profile::Tablet { .. }) => {
            data[0x110 / 8] = 0b111;
            40
        }
        (0x12, 0 | 1, Profile::Tablet { min, max }) => {
            data[0..4].copy_from_slice(&min.to_le_bytes());
            data[4..8].copy_from_slice(&max.to_le_bytes());
            20
        }
        _ => 0,
    };
    f.config[2] = size;
    f.config[8..136].copy_from_slice(&data);
}

fn device(profile: Profile) -> Device {
    let d = Device::new(DEVICE_INPUT, BASE, SHARED_MEMORY);
    d.on(move |f, e| {
        if let Event::Config(0 | 1) = e {
            configure(f, profile);
        }
    });
    d
}

type FakeInput = Input<zoraya_virtio::fake::FakeRegisters, zoraya_virtio::fake::FakeShared>;

fn up(d: &Device) -> FakeInput {
    Input::new(d.registers(), d.shared(), BASE).unwrap()
}

/// The device writes one event into the next buffer, truthfully.
fn push(d: &Device, kind: u16, code: u16, value: u32) {
    push_with(d, kind, code, value, 8);
}

fn push_with(d: &Device, kind: u16, code: u16, value: u32, length: u32) {
    let mut f = d.state.borrow_mut();
    let head = f.next_available(0).expect("a buffer is offered");
    let buffer = f.chain(0, head)[0];
    assert!(buffer.writable && buffer.length == 8);
    let mut bytes = [0u8; 8];
    bytes[0..2].copy_from_slice(&kind.to_le_bytes());
    bytes[2..4].copy_from_slice(&code.to_le_bytes());
    bytes[4..8].copy_from_slice(&value.to_le_bytes());
    assert!(f.write(buffer.address, &bytes));
    f.complete(0, u32::from(head), length);
}

fn syn(d: &Device) {
    push(d, ev::SYN, ev::SYN_REPORT, 0);
}

type Frames = Rc<RefCell<Vec<(Vec<Record>, u32)>>>;

/// Polls, with a client that takes frames while `room` says so.
fn poll(input: &mut FakeInput, frames: &Frames, room: bool) -> Result<(), Error> {
    let log = frames.clone();
    input.poll(&mut |records, dropped| {
        if room {
            log.borrow_mut().push((records.to_vec(), dropped));
        }
        room
    })
}

fn key(code: u32, value: u32) -> Record {
    Record {
        kind: KEY,
        code,
        value,
    }
}

#[test]
fn a_keyboard_is_found_and_every_buffer_offered() {
    let d = device(Profile::Keyboard);
    let input = up(&d);
    assert_eq!(input.kind(), Kind::Keyboard);
    let mut f = d.state.borrow_mut();
    let mut heads = Vec::new();
    while let Some(head) = f.next_available(0) {
        heads.push(head);
    }
    assert_eq!(heads, (0..BUFFERS).collect::<Vec<_>>());
    assert_eq!(f.driver_features, [0, 1]);
    assert_eq!(f.status & 4, 4);
}

#[test]
fn a_tablet_is_found_with_its_axes() {
    let d = device(Profile::Tablet {
        min: 0,
        max: 0x7fff,
    });
    let input = up(&d);
    let axis = Axis {
        min: 0,
        max: 0x7fff,
    };
    assert_eq!(input.kind(), Kind::Pointer { x: axis, y: axis });
}

#[test]
fn a_device_that_is_neither_or_has_a_bad_axis_is_refused_and_reset() {
    for profile in [
        Profile::Nothing,
        Profile::Tablet { min: 5, max: 5 },
        Profile::Tablet { min: 9, max: 3 },
    ] {
        let d = device(profile);
        let refused = Input::new(d.registers(), d.shared(), BASE).err();
        assert_eq!(refused, Some(Error::Unusable));
        assert_eq!(d.state.borrow().status, 0, "reset");
    }
}

#[test]
fn keys_arrive_in_frames() {
    let d = device(Profile::Keyboard);
    let mut input = up(&d);
    let frames = Frames::default();
    push(&d, ev::KEY, 30, 1);
    poll(&mut input, &frames, true).unwrap();
    assert!(frames.borrow().is_empty(), "no frame before SYN_REPORT");
    syn(&d);
    push(&d, ev::KEY, 30, 0);
    push(&d, ev::KEY, 42, 2);
    syn(&d);
    // An empty frame sends nothing.
    syn(&d);
    poll(&mut input, &frames, true).unwrap();
    assert_eq!(
        *frames.borrow(),
        [(vec![key(30, 1)], 0), (vec![key(30, 0), key(42, 2)], 0)]
    );
}

/// INV-0092-1: only the listed kinds, codes and ranges get through.
#[test]
fn strange_events_are_dropped_and_counted() {
    let d = device(Profile::Tablet {
        min: 100,
        max: 1100,
    });
    let mut input = up(&d);
    let frames = Frames::default();
    push(&d, ev::KEY, ev::KEY_LIMIT, 1); // code too high
    push(&d, ev::KEY, 0x110, 3); // value not 0..=2
    push(&d, ev::ABS, 2, 500); // an axis that is not x or y
    push(&d, ev::ABS, ev::ABS_X, 99); // below the range
    push(&d, ev::ABS, ev::ABS_Y, 1101); // above the range
    push(&d, 2, 0, 1); // EV_REL
    push(&d, ev::SYN, 3, 0); // SYN_DROPPED
    push(&d, 0x1f, 0, 0);
    push(&d, ev::ABS, ev::ABS_X, 100);
    push(&d, ev::ABS, ev::ABS_Y, 1100);
    push(&d, ev::KEY, 0x110, 1);
    syn(&d);
    poll(&mut input, &frames, true).unwrap();
    let expected = vec![
        Record {
            kind: AXIS,
            code: 0,
            value: 0,
        },
        Record {
            kind: AXIS,
            code: 1,
            value: AXIS_MAX,
        },
        key(0x110, 1),
    ];
    assert_eq!(*frames.borrow(), [(expected, 8)]);
    for (kind, code, value) in [(ev::ABS, ev::ABS_X, 100u32), (ev::ABS, ev::ABS_X, 600)] {
        push(&d, kind, code, value);
    }
    syn(&d);
    frames.borrow_mut().clear();
    poll(&mut input, &frames, true).unwrap();
    let values: Vec<u32> = frames.borrow()[0].0.iter().map(|r| r.value).collect();
    assert_eq!(values, [0, AXIS_MAX / 2]);
}

#[test]
fn axis_values_never_leave_the_scaled_range() {
    let axes = [
        Axis { min: 0, max: 1 },
        Axis {
            min: 0,
            max: u32::MAX,
        },
        Axis {
            min: u32::MAX - 1,
            max: u32::MAX,
        },
        Axis {
            min: 7,
            max: 0x7fff,
        },
    ];
    for axis in axes {
        for value in [0, 1, 7, 0x4000, 0x7fff, u32::MAX - 1, u32::MAX] {
            match axis.scale(value) {
                Some(v) => {
                    assert!(v <= AXIS_MAX);
                    assert!(value >= axis.min && value <= axis.max);
                }
                None => assert!(value < axis.min || value > axis.max),
            }
        }
        assert_eq!(axis.scale(axis.min), Some(0));
        assert_eq!(axis.scale(axis.max), Some(AXIS_MAX));
    }
}

/// INV-0092-2: a client with no room never stops the driver, and the count
/// arrives with the next frame that fits.
#[test]
fn a_full_client_loses_frames_counted_and_the_device_keeps_its_buffers() {
    let d = device(Profile::Keyboard);
    let mut input = up(&d);
    let frames = Frames::default();
    // Far more events than buffers, with no room at the client.
    for round in 0..200u32 {
        push(&d, ev::KEY, 30, round % 2);
        push(&d, ev::KEY, 31, round % 2);
        syn(&d);
        poll(&mut input, &frames, false).unwrap();
    }
    assert!(frames.borrow().is_empty());
    assert_eq!(input.dropped(), 400);
    push(&d, ev::KEY, 32, 1);
    syn(&d);
    poll(&mut input, &frames, true).unwrap();
    assert_eq!(*frames.borrow(), [(vec![key(32, 1)], 400)]);
    assert_eq!(input.dropped(), 0);
}

#[test]
fn a_frame_longer_than_the_limit_keeps_the_first_and_counts_the_rest() {
    let d = device(Profile::Keyboard);
    let mut input = up(&d);
    let frames = Frames::default();
    for i in 0..MAX_FRAME + 5 {
        push(&d, ev::KEY, i as u16, 1);
        if i % 16 == 15 {
            poll(&mut input, &frames, true).unwrap();
        }
    }
    syn(&d);
    poll(&mut input, &frames, true).unwrap();
    let got = frames.borrow();
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].0.len(), MAX_FRAME);
    assert_eq!(got[0].1, 5);
}

#[test]
fn a_broken_used_ring_is_a_protocol_break() {
    // A short write.
    let d = device(Profile::Keyboard);
    let mut input = up(&d);
    push_with(&d, ev::KEY, 30, 1, 4);
    assert_eq!(
        poll(&mut input, &Frames::default(), true),
        Err(Error::Device)
    );
    // A head past the queue. (Every head in the queue is offered again as
    // soon as it is read, so naming one twice is harmless and not caught.)
    let d = device(Profile::Keyboard);
    let mut input = up(&d);
    d.state.borrow_mut().complete(0, u32::from(BUFFERS) + 3, 8);
    assert_eq!(
        poll(&mut input, &Frames::default(), true),
        Err(Error::Device)
    );
    // A used index that jumps.
    let d = device(Profile::Keyboard);
    let mut input = up(&d);
    d.state.borrow_mut().set_used_index(0, BUFFERS + 1);
    assert_eq!(
        poll(&mut input, &Frames::default(), true),
        Err(Error::Device)
    );
}

#[test]
fn an_event_is_read_once_before_its_buffer_is_offered_again() {
    // The device rewrites a buffer as soon as it is offered again: what the
    // driver kept is what was there when it read it.
    let d = device(Profile::Keyboard);
    let mut input = up(&d);
    push(&d, ev::KEY, 30, 1);
    let mut shared = d.shared();
    let frames = Frames::default();
    poll(&mut input, &frames, true).unwrap();
    shared.write16(EVENTS_AT + 2, 99);
    syn(&d);
    poll(&mut input, &frames, true).unwrap();
    assert_eq!(frames.borrow()[0].0, [key(30, 1)]);
}

#[test]
fn reset_and_acknowledge_reach_the_registers() {
    let d = device(Profile::Keyboard);
    let mut input = up(&d);
    d.state.borrow_mut().interrupt_status = 1;
    input.acknowledge();
    assert_eq!(d.state.borrow().interrupt_status, 0);
    assert!(input.reset());
    d.state.borrow_mut().no_reset = true;
    assert!(!input.reset());
}

#[test]
fn the_shared_layout_fits() {
    const { assert!(EVENTS_AT >= QUEUE.end) };
    const { assert!(SHARED_MEMORY >= EVENTS_AT + 8 * BUFFERS as usize) };
    assert_eq!(SHARED_MEMORY % 4096, 0);
}

/// Holes cargo-mutants found (TASK-14 review): devices the profiles above do
/// not cover, built from their configuration directly.
struct Config {
    abs_bits: u8,
    x: (u32, u32),
    y: (u32, u32),
    info_size: u8,
}

fn custom(c: Config) -> Device {
    let d = Device::new(DEVICE_INPUT, BASE, SHARED_MEMORY);
    d.on(move |f, e| {
        if let Event::Config(0 | 1) = e {
            let (which, sub) = (f.config[0], f.config[1]);
            let mut data = [0u8; 128];
            let size = match (which, sub) {
                (0x11, 3) => {
                    data[0] = c.abs_bits;
                    1
                }
                (0x11, 1) => {
                    data[0] = 0xfe;
                    96
                }
                (0x12, 0 | 1) => {
                    let (min, max) = if sub == 0 { c.x } else { c.y };
                    data[0..4].copy_from_slice(&min.to_le_bytes());
                    data[4..8].copy_from_slice(&max.to_le_bytes());
                    c.info_size
                }
                _ => 0,
            };
            f.config[2] = size;
            f.config[8..136].copy_from_slice(&data);
        }
    });
    d
}

#[test]
fn a_device_with_one_absolute_axis_is_not_a_pointer() {
    for abs_bits in [0, 0b01, 0b10] {
        let d = custom(Config {
            abs_bits,
            x: (0, 100),
            y: (0, 100),
            info_size: 20,
        });
        assert_eq!(up(&d).kind(), Kind::Keyboard, "{abs_bits:#b}");
    }
}

#[test]
fn axis_info_of_exactly_eight_bytes_is_enough() {
    let d = custom(Config {
        abs_bits: 0b11,
        x: (0, 100),
        y: (0, 100),
        info_size: 8,
    });
    let axis = Axis { min: 0, max: 100 };
    assert_eq!(up(&d).kind(), Kind::Pointer { x: axis, y: axis });
}

#[test]
fn each_axis_uses_its_own_range() {
    let d = custom(Config {
        abs_bits: 0b11,
        x: (0, 1000),
        y: (0, 2000),
        info_size: 20,
    });
    let mut input = up(&d);
    let frames = Frames::default();
    push(&d, ev::ABS, ev::ABS_X, 500);
    push(&d, ev::ABS, ev::ABS_Y, 500);
    syn(&d);
    poll(&mut input, &frames, true).unwrap();
    let values: Vec<u32> = frames.borrow()[0].0.iter().map(|r| r.value).collect();
    assert_eq!(values, [AXIS_MAX / 2, AXIS_MAX / 4]);
}

#[test]
fn the_shared_memory_is_four_pages() {
    assert_eq!(SHARED_MEMORY, 16 * 1024);
}

/// A tablet with wheels, as QEMU's `virtio-tablet` (`REL_WHEEL` and
/// `REL_HWHEEL`), or with their high-resolution axes too, as a Linux
/// mouse that sends both.
fn wheel_device(hi_res: bool) -> Device {
    let d = Device::new(DEVICE_INPUT, BASE, SHARED_MEMORY);
    d.on(move |f, e| {
        if let Event::Config(0 | 1) = e {
            let (which, sub) = (f.config[0], f.config[1]);
            let mut data = [0u8; 128];
            let size = match (which, sub) {
                (0x11, 3) => {
                    data[0] = 0b11;
                    1
                }
                (0x11, 2) => {
                    data[0] = 1 << ev::REL_HWHEEL;
                    data[1] = 1 << (ev::REL_WHEEL - 8);
                    if hi_res {
                        data[1] |=
                            (1 << (ev::REL_WHEEL_HI_RES - 8)) | (1 << (ev::REL_HWHEEL_HI_RES - 8));
                    }
                    2
                }
                (0x12, 0 | 1) => {
                    data[4..8].copy_from_slice(&1000u32.to_le_bytes());
                    20
                }
                _ => 0,
            };
            f.config[2] = size;
            f.config[8..136].copy_from_slice(&data);
        }
    });
    d
}

fn wheel(code: u32, value: i32) -> Record {
    Record {
        kind: WHEEL,
        code,
        value: value as u32,
    }
}

/// Proposed ADR-0322: a notch is 120, toward the person (evdev's -1) is
/// above 0, the horizontal wheel keeps evdev's sign, and a value past the
/// bound, 0 or another relative axis is dropped and counted.
#[test]
fn wheel_notches_become_signed_120ths() {
    let d = wheel_device(false);
    let mut input = up(&d);
    let frames = Frames::default();
    let neg = |v: i32| v as u32;
    push(&d, ev::REL, ev::REL_WHEEL, neg(-1)); // toward the person
    push(&d, ev::REL, ev::REL_WHEEL, 2); // away, two notches
    push(&d, ev::REL, ev::REL_HWHEEL, 1); // right
    push(&d, ev::REL, ev::REL_WHEEL, 17); // past 16 notches
    push(&d, ev::REL, ev::REL_WHEEL, 0);
    push(&d, ev::REL, ev::REL_WHEEL, neg(i32::MIN)); // overflows when scaled
    push(&d, ev::REL, 0, 5); // REL_X
    push(&d, ev::REL, ev::REL_WHEEL_HI_RES, 5000); // past the bound
    syn(&d);
    poll(&mut input, &frames, true).unwrap();
    let expected = vec![wheel(0, 120), wheel(0, -240), wheel(1, 120)];
    assert_eq!(*frames.borrow(), [(expected, 5)]);
    // Every record the driver keeps is one the compositor takes.
    for r in &frames.borrow()[0].0 {
        let e = zoraya_proto_input::Event {
            kind: zoraya_protocol::Below(r.kind),
            code: r.code,
            value: r.value,
        };
        assert!(e.is_valid(), "{r:?}");
    }
}

/// A device with the high-resolution wheels: their 120ths are taken, and
/// the notches it also sends for the same turn are skipped, not counted.
#[test]
fn hi_res_wheels_are_taken_once() {
    let d = wheel_device(true);
    let mut input = up(&d);
    let frames = Frames::default();
    push(&d, ev::REL, ev::REL_WHEEL_HI_RES, 60);
    push(&d, ev::REL, ev::REL_WHEEL, 1);
    push(&d, ev::REL, ev::REL_HWHEEL_HI_RES, (-30i32) as u32);
    push(&d, ev::REL, ev::REL_HWHEEL, (-1i32) as u32);
    syn(&d);
    poll(&mut input, &frames, true).unwrap();
    assert_eq!(*frames.borrow(), [(vec![wheel(0, -60), wheel(1, -30)], 0)]);
}

/// A keyboard's relative events are not taken: wheels are a pointer's.
#[test]
fn a_keyboard_has_no_wheel() {
    let d = device(Profile::Keyboard);
    let mut input = up(&d);
    let frames = Frames::default();
    push(&d, ev::REL, ev::REL_WHEEL, 1);
    push(&d, ev::KEY, 30, 1);
    syn(&d);
    poll(&mut input, &frames, true).unwrap();
    assert_eq!(*frames.borrow(), [(vec![key(30, 1)], 1)]);
}
