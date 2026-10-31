//! The kernel's frame allocator: `zoraya-pmm` over the device tree's memory
//! (ADR-0021).

use core::slice;

use zoraya_dtb::platform::{MAX_REGIONS, PlatformInfo};
use zoraya_pmm::{
    FRAME_SIZE, Frame, FrameAllocator, OutOfFrames, Range, Stats, place_metadata, storage_words,
};

use super::{kernel_image_phys, phys_to_virt};
use crate::dtb;
use crate::sync::SpinLock;

static FRAMES: SpinLock<Option<FrameAllocator<'static>>> = SpinLock::new(None);

/// Physical range of the allocator's own metadata, once it exists.
static METADATA: SpinLock<Option<Range>> = SpinLock::new(None);

/// At most [`MAX_REGIONS`] from the device tree, then the image, the device
/// tree blob and the metadata.
const MAX_RESERVED: usize = MAX_REGIONS + 3;

/// Starts the allocator over all RAM minus everything ADR-0021 reserves. Call
/// once, with the direct map on.
pub fn init(info: &PlatformInfo<'_>) {
    let mut memory = [Range { start: 0, end: 0 }; MAX_REGIONS];
    for (slot, r) in memory.iter_mut().zip(info.memory.as_slice()) {
        *slot = Range::at(r.address, r.size);
    }
    let memory = &memory[..info.memory.as_slice().len()];

    let mut reserved = [Range { start: 0, end: 0 }; MAX_RESERVED];
    let mut count = 0;
    for r in info.reserved.as_slice() {
        reserved[count] = Range::at(r.address, r.size);
        count += 1;
    }
    let (blob, blob_len) = dtb::phys();
    reserved[count] = kernel_image_phys();
    reserved[count + 1] = Range::at(blob as u64, blob_len as u64);
    count += 2;

    let words = storage_words(memory).expect("memory regions");
    let meta = place_metadata(memory, &reserved[..count], words).expect("room for the frame map");
    reserved[count] = meta;
    count += 1;

    // SAFETY: `meta` lies in RAM that is left by every reservation above, so
    // the direct map has it read-write and nothing else uses it: the
    // allocator built below never hands it out. It starts on a frame boundary
    // and holds `words` u64s. The slice lives as long as the kernel.
    let storage = unsafe { slice::from_raw_parts_mut(phys_to_virt(meta.start) as *mut u64, words) };
    let frames = FrameAllocator::new(memory, &reserved[..count], storage).expect("frame map");
    *FRAMES.lock() = Some(frames);
    *METADATA.lock() = Some(meta);
}

/// One frame, with arbitrary contents.
pub fn alloc() -> Result<Frame, OutOfFrames> {
    FRAMES.lock().as_mut().ok_or(OutOfFrames)?.alloc()
}

/// One zeroed frame.
pub fn alloc_zeroed() -> Result<Frame, OutOfFrames> {
    let frame = alloc()?;
    // SAFETY: the frame is allocated and so is ordinary RAM, which the direct
    // map holds read-write, and it belongs to this function until it returns.
    unsafe {
        (phys_to_virt(frame.addr()) as *mut u8).write_bytes(0, FRAME_SIZE as usize);
    }
    Ok(frame)
}

/// Gives a frame back. A bad free is a kernel bug, so it panics.
pub fn free(frame: Frame) {
    let mut frames = FRAMES.lock();
    let frames = frames.as_mut().expect("frame allocator is up");
    if let Err(e) = frames.free(frame) {
        panic!("bad free of frame {:#x}: {e:?}", frame.addr());
    }
}

/// Frame counts.
pub fn stats() -> Stats {
    FRAMES
        .lock()
        .as_ref()
        .map_or(Stats::default(), FrameAllocator::stats)
}

/// Physical range of the frame map.
#[cfg(feature = "qemu-test")]
pub fn metadata() -> Range {
    METADATA.lock().expect("frame allocator is up")
}
