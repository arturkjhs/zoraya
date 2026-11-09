use std::cell::{Cell, RefCell};
use std::rc::Rc;

use zoraya_virtio::fake::{Device, Event, Fake};
use zoraya_virtio::{DEVICE_GPU, reg};

use super::*;

const BASE: u64 = 0x4000_0000;
const FRAME: u64 = 0x8000_0000;

/// How the fake device misbehaves.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Hostile {
    #[default]
    None,
    /// Answers with a type that is neither the one asked for nor an error.
    WrongType,
    /// Claims a shorter response than the type needs.
    Short,
    /// Claims more bytes than the response buffer holds.
    Long,
    /// Returns a head the driver never offered.
    WrongHead,
    /// Moves the used index past what was submitted.
    Jumps,
    /// Answers every command with an error.
    Errors,
    /// Scanout 0 is not enabled.
    Disabled,
    /// Scanout 0 is larger than any accepted display.
    Huge,
    /// Never answers.
    Silent,
    /// An error type in a response shorter than a header.
    ShortError,
}

/// A command as the device saw it: its type and the words after the header.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Seen {
    kind: u32,
    words: Vec<u32>,
    /// Bytes the driver offered for the response.
    room: u32,
}

struct Gpu0 {
    seen: Rc<RefCell<Vec<Seen>>>,
    device: Device,
    /// The size `GET_DISPLAY_INFO` reports.
    size: Rc<Cell<(u32, u32)>>,
}

fn word(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(b[at..at + 4].try_into().unwrap())
}

fn serve(f: &mut Fake, hostile: Hostile, seen: &RefCell<Vec<Seen>>, size: (u32, u32)) {
    while let Some(head) = f.next_available(0) {
        let chain = f.chain(0, head);
        assert_eq!(chain.len(), 2, "a request and a response");
        assert!(!chain[0].writable && chain[1].writable);
        let request = f
            .read(chain[0].address, chain[0].length as usize)
            .expect("request in shared memory");
        let kind = word(&request, 0);
        let words = request[24..]
            .chunks(4)
            .map(|c| u32::from_le_bytes(c.try_into().unwrap()))
            .collect();
        seen.borrow_mut().push(Seen {
            kind,
            words,
            room: chain[1].length,
        });
        if hostile == Hostile::Silent {
            continue;
        }
        let mut response = vec![0u8; chain[1].length as usize];
        let ok = if kind == cmd::GET_DISPLAY_INFO {
            let (w, h, on) = match hostile {
                Hostile::Disabled => (1024, 768, 0),
                Hostile::Huge => (MAX_SIDE + 1, 768, 1),
                _ => (size.0, size.1, 1u32),
            };
            response[24 + 8..24 + 12].copy_from_slice(&w.to_le_bytes());
            response[24 + 12..24 + 16].copy_from_slice(&h.to_le_bytes());
            response[24 + 16..24 + 20].copy_from_slice(&on.to_le_bytes());
            cmd::OK_DISPLAY_INFO
        } else {
            cmd::OK_NODATA
        };
        let (answer, length) = match hostile {
            Hostile::WrongType => (0x1300, response.len() as u32),
            Hostile::Errors => (cmd::ERR_UNSPEC + 5, 24),
            Hostile::ShortError => (cmd::ERR_UNSPEC + 5, 16),
            Hostile::Short => (ok, 16),
            Hostile::Long => (ok, chain[1].length + 1),
            _ => (ok, response.len() as u32),
        };
        response[0..4].copy_from_slice(&answer.to_le_bytes());
        f.write(chain[1].address, &response);
        match hostile {
            Hostile::WrongHead => f.complete(0, u32::from(head) + 1, length),
            Hostile::Jumps => {
                f.complete(0, u32::from(head), length);
                let index = f.queues[0].used_index.wrapping_add(5);
                f.set_used_index(0, index);
            }
            _ => f.complete(0, u32::from(head), length),
        }
    }
}

fn device(hostile: Hostile) -> Gpu0 {
    let device = Device::new(DEVICE_GPU, BASE, SHARED_MEMORY);
    let seen = Rc::new(RefCell::new(Vec::new()));
    let size = Rc::new(Cell::new((1024, 768)));
    let (log, shown) = (seen.clone(), size.clone());
    device.on(move |f, e| match e {
        Event::Notify(0) => serve(f, hostile, &log, shown.get()),
        // `events_clear`: the bits written there leave `events_read`.
        Event::Config(4) => {
            let read = word(&f.config, 0);
            let clear = word(&f.config, 4);
            f.config[0..4].copy_from_slice(&(read & !clear).to_le_bytes());
        }
        _ => {}
    });
    Gpu0 { seen, device, size }
}

type FakeGpu = Gpu<zoraya_virtio::fake::FakeRegisters, zoraya_virtio::fake::FakeShared>;

fn up(d: &Gpu0) -> FakeGpu {
    Gpu::new(d.device.registers(), d.device.shared(), BASE).unwrap()
}

/// The device answers at notify, so the driver never waits; a silent device
/// makes the wait give up.
fn no_wait() -> impl FnMut() -> Result<(), Error> {
    let mut left = 3;
    move || {
        left -= 1;
        if left == 0 {
            Err(Error::Device)
        } else {
            Ok(())
        }
    }
}

const WHOLE: u64 = 1024 * 768 * 4;

fn ready(d: &Gpu0) -> FakeGpu {
    let mut gpu = up(d);
    gpu.display_info(&mut no_wait()).unwrap();
    gpu.attach(&[(FRAME, WHOLE)], &mut no_wait()).unwrap();
    d.seen.borrow_mut().clear();
    gpu
}

#[test]
fn the_driver_negotiates_version_1_and_nothing_else() {
    let d = device(Hostile::None);
    d.device.state.borrow_mut().device_features = [u32::MAX, u32::MAX];
    let _gpu = up(&d);
    assert_eq!(d.device.state.borrow().driver_features, [0, 1]);
    let status = d.device.state.borrow().status;
    assert_eq!(status & 4, 4, "DRIVER_OK");
}

#[test]
fn the_driver_refuses_another_device() {
    let d = Device::new(18, BASE, SHARED_MEMORY);
    let refused = Gpu::new(d.registers(), d.shared(), BASE).err();
    assert_eq!(refused, Some(Error::Transport(TransportError::WrongDevice)));
}

#[test]
fn display_attach_and_flush_send_the_right_commands() {
    let d = device(Hostile::None);
    let mut gpu = up(&d);
    assert_eq!(gpu.display_info(&mut no_wait()), Ok((1024, 768)));
    assert_eq!(gpu.size(), (1024, 768));
    // The whole display-info reply fits what the driver offers.
    assert_eq!(d.seen.borrow()[0].room, 24 + 16 * 24);
    let runs = [(FRAME, WHOLE / 2), (FRAME + 0x100_0000, WHOLE / 2), (1, 1)];
    gpu.attach(&runs, &mut no_wait()).unwrap();
    let rect = Rect {
        x: 8,
        y: 16,
        width: 100,
        height: 20,
    };
    gpu.flush(rect, &mut no_wait()).unwrap();
    let seen = d.seen.borrow().clone();
    let kinds: Vec<u32> = seen.iter().map(|s| s.kind).collect();
    assert_eq!(
        kinds,
        [
            cmd::GET_DISPLAY_INFO,
            cmd::RESOURCE_CREATE_2D,
            cmd::RESOURCE_ATTACH_BACKING,
            cmd::SET_SCANOUT,
            cmd::TRANSFER_TO_HOST_2D,
            cmd::RESOURCE_FLUSH,
        ]
    );
    assert_eq!(seen[1].words, [RESOURCE, FORMAT, 1024, 768]);
    // Only the runs that cover the resource: the third is not sent.
    let half = (WHOLE / 2) as u32;
    assert_eq!(
        seen[2].words,
        [
            RESOURCE,
            2,
            FRAME as u32,
            0,
            half,
            0,
            0x100_0000 + FRAME as u32,
            0,
            half,
            0
        ]
    );
    assert_eq!(seen[3].words, [0, 0, 1024, 768, 0, RESOURCE]);
    let offset = (16 * 1024 + 8) * 4;
    assert_eq!(seen[4].words, [8, 16, 100, 20, offset, 0, RESOURCE, 0]);
    assert_eq!(seen[5].words, [8, 16, 100, 20, RESOURCE, 0]);
}

/// INV-0090-2.
#[test]
fn no_rectangle_outside_the_resource_reaches_the_device() {
    let d = device(Hostile::None);
    let mut gpu = ready(&d);
    let bad = [
        (0, 0, 0, 1),
        (0, 0, 1, 0),
        (1024, 0, 1, 1),
        (0, 768, 1, 1),
        (1, 0, 1024, 1),
        (0, 1, 1, 768),
        (u32::MAX, 0, 2, 1),
        (0, u32::MAX, 1, 2),
        (0, 0, u32::MAX, 1),
        (5, 5, u32::MAX - 2, u32::MAX - 2),
    ];
    for (x, y, width, height) in bad {
        let rect = Rect {
            x,
            y,
            width,
            height,
        };
        assert_eq!(
            gpu.flush(rect, &mut no_wait()),
            Err(Error::BadRange),
            "{rect:?}"
        );
    }
    assert!(d.seen.borrow().is_empty());
    let edge = Rect {
        x: 1023,
        y: 767,
        width: 1,
        height: 1,
    };
    assert_eq!(gpu.flush(edge, &mut no_wait()), Ok(()));
}

#[test]
fn flush_before_attach_and_a_second_attach_are_refused() {
    let d = device(Hostile::None);
    let mut gpu = up(&d);
    let rect = Rect {
        x: 0,
        y: 0,
        width: 1,
        height: 1,
    };
    assert_eq!(
        gpu.attach(&[(FRAME, WHOLE)], &mut no_wait()),
        Err(Error::NoDisplay)
    );
    gpu.display_info(&mut no_wait()).unwrap();
    assert_eq!(gpu.flush(rect, &mut no_wait()), Err(Error::NotAttached));
    gpu.attach(&[(FRAME, WHOLE)], &mut no_wait()).unwrap();
    assert_eq!(
        gpu.attach(&[(FRAME, WHOLE)], &mut no_wait()),
        Err(Error::Attached)
    );
}

#[test]
fn backing_that_does_not_cover_the_resource_is_refused() {
    let d = device(Hostile::None);
    let mut gpu = up(&d);
    gpu.display_info(&mut no_wait()).unwrap();
    let cases: [&[(u64, u64)]; 4] = [
        &[],
        &[(FRAME, WHOLE - 1)],
        &[(FRAME, 0), (FRAME, WHOLE)],
        &[(FRAME, u64::from(u32::MAX) + 1)],
    ];
    for runs in cases {
        assert_eq!(gpu.attach(runs, &mut no_wait()), Err(Error::BadBacking));
    }
    let many: Vec<(u64, u64)> = (0..MAX_RUNS as u64 + 1)
        .map(|i| (FRAME + i * 4096, WHOLE / MAX_RUNS as u64 + 1))
        .collect();
    let fits = &many[..MAX_RUNS];
    // Covering takes more runs than one attach carries.
    let small: Vec<(u64, u64)> = (0..WHOLE / 2048)
        .map(|i| (FRAME + i * 4096, 2048))
        .collect();
    assert_eq!(gpu.attach(&small, &mut no_wait()), Err(Error::BadBacking));
    assert!(
        d.seen
            .borrow()
            .iter()
            .all(|s| s.kind == cmd::GET_DISPLAY_INFO)
    );
    assert_eq!(gpu.attach(fits, &mut no_wait()), Ok(()));
}

#[test]
fn a_display_that_is_off_or_too_large_is_refused() {
    for hostile in [Hostile::Disabled, Hostile::Huge] {
        let d = device(hostile);
        let mut gpu = up(&d);
        assert_eq!(gpu.display_info(&mut no_wait()), Err(Error::NoDisplay));
    }
}

/// INV-0090-3.
#[test]
fn every_lie_in_a_response_is_a_protocol_break() {
    for hostile in [
        Hostile::WrongType,
        Hostile::Short,
        Hostile::Long,
        Hostile::WrongHead,
        Hostile::Jumps,
        Hostile::Silent,
    ] {
        let d = device(hostile);
        let mut gpu = up(&d);
        assert_eq!(
            gpu.display_info(&mut no_wait()),
            Err(Error::Device),
            "{hostile:?}"
        );
    }
}

#[test]
fn an_error_response_is_reported_as_such() {
    let d = device(Hostile::Errors);
    let mut gpu = up(&d);
    assert_eq!(
        gpu.display_info(&mut no_wait()),
        Err(Error::Command(cmd::ERR_UNSPEC + 5))
    );
}

#[test]
fn a_response_never_written_does_not_read_as_success() {
    // The device returns the chain with a full length but writes nothing:
    // the type stays the 0 the driver wrote, which is no success.
    let d = Device::new(DEVICE_GPU, BASE, SHARED_MEMORY);
    d.on(|f, e| {
        if e == Event::Notify(0) {
            while let Some(head) = f.next_available(0) {
                let length = f.chain(0, head)[1].length;
                f.complete(0, u32::from(head), length);
            }
        }
    });
    let mut gpu = Gpu::new(d.registers(), d.shared(), BASE).unwrap();
    gpu.shared.write32(RESPONSE_AT, cmd::OK_DISPLAY_INFO);
    assert_eq!(gpu.display_info(&mut no_wait()), Err(Error::Device));
}

#[test]
fn reset_and_acknowledge_reach_the_registers() {
    let d = device(Hostile::None);
    let mut gpu = up(&d);
    d.device.state.borrow_mut().interrupt_status = 3;
    gpu.acknowledge();
    assert_eq!(d.device.state.borrow().interrupt_status, 0);
    assert!(gpu.reset());
    assert_eq!(d.device.registers().read(reg::STATUS), 0);
    d.device.state.borrow_mut().no_reset = true;
    assert!(!gpu.reset());
}

#[test]
fn the_shared_layout_fits_and_does_not_overlap() {
    const { assert!(REQUEST_AT >= QUEUE.end) };
    const { assert!(RESPONSE_AT >= REQUEST_AT + MAX_REQUEST) };
    const { assert!(SHARED_MEMORY >= RESPONSE_AT + DISPLAY_INFO) };
    assert_eq!(SHARED_MEMORY % 4096, 0);
}

/// Holes cargo-mutants found (TASK-14 review).
#[test]
fn a_short_error_response_is_a_protocol_break() {
    let d = device(Hostile::ShortError);
    let mut gpu = up(&d);
    assert_eq!(gpu.display_info(&mut no_wait()), Err(Error::Device));
}

#[test]
fn a_run_of_exactly_the_largest_length_is_accepted() {
    let d = device(Hostile::None);
    let mut gpu = up(&d);
    gpu.display_info(&mut no_wait()).unwrap();
    assert_eq!(
        gpu.attach(&[(FRAME, u64::from(u32::MAX))], &mut no_wait()),
        Ok(())
    );
}

#[test]
fn the_shared_memory_is_what_the_layout_needs() {
    assert_eq!(SHARED_MEMORY, 36864);
}

/// The device reports a new display: its event bit, and the new size.
fn resize(d: &Gpu0, w: u32, h: u32) {
    d.size.set((w, h));
    let mut s = d.device.state.borrow_mut();
    let read = word(&s.config, 0) | config::EVENT_DISPLAY;
    s.config[0..4].copy_from_slice(&read.to_le_bytes());
}

#[test]
fn a_display_event_is_seen_once_and_cleared() {
    let d = device(Hostile::None);
    let mut gpu = ready(&d);
    assert!(!gpu.display_changed());
    resize(&d, 1600, 1000);
    assert!(gpu.display_changed());
    assert_eq!(word(&d.device.state.borrow().config, 0), 0, "cleared");
    assert!(!gpu.display_changed());
    assert_eq!(gpu.size(), (1024, 768), "the size is display_info's to ask");
    assert_eq!(gpu.display_info(&mut no_wait()), Ok((1600, 1000)));
}

#[test]
fn a_new_size_shows_the_other_resource_then_drops_the_old() {
    let d = device(Hostile::None);
    let mut gpu = ready(&d);
    resize(&d, 1600, 1000);
    assert!(gpu.display_changed());
    gpu.display_info(&mut no_wait()).unwrap();
    // Until the new attach, flushes go to the old resource at its size.
    let r = |x, y, width, height| Rect {
        x,
        y,
        width,
        height,
    };
    assert_eq!(
        gpu.flush(r(0, 0, 1600, 1000), &mut no_wait()),
        Err(Error::BadRange)
    );
    d.seen.borrow_mut().clear();
    gpu.flush(r(0, 0, 1024, 768), &mut no_wait()).unwrap();
    assert_eq!(d.seen.borrow()[0].words[6], RESOURCE, "the old one");
    // Backing for the old size does not cover the new one.
    assert_eq!(
        gpu.attach(&[(FRAME, WHOLE)], &mut no_wait()),
        Err(Error::BadBacking)
    );
    d.seen.borrow_mut().clear();
    let big = 1600 * 1000 * 4;
    gpu.attach(&[(FRAME, big)], &mut no_wait()).unwrap();
    let seen: Vec<(u32, Vec<u32>)> = d
        .seen
        .borrow()
        .iter()
        .map(|s| (s.kind, s.words.clone()))
        .collect();
    let kinds: Vec<u32> = seen.iter().map(|s| s.0).collect();
    assert_eq!(
        kinds,
        [
            cmd::RESOURCE_CREATE_2D,
            cmd::RESOURCE_ATTACH_BACKING,
            cmd::SET_SCANOUT,
            cmd::RESOURCE_UNREF
        ]
    );
    assert_eq!(seen[0].1[..4], [2, FORMAT, 1600, 1000], "resource 2");
    assert_eq!(seen[1].1[0], 2);
    assert_eq!(
        seen[2].1[..6],
        [0, 0, 1600, 1000, 0, 2],
        "scanout 0 shows 2"
    );
    assert_eq!(seen[3].1[0], RESOURCE, "then 1 goes");
    // The same size again is refused; flushes reach resource 2 at its size.
    assert_eq!(
        gpu.attach(&[(FRAME, big)], &mut no_wait()),
        Err(Error::Attached)
    );
    d.seen.borrow_mut().clear();
    gpu.flush(r(1000, 900, 600, 100), &mut no_wait()).unwrap();
    let words = d.seen.borrow()[0].words.clone();
    assert_eq!(words[4..7], [(900 * 1600 + 1000) * 4, 0, 2]);
    // A third size goes back to resource 1, and drops 2.
    resize(&d, 1280, 800);
    assert!(gpu.display_changed());
    gpu.display_info(&mut no_wait()).unwrap();
    d.seen.borrow_mut().clear();
    gpu.attach(&[(FRAME, 1280 * 800 * 4)], &mut no_wait())
        .unwrap();
    let seen = d.seen.borrow();
    assert_eq!(seen[0].words[0], RESOURCE);
    assert_eq!(seen[3].words[0], 2);
}
