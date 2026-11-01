#![no_std]
#![no_main]

//! The measurements plan.md asks for. Numbers only: nothing depends on them.

use zoraya_abi::spawn::{Encoder, HandleEntry, Transfer};
use zoraya_abi::{
    ChannelObject, DEADLINE_INFINITE, DIRECTORY, Handle, JOB, MapFlags, MemoryFlags, Mode,
    ObjectType, Packet, channel_signals,
};
use zoraya_user::{
    channel_create, channel_read, channel_write, clock_monotonic, directory_lookup, handle_close,
    memory_create, memory_map, port_create, port_subscribe, port_wait, println, spawn,
};

const ROUNDS: usize = 2_000;
const LARGE: usize = 60 * 1024;
static mut BUFFER: [u8; LARGE] = [0; LARGE];
static mut SAMPLES: [u32; ROUNDS] = [0; ROUNDS];

fn buffer() -> &'static mut [u8] {
    // SAFETY: one thread, and each caller's slice ends before the next.
    unsafe { &mut *core::ptr::addr_of_mut!(BUFFER) }
}

fn median(mut f: impl FnMut()) -> u32 {
    // SAFETY: as in `buffer`.
    let samples = unsafe { &mut *core::ptr::addr_of_mut!(SAMPLES) };
    for sample in samples.iter_mut() {
        let start = clock_monotonic();
        f();
        *sample = (clock_monotonic() - start) as u32;
    }
    samples.sort_unstable();
    samples[ROUNDS / 2]
}

fn wait(port: Handle) {
    let mut packets = [Packet::default()];
    port_wait(port, DEADLINE_INFINITE, &mut packets).expect("wait");
}

fn echo_child(end: Handle) -> Handle {
    let image = directory_lookup(DIRECTORY, "child").expect("child");
    let mut buffer = [0; 256];
    let mut e = Encoder::new(&mut buffer, JOB, image);
    e.handle(HandleEntry {
        slot: 3,
        handle: end,
        rights: ChannelObject::RIGHTS,
        transfer: Transfer::Move,
    });
    e.arg(b"channel-echo-loop");
    let length = e.finish().expect("fits");
    let child = spawn(&buffer[..length]).expect("spawn");
    handle_close(image).expect("close");
    child
}

#[unsafe(no_mangle)]
fn main() -> i32 {
    let (a, b) = channel_create().expect("create");
    let child = echo_child(b);
    let port = port_create().expect("port");
    port_subscribe(port, a, channel_signals::READABLE, 1, Mode::Level).expect("subscribe");
    let call = |bytes: usize| {
        channel_write(a, &buffer()[..bytes], &[]).expect("write");
        wait(port);
        channel_read(a, buffer(), &mut []).expect("read");
    };
    let small = median(|| call(16));
    let large = median(|| call(LARGE));
    println!("chanbench: round trip 16 B: {small} ns, {LARGE} B: {large} ns");
    let per_kib = (u64::from(large) - u64::from(small)) * 1024 / (2 * LARGE as u64 - 32);
    println!("chanbench: inline copy, one way, per KiB: {per_kib} ns");

    // The same bytes through a mapped object: a copy into it, and a small call.
    let memory =
        memory_create(LARGE.next_multiple_of(4096) as u64, MemoryFlags::NONE).expect("memory");
    let at = memory_map(
        memory,
        0,
        LARGE.next_multiple_of(4096) as u64,
        MapFlags::WRITE,
    )
    .expect("map");
    let mapped = median(|| {
        // SAFETY: mapped just above, and this process's alone here.
        unsafe {
            core::ptr::copy_nonoverlapping(buffer().as_ptr(), at as *mut u8, LARGE);
        }
        call(16);
    });
    println!("chanbench: {LARGE} B through a mapped object: {mapped} ns a round trip");

    // `port_wait` with one channel, and with many idle.
    let ready = |port: Handle| {
        median(|| {
            channel_write(a, b"x", &[]).expect("write");
            wait(port);
            channel_read(a, buffer(), &mut []).expect("read");
        })
    };
    let one = ready(port);
    const IDLE: usize = 400;
    let mut ends = [a; 2 * IDLE];
    for i in 0..IDLE {
        let (x, y) = channel_create().expect("create");
        ends[2 * i] = x;
        ends[2 * i + 1] = y;
        port_subscribe(
            port,
            x,
            channel_signals::READABLE,
            100 + i as u64,
            Mode::Level,
        )
        .expect("subscribe");
    }
    let many = ready(port);
    println!("chanbench: call with 1 channel: {one} ns, with {IDLE} more idle: {many} ns");
    for h in ends {
        handle_close(h).expect("close");
    }
    for h in [a, port, child, memory] {
        handle_close(h).expect("close");
    }
    0
}
