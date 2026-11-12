//! The kernel's frame allocator: `zoraya-pmm` over the device tree's memory
//! (ADR-0021).

use core::slice;

use zoraya_abi::Status;
use zoraya_dtb::platform::{MAX_REGIONS, PlatformInfo};
use zoraya_pmm::{
    FRAME_SIZE, Frame, FrameAllocator, OutOfFrames, Range, RunSearch, Stats, place_metadata,
    storage_words,
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
/// When none is free and the reaper still holds frames, it takes one of
/// them (ADR-0270 point 4): memory that is on its way back is not
/// exhaustion, and the allocation stays one unit of work (INV-0270-1). The
/// unit is counted at the caller's line, as [`free`]'s is, so that the
/// diagnosis of a busy interval names the path that allocated (the heap's
/// growth, a page table, an object).
#[track_caller]
pub fn alloc() -> Result<Frame, OutOfFrames> {
    crate::steps::count(1);
    let found = FRAMES.lock().as_mut().ok_or(OutOfFrames)?.alloc();
    found.or_else(|e| crate::reaper::take().ok_or(e))
}

/// One zeroed frame, counted at the caller's line.
#[track_caller]
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
/// the frame map is repeated after each, so the number bounds the work.
const CONTIG_HELP_ROUNDS: usize = (MAX_CONTIG_FRAMES / crate::steps::STEP as u64) as usize + 1;

/// Words of the frame map a run's search reads between two step points (each
/// counted as a unit of work): with the candidate run that may pass it (at
/// most `MAX_CONTIG_FRAMES / 64 + 2` words), within a step.
const SCAN_WORDS: u64 = 48;

/// `count` contiguous frames, not zeroed: the caller zeroes them with
/// [`zero_run`] in steps, before anything else can reach them.
///
/// The scan of the frame map is made in steps of [`SCAN_WORDS`] words with a
/// step point (`steps::pause`) between them, the frame map's lock released;
/// a run is taken only once all of it was read free under the lock
/// (`zoraya_pmm::FrameAllocator::alloc_contig_step`). When none is found and
/// the reaper holds frames, each round of its help frees a step of them
/// between two step points, and the scan starts again. No interval holds
/// more than a step of either (INV-0270-1); the caller holds no spinlock, as
/// for any step point.
pub fn alloc_contig(count: u64) -> Result<Frame, Status> {
    alloc_contig_in_steps(count, crate::steps::between)
}

/// [`alloc_contig`] with the step point given, for the test kernel, whose
/// threads are no process and so make none. A step point that fails (the
/// process was asked to end, `steps::between`) ends the search with its
/// status, so that a kill is not waited out by a scan that grows with the
/// machine's memory (ADR-0270 point 3); the caller frees nothing of the run,
/// which it did not take yet.
///
/// The scan is not atomic: the lock is released between steps. If frames were
/// freed since the scan began (`FrameAllocator::generation`), a run may have
/// appeared behind it, so a scan that found none is made once more, counted
/// as a round of help; `memory_create` of a contiguous object does not fail
/// for a run that was free at every instant.
pub(crate) fn alloc_contig_in_steps(
    count: u64,
    mut step_point: impl FnMut() -> Result<(), Status>,
) -> Result<Frame, Status> {
    if count > MAX_CONTIG_FRAMES {
        return Err(Status::NoMemory);
    }
    let mut rescanned = false;
    for round in 0.. {
        let mut search = RunSearch::default();
        let mut began = None;
        let (found, changed) = loop {
            let (step, generation) = {
                let mut frames = FRAMES.lock();
                let frames = frames.as_mut().ok_or(Status::NoMemory)?;
                let step = frames.alloc_contig_step(count, 1, &mut search, SCAN_WORDS);
                (step, frames.generation())
            };
            let began = *began.get_or_insert(generation);
            crate::steps::count(search.words_read() as usize);
            match step {
                Ok(None) => step_point()?,
                Ok(Some(first)) => break (Ok(first), false),
                Err(e) => break (Err(e), generation != began),
            }
        };
        match found {
            Ok(first) => {
                crate::steps::count(1);
                return Ok(first);
            }
            Err(_) if changed && !rescanned && round < CONTIG_HELP_ROUNDS => {
                rescanned = true;
                step_point()?;
            }
            Err(_) if round == CONTIG_HELP_ROUNDS => return Err(Status::NoMemory),
            Err(_) => {
                step_point()?;
                if !crate::reaper::help() {
                    return Err(Status::NoMemory);
                }
                step_point()?;
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

/// Gives a run back, counted at the caller's line as [`free`] is.
#[track_caller]
pub fn free_contig(first: Frame, count: u64) {
    crate::steps::count(count as usize);
    let mut frames = FRAMES.lock();
    let frames = frames.as_mut().expect("frame allocator is up");
    if let Err(e) = frames.free_contig(first, count) {
        panic!("bad free of run at {:#x}: {e:?}", first.addr());
    }
}

/// Gives a frame back. A bad free is a kernel bug, so it panics. The units
/// are counted at the caller's line, so that the test kernel's diagnosis of
/// a busy interval names the path that freed (`steps::diagnose_units`).
#[track_caller]
pub fn free(frame: Frame) {
    crate::steps::count(1);
    let mut frames = FRAMES.lock();
    let frames = frames.as_mut().expect("frame allocator is up");
    if let Err(e) = frames.free(frame) {
        panic!("bad free of frame {:#x}: {e:?}", frame.addr());
    }
}

/// The frames an allocation can get: the free ones and those the reaper
/// holds, which [`alloc`] takes when none is free. For a check before many
/// allocations (the heap's growth), so that memory on its way back is not
/// taken for exhaustion.
pub fn available() -> u64 {
    stats().free + crate::reaper::held()
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
