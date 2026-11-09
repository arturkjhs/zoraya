#![no_std]
#![no_main]

//! The GUI stack in QEMU (ADR-0090 to ADR-0097), run by the test kernel with
//! the GUI devices attached. It starts the real drivers and compositor with
//! the code `init` uses, plays several clients and a fake input driver
//! against them, kills and restarts drivers, and measures a full redraw.
//! Without a GPU it says so and passes.

use alloc::vec;
use alloc::vec::Vec;

use zoraya_abi::spawn::{Encoder, Transfer};
use zoraya_abi::{
    ChannelEntry, Handle, JOB, JobFlags, MapFlags, MemoryFlags, Mode, ObjectType, Packet, Rights,
    Status, channel_signals, process_signals,
};
use zoraya_keymap::code;
use zoraya_proto_gui::{self as gui, status};
use zoraya_proto_input as input;
use zoraya_protocol::{Below, ErrorReply, Header, Message, NO_REPLY, Seq, decode, encode};
use zoraya_user::gui::{Gui, manager_send, scan, wait_reply};
use zoraya_user::{
    channel_create, channel_read, channel_write, clock_monotonic, console_entry, dup, handle_close,
    handle_duplicate, image, job_create, job_kill, memory_create, memory_map, port_create,
    port_subscribe, port_wait, println, process_exit_record, spawn, thread_yield, wait_exit,
};

extern crate alloc;

/// A spawn entry that moves `handle` to `slot`.
fn moved(slot: u32, rights: Rights, handle: Handle) -> zoraya_abi::spawn::HandleEntry {
    zoraya_abi::spawn::HandleEntry {
        slot,
        handle,
        rights,
        transfer: Transfer::Move,
    }
}

const COLOUR_A: u32 = 0x00aa_0000;
const COLOUR_B: u32 = 0x0000_bb00;

struct Client {
    end: Handle,
    memory: Handle,
    pixels: &'static mut [u32],
    width: u32,
    height: u32,
    /// Events seen while waiting for replies: (ordinal, bytes).
    events: Vec<(u32, Vec<u8>)>,
}

fn message<'a, M: Message<'a>>(m: &M, transaction: u32) -> Vec<u8> {
    let mut out = vec![0u8; 4096];
    let n = encode(m, transaction, &mut out).expect("encode");
    out.truncate(n);
    out
}

fn moved_entry(handle: Handle, rights: Rights) -> ChannelEntry {
    ChannelEntry {
        handle,
        rights,
        transfer: Transfer::Move,
    }
}

/// A memory object of whole pages for `pixels` pixels, mapped writable.
fn surface_memory(pixels: usize) -> (Handle, &'static mut [u32]) {
    let size = (pixels as u64 * 4).next_multiple_of(4096);
    let memory = memory_create(size, MemoryFlags::NONE).expect("memory");
    let at = memory_map(memory, 0, size, MapFlags::WRITE).expect("map");
    // SAFETY: a new writable mapping of `size` bytes, kept for the
    // process's life, reached only through this slice.
    let slice = unsafe { core::slice::from_raw_parts_mut(at as *mut u32, pixels) };
    (memory, slice)
}

impl Client {
    fn connect(gui: &Gui) -> Client {
        Client {
            end: gui.new_client().expect("a client"),
            memory: Handle::new(1).expect("slot"),
            pixels: &mut [],
            width: 0,
            height: 0,
            events: Vec::new(),
        }
    }

    /// Sends a request and gives the first reply that is not an event.
    fn call(&mut self, bytes: &[u8], handles: &[ChannelEntry]) -> Vec<u8> {
        channel_write(self.end, bytes, handles).expect("write");
        loop {
            let mut reply = vec![0u8; 256];
            let n = wait_reply(self.end, &mut reply, 5_000_000_000).expect("a reply in time");
            reply.truncate(n);
            let ordinal = Header::decode(&reply).expect("a header").0.ordinal;
            if gui::CLIENT_EVENTS.contains(&ordinal) {
                self.events.push((ordinal, reply));
            } else {
                return reply;
            }
        }
    }

    fn create_with(&mut self, width: u32, height: u32, memory: Handle, rights: Rights) -> u32 {
        let m = message(&gui::Create { width, height }, 2);
        let reply = self.call(&m, &[moved_entry(memory, rights)]);
        decode::<gui::Done>(&reply, 0).expect("Done").1.status.0
    }

    fn create(&mut self, width: u32, height: u32, fill: u32) -> u32 {
        let (memory, pixels) = surface_memory((width * height) as usize);
        pixels.fill(fill);
        let view = handle_duplicate(memory, Rights::READ.union(Rights::TRANSFER)).expect("view");
        let s = self.create_with(width, height, view, Rights::READ);
        if s == status::OK {
            self.memory = memory;
            self.pixels = pixels;
            self.width = width;
            self.height = height;
        }
        s
    }

    fn commit(&mut self, x: u32, y: u32, width: u32, height: u32) -> u32 {
        let m = message(
            &gui::Commit {
                x,
                y,
                width,
                height,
            },
            3,
        );
        let reply = self.call(&m, &[]);
        decode::<gui::FrameDone>(&reply, 0)
            .expect("FrameDone")
            .1
            .status
            .0
    }

    fn whole(&mut self) -> u32 {
        self.commit(0, 0, self.width, self.height)
    }

    /// Takes every event waiting, without blocking.
    fn drain(&mut self) -> Vec<(u32, Vec<u8>)> {
        let mut out = core::mem::take(&mut self.events);
        let mut handles = [self.end; 1];
        loop {
            let mut bytes = vec![0u8; 256];
            match channel_read(self.end, &mut bytes, &mut handles) {
                Ok(sizes) => {
                    assert_eq!(sizes.handles, 0, "the compositor sends no handle");
                    bytes.truncate(sizes.bytes as usize);
                    let ordinal = Header::decode(&bytes).expect("header").0.ordinal;
                    out.push((ordinal, bytes));
                }
                Err((Status::WouldBlock, _)) => return out,
                Err((s, _)) => panic!("read: {s:?}"),
            }
        }
    }

    /// Waits on this client's own channel, the one its keys arrive on, until
    /// `done` holds for what has arrived, or two seconds pass; gives it all.
    /// A round trip to the manager does not order the input driver's
    /// channel, so it cannot tell that a key has reached its client.
    fn drain_until(&mut self, done: impl Fn(&[(u32, Vec<u8>)]) -> bool) -> Vec<(u32, Vec<u8>)> {
        let port = port_create().expect("port");
        let readable = channel_signals::READABLE;
        port_subscribe(port, self.end, readable, 1, Mode::Level).expect("subscribe");
        let until = clock_monotonic() + 2_000_000_000;
        let mut got = self.drain();
        while !done(&got) && clock_monotonic() < until {
            let mut packets = [Packet::default()];
            let _ = port_wait(port, until, &mut packets);
            got.extend(self.drain());
        }
        handle_close(port).expect("close");
        got
    }

    /// Keys, waiting until `count` have arrived (or two seconds).
    fn keys_wait(&mut self, count: usize) -> Vec<(u32, bool, u32, u32)> {
        let is_key = |e: &(u32, Vec<u8>)| decode::<gui::Key>(&e.1, 0).is_ok();
        self.drain_until(|got| got.iter().filter(|e| is_key(e)).count() >= count)
            .iter()
            .filter_map(|(_, b)| decode::<gui::Key>(b, 0).ok())
            .map(|(_, k)| (k.code, k.pressed, k.text.0, k.dropped))
            .collect()
    }

    fn keys(&mut self) -> Vec<(u32, bool, u32, u32)> {
        self.drain()
            .iter()
            .filter_map(|(_, b)| decode::<gui::Key>(b, 0).ok())
            .map(|(_, k)| (k.code, k.pressed, k.text.0, k.dropped))
            .collect()
    }
}

/// The test's own input driver, attached through the manager.
struct FakeInput(Handle);

impl FakeInput {
    fn attach(gui: &Gui) -> FakeInput {
        let (ours, theirs) = channel_create().expect("channel");
        let mut reply = [0u8; 64];
        let n = manager_send(
            gui.manager,
            &gui::AttachInput {},
            Some(theirs),
            7,
            &mut reply,
        )
        .expect("the manager answers");
        let (_, d) = decode::<gui::ManagerDone>(&reply[..n], 0).expect("ManagerDone");
        assert_eq!(d.status.0, status::OK);
        FakeInput(ours)
    }

    fn keys(&self, codes: &[(u32, u32)]) {
        let events: Vec<input::Event> = codes
            .iter()
            .map(|(code, value)| input::Event {
                kind: Below(input::kind::KEY),
                code: *code,
                value: *value,
            })
            .collect();
        let m = input::Frame {
            dropped: 0,
            events: Seq::Of(&events),
        };
        // As a driver would, but waiting: the compositor shares the CPU.
        let bytes = message(&m, NO_REPLY);
        loop {
            match channel_write(self.0, &bytes, &[]) {
                Ok(()) => return,
                Err(Status::WouldBlock) => thread_yield(),
                Err(s) => panic!("input frame: {s:?}"),
            }
        }
    }
}

fn stats(gui: &Gui) -> gui::StatsReply {
    let mut reply = [0u8; 128];
    let n = manager_send(gui.manager, &gui::Stats {}, None, 9, &mut reply).expect("stats");
    decode::<gui::StatsReply>(&reply[..n], 0)
        .expect("StatsReply")
        .1
}

/// True once the display driver has answered more than `flushes` flushes,
/// within a second: the compositor does not wait for it (ADR-0093,
/// Decision), so the count follows the commit's reply.
fn flushed_since(gui: &Gui, flushes: u64) -> bool {
    let until = clock_monotonic() + 1_000_000_000;
    while clock_monotonic() < until {
        if stats(gui).flushes > flushes {
            return true;
        }
        zoraya_user::sleep_until(clock_monotonic() + 10_000_000);
    }
    false
}

/// Lets the compositor run: the test and it share one CPU.
fn settle(gui: &Gui) {
    // A manager round trip means the compositor has taken everything sent
    // before it.
    let _ = stats(gui);
}

fn surfaces_and_commits(a: &mut Client, b: &mut Client) {
    assert_eq!(a.create(120, 80, COLOUR_A), status::OK);
    assert_eq!(b.create(120, 80, COLOUR_B), status::OK);
    assert_eq!(a.whole(), status::OK);
    assert_eq!(b.whole(), status::OK);
    assert_eq!(a.commit(10, 10, 5, 5), status::OK);
    println!("t_gui: surfaces and commits ok");
}

/// INV-0093-3, against the real compositor.
fn hostile_surfaces(gui: &Gui, a: &mut Client) {
    let mut h = Client::connect(gui);
    assert_eq!(h.commit(0, 0, 1, 1), status::NO_SURFACE);
    let (w, hh) = (1024u32, 768u32);
    for (width, height) in [
        (0, 1),
        (1, 0),
        (w + 1, 1),
        (1, hh + 1),
        (u32::MAX, u32::MAX),
    ] {
        let (memory, _) = surface_memory(1024);
        assert_eq!(
            h.create_with(width, height, memory, Rights::READ.union(Rights::TRANSFER)),
            status::BAD_BUFFER
        );
    }
    // A memory object smaller than declared.
    let (small, _) = surface_memory(1024);
    assert_eq!(
        h.create_with(100, 100, small, Rights::READ.union(Rights::TRANSFER)),
        status::BAD_BUFFER
    );
    // Not a memory object.
    let (x, y) = channel_create().expect("channel");
    assert_eq!(
        h.create_with(10, 10, x, zoraya_abi::ChannelObject::RIGHTS),
        status::BAD_BUFFER
    );
    let _ = handle_close(y);
    assert_eq!(h.create(64, 64, 0x0012_3456), status::OK);
    assert_eq!(h.create(64, 64, 0), status::EXISTS);
    for (x, y, width, height) in [
        (0, 0, 65, 1),
        (64, 0, 1, 1),
        (0, 0, 0, 0),
        (u32::MAX, 0, 2, 1),
        (1, 1, 64, 64),
    ] {
        assert_eq!(h.commit(x, y, width, height), status::BAD_RANGE);
    }
    assert_eq!(a.whole(), status::OK, "others are served");
    let _ = handle_close(h.end);
    println!("t_gui: hostile surfaces and damage ok");
}

/// A buffer rewritten during composition: a child writes it without pause
/// while its owner commits.
fn rewritten_during_composition(gui: &Gui, a: &mut Client) {
    let mut h = Client::connect(gui);
    assert_eq!(h.create(256, 256, 1), status::OK);
    let job = job_create(JOB, JobFlags::NONE).expect("job");
    let program = image("guibench");
    let mut buffer = [0u8; 512];
    let mut e = Encoder::new(&mut buffer, job, program);
    e.handle(console_entry());
    e.handle(dup(5, h.memory, Rights::READ.union(Rights::WRITE)));
    e.arg(b"scribble");
    let length = e.finish().expect("spawn buffer");
    let scribbler = spawn(&buffer[..length]).expect("spawn");
    let _ = handle_close(program);
    for _ in 0..20 {
        assert_eq!(h.whole(), status::OK);
        assert_eq!(a.commit(0, 0, 8, 8), status::OK);
    }
    job_kill(job).expect("kill");
    wait_exit(scribbler);
    let _ = handle_close(scribbler);
    let _ = handle_close(job);
    let _ = handle_close(h.end);
    println!("t_gui: a buffer rewritten during composition ok");
}

/// INV-0093-1, INV-0094-1, INV-0094-3: keys to the focused client only,
/// F12 the compositor's, a deaf client loses only its own events.
fn routing_and_focus(gui: &Gui, keyboard: &FakeInput, a: &mut Client, b: &mut Client) {
    let focused = |e: &(u32, Vec<u8>)| decode::<gui::Focus>(&e.1, 0).is_ok_and(|(_, f)| f.focused);
    let is_key = |e: &(u32, Vec<u8>)| decode::<gui::Key>(&e.1, 0).is_ok();
    a.drain();
    b.drain();
    keyboard.keys(&[(30, 1), (30, 0)]);
    assert_eq!(
        a.keys_wait(2),
        [(30, true, 'a' as u32, 0), (30, false, 0, 0)]
    );
    // The compositor has read the frame. It is one thread, so a manager
    // round trip now proves it has routed all of it: b would have had any.
    settle(gui);
    assert!(b.keys().is_empty(), "b is not focused");
    keyboard.keys(&[(code::F12, 1), (code::F12, 0)]);
    let mut to_b = b.drain_until(|got| got.iter().any(focused));
    // The release follows the press in the same frame.
    settle(gui);
    to_b.extend(b.drain());
    let to_a = a.drain();
    assert!(
        !to_a.iter().any(is_key) && !to_b.iter().any(is_key),
        "F12 reached nobody"
    );
    assert!(
        to_b.iter().any(focused),
        "b got the focus within two seconds"
    );
    keyboard.keys(&[(48, 1)]);
    assert_eq!(b.keys_wait(1), [(48, true, 'b' as u32, 0)]);
    settle(gui);
    assert!(a.keys().is_empty());
    // b stops reading: its queue fills, and a is served throughout.
    for _ in 0..100 {
        keyboard.keys(&[(48, 1), (48, 0)]);
    }
    settle(gui);
    assert_eq!(a.commit(0, 0, 4, 4), status::OK);
    let first = b.keys();
    assert!(first.len() < 200, "some were dropped: {}", first.len());
    keyboard.keys(&[(48, 1)]);
    // The first event b gets after it had no room carries the count. (The
    // compositor may still have been taking the flood when b drained, so
    // what follows is not only the last key.)
    let after = b.keys_wait(1);
    assert!(
        after.first().is_some_and(|k| k.3 > 0),
        "the count arrives: {after:?}"
    );
    // Back to a for the rest. Its Focus must arrive here, not in the next
    // test, which expects a to hear nothing; the flood's tail may still be
    // queued ahead of the F12 frame.
    keyboard.keys(&[(code::F12, 1), (code::F12, 0)]);
    let back = a.drain_until(|got| got.iter().any(focused));
    assert!(
        back.iter().any(focused),
        "a got the focus back within two seconds"
    );
    settle(gui);
    a.drain();
    b.drain();
    println!("t_gui: routing, focus and a deaf client ok");
}

/// INV-0093-2: manager and input messages mean nothing on a client channel,
/// and the other client hears nothing of them; client memory stays as its
/// owner wrote it.
fn no_injection(gui: &Gui, a: &mut Client, b: &mut Client) {
    a.drain();
    let frame = input::Frame {
        dropped: 0,
        events: Seq::Of(&[input::Event {
            kind: Below(input::kind::KEY),
            code: 30,
            value: 1,
        }]),
    };
    let attempts: [(Vec<u8>, bool); 6] = [
        (message(&gui::AddClient {}, 5), true),
        (message(&gui::AttachDisplay {}, 5), true),
        (message(&gui::AttachInput {}, 5), true),
        (message(&gui::Stats {}, 5), false),
        (message(&frame, 5), false),
        (
            message(
                &gui::Key {
                    dropped: 0,
                    code: 30,
                    pressed: true,
                    modifiers: zoraya_protocol::Flags(0),
                    text: Below('a' as u32),
                },
                5,
            ),
            false,
        ),
    ];
    for (bytes, with_handle) in attempts {
        let handles: Vec<ChannelEntry> = if with_handle {
            let (x, y) = channel_create().expect("channel");
            let _ = handle_close(y);
            vec![moved_entry(x, zoraya_abi::ChannelObject::RIGHTS)]
        } else {
            Vec::new()
        };
        let reply = b.call(&bytes, &handles);
        let (_, e) = decode::<ErrorReply>(&reply, 0).expect("an error reply");
        assert_eq!(e.status, status::UNSUPPORTED);
    }
    settle(gui);
    assert!(a.drain().is_empty(), "a heard nothing");
    assert!(
        a.pixels.iter().all(|p| *p == COLOUR_A),
        "a's pixels are a's"
    );
    assert!(
        b.pixels.iter().all(|p| *p == COLOUR_B),
        "b's pixels are b's"
    );
    println!("t_gui: no client can inject or read ok");
}

/// INV-0097-2: a killed GPU or input driver is restarted alone, and the
/// compositor takes the new one; the new GPU driver answers a flush.
fn driver_restarts(gui: &mut Gui, a: &mut Client) {
    let gpud = gui.gpud.expect("a GPU driver");
    job_kill(gpud.job).expect("kill the GPU driver");
    wait_exit(gpud.process);
    // The compositor sees the driver go at its next flush, and goes on.
    assert_eq!(a.whole(), status::OK);
    assert!(gui.restart_gpu(), "restart the GPU driver");
    settle(gui);
    let before = stats(gui);
    assert_eq!(a.whole(), status::OK);
    assert!(
        flushed_since(gui, before.flushes),
        "the new driver answers flushes"
    );
    if let Some(Some(r)) = gui.inputd.first().copied() {
        {
            job_kill(r.job).expect("kill an input driver");
            wait_exit(r.process);
            // `restart_input` is true only once the compositor accepted the
            // new driver's channel.
            assert!(gui.restart_input(0), "restart the input driver");
            settle(gui);
            let r = gui.inputd[0].expect("a new input driver");
            for _ in 0..50 {
                thread_yield();
            }
            assert!(
                process_exit_record(r.process).is_err(),
                "the new input driver runs"
            );
        }
    }
    let _ = process_signals::TERMINATED;
    assert_eq!(a.whole(), status::OK);
    println!("t_gui: driver restarts ok");
}

/// ADR-0093, Decision: a display driver that never answers stalls nobody.
/// Clients keep getting frame callbacks and keys; at the deadline the
/// compositor drops the driver and tells the manager.
fn a_hung_display_driver_stalls_nobody(
    gui: &mut Gui,
    keyboard: &FakeInput,
    a: &mut Client,
    b: &mut Client,
) {
    let gpud = gui.gpud.take().expect("a GPU driver");
    gpud.end();
    let (ours, theirs) = channel_create().expect("channel");
    let job = job_create(JOB, JobFlags::NONE).expect("job");
    let program = image("guibench");
    let mut buffer = [0u8; 512];
    let mut e = Encoder::new(&mut buffer, job, program);
    e.handle(console_entry());
    e.handle(moved(5, zoraya_abi::ChannelObject::RIGHTS, theirs));
    e.arg(b"hang-display");
    let length = e.finish().expect("spawn buffer");
    let hung = spawn(&buffer[..length]).expect("spawn");
    let _ = handle_close(program);
    let mut reply = [0u8; 64];
    let n = manager_send(
        gui.manager,
        &gui::AttachDisplay {},
        Some(ours),
        1,
        &mut reply,
    )
    .expect("the manager answers");
    let (_, d) = decode::<gui::ManagerDone>(&reply[..n], 0).expect("ManagerDone");
    assert_eq!(d.status.0, status::OK);
    let before = stats(gui);
    a.drain();
    b.drain();
    let start = clock_monotonic();
    for _ in 0..10 {
        let t = clock_monotonic();
        assert_eq!(a.whole(), status::OK);
        assert_eq!(b.commit(0, 0, 4, 4), status::OK);
        keyboard.keys(&[(30, 1), (30, 0)]);
        settle(gui);
        assert_eq!(a.keys().len(), 2, "keys still arrive");
        assert!(
            clock_monotonic() - t < 500_000_000,
            "a round took over 0.5 s"
        );
    }
    assert!(clock_monotonic() - start < zoraya_compositor::FLUSH_DEADLINE_NS);
    let during = stats(gui);
    assert!(during.frames > before.frames, "frames are composed");
    assert_eq!(during.flushes, before.flushes, "and none is answered");
    // Past the deadline the compositor gives up on the driver by itself.
    zoraya_user::sleep_until(start + zoraya_compositor::FLUSH_DEADLINE_NS + 500_000_000);
    let after = stats(gui);
    assert_eq!(after.hangs, before.hangs + 1);
    zoraya_user::gui::read_manager_events(gui.manager);
    assert!(
        zoraya_user::gui::take_display_hung(),
        "the manager was told"
    );
    assert_eq!(a.whole(), status::OK, "still serving");
    job_kill(job).expect("kill");
    wait_exit(hung);
    let _ = handle_close(hung);
    let _ = handle_close(job);
    assert!(gui.restart_gpu(), "a real driver again");
    let before = stats(gui);
    assert_eq!(a.whole(), status::OK);
    assert!(flushed_since(gui, before.flushes), "and it answers");
    println!("t_gui: a hung display driver stalls nobody ok");
}

/// Measurements: a full-screen surface committed whole, again and again.
fn full_redraw(gui: &Gui) {
    let mut f = Client::connect(gui);
    assert_eq!(f.create(1024, 768, 0x0040_4040), status::OK);
    assert_eq!(f.whole(), status::OK);
    let rounds = 30;
    let before = stats(gui);
    let start = clock_monotonic();
    for i in 0..rounds {
        f.pixels.fill(0x0010_1010 * (i % 8));
        assert_eq!(f.whole(), status::OK);
    }
    let elapsed = clock_monotonic() - start;
    let after = stats(gui);
    let frames = (after.frames - before.frames).max(1);
    let fps_x10 = u64::from(rounds) * 10_000_000_000 / elapsed.max(1);
    println!(
        "t_gui: measure: full redraw {}.{} frames/s, compose {} us/frame, flush {} us/frame",
        fps_x10 / 10,
        fps_x10 % 10,
        (after.compose_ns - before.compose_ns) / frames / 1000,
        (after.flush_ns - before.flush_ns) / frames / 1000
    );
    let _ = handle_close(f.end);
}

/// 7b's measurements (plan.md): a frame composed with the switcher open
/// (its glass blurs what is under it), and frames committed under it.
fn switcher_cost(gui: &Gui, keyboard: &FakeInput) {
    let mut f = Client::connect(gui);
    assert_eq!(f.create(600, 400, 0x0030_6090), status::OK);
    assert_eq!(f.whole(), status::OK);
    settle(gui);
    let before = stats(gui);
    keyboard.keys(&[(code::LEFT_ALT, 1), (code::TAB, 1), (code::TAB, 0)]);
    // Two round trips: the input and the frame it causes are behind them.
    settle(gui);
    settle(gui);
    let opened = stats(gui);
    let rounds = 10;
    for i in 0..rounds {
        f.pixels.fill(0x0010_1010 * (i % 8));
        assert_eq!(f.whole(), status::OK);
    }
    let under = stats(gui);
    keyboard.keys(&[(code::LEFT_ALT, 0)]);
    settle(gui);
    let per = |a: &gui::StatsReply, b: &gui::StatsReply| {
        (b.compose_ns - a.compose_ns) / (b.frames - a.frames).max(1) / 1000
    };
    println!(
        "t_gui: measure: switcher open {} us/frame, frames under it {} us/frame",
        per(&before, &opened),
        per(&opened, &under)
    );
    let _ = handle_close(f.end);
}

#[unsafe(no_mangle)]
fn main() -> i32 {
    let devices = scan();
    let Some(gpu) = devices.gpu else {
        println!("t_gui: no GPU attached: skipped");
        return 0;
    };
    let mut gui = Gui::start(gpu, &devices.inputs).expect("the GUI starts");
    let keyboard = FakeInput::attach(&gui);
    let mut a = Client::connect(&gui);
    let mut b = Client::connect(&gui);
    surfaces_and_commits(&mut a, &mut b);
    hostile_surfaces(&gui, &mut a);
    rewritten_during_composition(&gui, &mut a);
    routing_and_focus(&gui, &keyboard, &mut a, &mut b);
    no_injection(&gui, &mut a, &mut b);
    driver_restarts(&mut gui, &mut a);
    a_hung_display_driver_stalls_nobody(&mut gui, &keyboard, &mut a, &mut b);
    full_redraw(&gui);
    switcher_cost(&gui, &keyboard);
    let _ = handle_close(a.end);
    let _ = handle_close(b.end);
    let _ = handle_close(keyboard.0);
    gui.end();
    println!("t_gui: all passed");
    0
}
