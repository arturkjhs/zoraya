#![no_std]
#![no_main]

//! Throughput through the file protocol (TASK-13). Numbers only.

use alloc::vec;

use zoraya_abi::{Handle, MapFlags, MemoryFlags};
use zoraya_proto_file::open_flags;
use zoraya_user::files::{ALL, File};
use zoraya_user::{clock_monotonic, memory_create, memory_map, println};

extern crate alloc;

const MIB: usize = 1 << 20;

fn rate(bytes: usize, nanos: u64) -> u64 {
    bytes as u64 * 1_000_000_000 / nanos.max(1) / 1024
}

fn inline(root: &File, name: &str, size: usize) {
    let f = root
        .open(name, open_flags::CREATE | open_flags::TRUNCATE, ALL)
        .expect("create");
    let chunk = vec![0x5a; size.min(32 * 1024)];
    let start = clock_monotonic();
    let mut at = 0;
    while at < size {
        f.write(at as u64, 0, &chunk).expect("write");
        at += chunk.len();
    }
    let written = clock_monotonic() - start;
    let start = clock_monotonic();
    root.sync().expect("sync");
    let synced = clock_monotonic() - start;
    let mut out = vec![0; chunk.len()];
    let start = clock_monotonic();
    let mut at = 0;
    while at < size {
        at += f.read(at as u64, &mut out).expect("read");
    }
    let read = clock_monotonic() - start;
    println!(
        "fsbench: inline {size} B: write {} KiB/s, read {} KiB/s, sync {} us",
        rate(size, written),
        rate(size, read),
        synced / 1000
    );
}

fn through_memory(root: &File, name: &str, size: usize) {
    let f = root
        .open(name, open_flags::CREATE | open_flags::TRUNCATE, ALL)
        .expect("create");
    let bytes = size as u64;
    let memory = memory_create(bytes, MemoryFlags::NONE).expect("memory");
    let at = memory_map(memory, 0, bytes, MapFlags::WRITE).expect("map");
    f.attach(memory, bytes).expect("attach");
    // SAFETY: mapped just above, and only this process writes it now.
    unsafe { core::ptr::write_bytes(at as *mut u8, 0x5a, size) };
    let start = clock_monotonic();
    f.write_buffer(0, 0, size as u32).expect("write");
    let written = clock_monotonic() - start;
    let start = clock_monotonic();
    root.sync().expect("sync");
    let synced = clock_monotonic() - start;
    let start = clock_monotonic();
    f.read_buffer(0, 0, size as u32).expect("read");
    let read = clock_monotonic() - start;
    println!(
        "fsbench: memory object {size} B: write {} KiB/s, read {} KiB/s, sync {} us",
        rate(size, written),
        rate(size, read),
        synced / 1000
    );
}

#[unsafe(no_mangle)]
fn main() -> i32 {
    let root = File {
        end: Handle::new(5).expect("slot"),
    };
    inline(&root, "bench-small", 4096);
    inline(&root, "bench-large", MIB);
    through_memory(&root, "bench-small-m", 4096);
    through_memory(&root, "bench-large-m", MIB);
    for name in [
        "bench-small",
        "bench-large",
        "bench-small-m",
        "bench-large-m",
    ] {
        root.remove(name).expect("remove");
    }
    0
}
