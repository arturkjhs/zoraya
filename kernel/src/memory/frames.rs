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
///
/// When none is free and the reaper still holds frames, it frees a step of
/// them and this tries again (ADR-0270 point 4): memory that is on its way
/// back is not exhaustion.
pub fn alloc() -> Result<Frame, OutOfFrames> {
    crate::steps::count(1);
    loop {
        let found = FRAMES.lock().as_mut().ok_or(OutOfFrames)?.alloc();
        match found {
            Err(e) if !crate::reaper::help() => return Err(e),
            Err(_) => {}
            Ok(frame) => return Ok(frame),
        }
    }
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

/// The most frames one contiguous run may have: 4 MiB. A run is found by a
/// scan of the frame map and is zeroed, both with IRQ masked unless the
/// zeroing is cut into steps; the drivers' queues and buffers need at most
/// 2 MiB (`fsd`'s data area), and nothing else asks (ADR-0270 open
/// questions).
pub const MAX_CONTIG_FRAMES: u64 = 1024;

/// Rounds of `reaper::help` a run's search makes before it fails: the scan of
/// the frame map is repeated after each, so the number bounds the masked time.
const CONTIG_HELP_ROUNDS: usize = 8;

/// `count` contiguous frames, not zeroed: the caller zeroes them with
/// [`zero_run`] in steps, before anything else can reach them.
pub fn alloc_contig(count: u64) -> Result<Frame, OutOfFrames> {
    if count > MAX_CONTIG_FRAMES {
        return Err(OutOfFrames);
    }
    for round in 0.. {
        let found = FRAMES
            .lock()
            .as_mut()
            .ok_or(OutOfFrames)?
            .alloc_contig(count, 1);
        match found {
            Err(e) if round == CONTIG_HELP_ROUNDS || !crate::reaper::help() => return Err(e),
            Err(_) => {}
            Ok(first) => {
                crate::steps::count(1);
                return Ok(first);
            }
        }
    }
    unreachable!()
}

/// Zeroes `count` frames of the run at `first`, counted as work.
pub fn zero_run(first: Frame, count: u64) {
    crate::steps::count(count as usize);
    // SAFETY: as in `alloc_zeroed`, for each frame of the run, which belongs
    // to the caller until it is shared.
    unsafe {
        (phys_to_virt(first.addr()) as *mut u8).write_bytes(0, (count * FRAME_SIZE) as usize);
    }
}

/// Gives a run back.
pub fn free_contig(first: Frame, count: u64) {
    crate::steps::count(count as usize);
    let mut frames = FRAMES.lock();
    let frames = frames.as_mut().expect("frame allocator is up");
    if let Err(e) = frames.free_contig(first, count) {
        panic!("bad free of run at {:#x}: {e:?}", first.addr());
    }
}

/// Gives a frame back. A bad free is a kernel bug, so it panics.
pub fn free(frame: Frame) {
    crate::steps::count(1);
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

/// [`stats`] once the reaper has freed what was queued (ADR-0270): for a test
/// that compares counts with its starting values. Not for code that holds a
/// lock, since it yields.
#[cfg(feature = "qemu-test")]
pub fn settled_stats() -> Stats {
    crate::reaper::drain();
    stats()
}
