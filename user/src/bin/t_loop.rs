#![no_std]
#![no_main]

//! Create and destroy, round after round: channels with handles in flight,
//! memory objects with mappings, and pins. The test kernel compares its heap
//! and frame counts around a run.

use zoraya_abi::spawn::Transfer;
use zoraya_abi::{
    ChannelEntry, ChannelObject, FIRST_DEVICE_SLOT, Handle, MapFlags, MemoryFlags, ObjectType,
    Rights,
};
use zoraya_user::{
    channel_create, channel_write, dma_pin, dma_release_quarantine, dma_unpin, handle_close,
    memory_create, memory_map, memory_unmap,
};

const ROUNDS: usize = 200;
const PAGE: u64 = 4096;

fn channels_in_flight() {
    let (a, b) = channel_create().expect("create");
    let (c, d) = channel_create().expect("create");
    let (e, f) = channel_create().expect("create");
    let entry = ChannelEntry {
        handle: c,
        rights: ChannelObject::RIGHTS,
        transfer: Transfer::Move,
    };
    channel_write(a, &[1; 100], &[entry]).expect("write");
    // `c` is in `b`'s queue; `e` goes into `c`'s.
    let entry = ChannelEntry {
        handle: e,
        rights: ChannelObject::RIGHTS,
        transfer: Transfer::Move,
    };
    channel_write(d, b"deeper", &[entry]).expect("write");
    for h in [a, b, d, f] {
        handle_close(h).expect("close");
    }
}

fn mapped_memory() {
    let memory = memory_create(4 * PAGE, MemoryFlags::NONE).expect("memory");
    let kept = memory_map(memory, 0, PAGE, MapFlags::WRITE).expect("map");
    let also = memory_map(memory, PAGE, 3 * PAGE, MapFlags::READ).expect("map");
    handle_close(memory).expect("close");
    memory_unmap(also, 3 * PAGE).expect("unmap");
    memory_unmap(kept, PAGE).expect("unmap");
}

fn pins(device: Handle) {
    let memory = memory_create(PAGE, MemoryFlags::CONTIGUOUS).expect("memory");
    let mut runs = [[0; 2]; 2];
    let pin = dma_pin(memory, device, &mut runs).expect("pin");
    dma_unpin(pin).expect("unpin");
    let pin = dma_pin(memory, device, &mut runs).expect("pin");
    handle_close(pin).expect("into quarantine");
    handle_close(memory).expect("close");
    dma_release_quarantine(device).expect("release");
}

#[unsafe(no_mangle)]
fn main() -> i32 {
    // Any device with a transport: the reset works on all of them.
    let device = Handle::new(u64::from(FIRST_DEVICE_SLOT)).expect("slot");
    let _ = Rights::NONE;
    for _ in 0..ROUNDS {
        channels_in_flight();
        mapped_memory();
        pins(device);
    }
    0
}
