//! The kernel heap (ADR-0022): `zoraya-heap` behind `GlobalAlloc`, growing
//! through the heap window by mapping frames one at a time.
//!
//! Infallible allocation (`Box::new`, `Vec::push`) panics when memory runs
//! out. It is allowed only at boot and at bounded call sites no user action
//! can influence. Anything a system call can reach uses [`uncharged_box`] and
//! `Vec::try_reserve` and returns the failure.

use alloc::alloc::{alloc, dealloc};
use alloc::boxed::Box;
use alloc::sync::Arc;
use core::alloc::{GlobalAlloc, Layout};
use core::ptr;

use zoraya_heap::{Backing, CHUNK_SIZE, Heap, PAGE_SIZE, Stats};
use zoraya_pmm::OutOfFrames;

use super::layout::{HEAP_WINDOW, HEAP_WINDOW_SIZE};
use super::{frames, paging};
use crate::arch;
use crate::steps::{self, STEP};
use crate::sync::SpinLock;

struct Window {
    /// The next address `grow` hands to the heap.
    next: u64,
    /// The end of the pages mapped ahead of `next` by [`prepare`]: `grow`
    /// hands them out without mapping (ADR-0270).
    mapped: u64,
}

impl Window {
    /// Undoes the first `done` pages of a growth.
    fn roll_back(start: u64, done: usize) {
        for i in 0..done {
            let frame = paging::unmap_page(start + i as u64 * PAGE_SIZE as u64);
            frames::free(frame);
        }
    }

    /// Maps `pages` more pages at `mapped`, or none of them.
    fn map_ahead(&mut self, pages: usize) -> bool {
        let start = self.mapped;
        let end = (pages as u64)
            .checked_mul(PAGE_SIZE as u64)
            .and_then(|bytes| start.checked_add(bytes));
        let Some(end) = end else {
            return false;
        };
        if end > HEAP_WINDOW + HEAP_WINDOW_SIZE || pages as u64 > frames::available() {
            return false;
        }
        for i in 0..pages {
            let va = start + i as u64 * PAGE_SIZE as u64;
            let mapped = frames::alloc().map(|frame| (frame, paging::map_page(va, frame)));
            match mapped {
                Ok((_, Ok(()))) => {}
                Ok((frame, Err(_))) => {
                    frames::free(frame);
                    Self::roll_back(start, i);
                    return false;
                }
                Err(OutOfFrames) => {
                    Self::roll_back(start, i);
                    return false;
                }
            }
        }
        self.mapped = end;
        true
    }
}

// SAFETY: `grow` returns fresh addresses from a window nothing else maps,
// never the same twice (`next` only moves up, and `mapped` is never below
// it), backed by frames that were allocated for it and mapped by
// `map_ahead`, here or earlier in `prepare`, and that stay mapped read-write
// for the kernel's lifetime.
unsafe impl Backing for Window {
    /// One chunk (16 pages), not `GROW_SIZE`: a growth for a request of up to
    /// that maps 16 pages in the request's masked interval, the fixed work
    /// beside a step (INV-0270-1); a larger request is prepared ([`prepare`]).
    const LEAST_GROWTH: usize = CHUNK_SIZE;

    fn grow(&mut self, pages: usize) -> Option<usize> {
        let bytes = (pages as u64).checked_mul(PAGE_SIZE as u64)?;
        let end = self.next.checked_add(bytes)?;
        if end > self.mapped && !self.map_ahead(((end - self.mapped) / PAGE_SIZE as u64) as usize) {
            return None;
        }
        let start = self.next;
        self.next = end;
        Some(start as usize)
    }
}

static HEAP: SpinLock<Heap<Window>> = SpinLock::new(Heap::new(Window {
    next: HEAP_WINDOW,
    mapped: HEAP_WINDOW,
}));

struct KernelHeap;

// SAFETY: blocks come from `zoraya-heap`, which meets the layout and never
// hands out overlapping live blocks (ADR-0022, tested on the host); the lock
// serialises every call.
unsafe impl GlobalAlloc for KernelHeap {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        HEAP.lock().alloc(layout)
    }

    unsafe fn dealloc(&self, block: *mut u8, layout: Layout) {
        // SAFETY: forwarded from the caller, who has it from `alloc` above.
        unsafe { HEAP.lock().dealloc(block, layout) }
    }
}

#[global_allocator]
static GLOBAL: KernelHeap = KernelHeap;

/// Out of kernel heap memory.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OutOfMemory;

/// `Box::new` that returns the failure. On failure `value` is dropped.
pub fn uncharged_box<T>(value: T) -> Result<Box<T>, OutOfMemory> {
    let layout = Layout::new::<T>();
    if layout.size() == 0 {
        return Ok(Box::new(value));
    }
    // SAFETY: the layout's size is not zero.
    let block = unsafe { alloc(layout) }.cast::<T>();
    if block.is_null() {
        return Err(OutOfMemory);
    }
    // SAFETY: `block` is a live, unaliased allocation with `T`'s layout, so
    // it can take the value, and `Box` may own and free it.
    unsafe {
        ptr::write(block, value);
        Ok(Box::from_raw(block))
    }
}

/// `Arc::new` that returns the failure, which stable Rust has no form of
/// (ADR-0041). It takes and frees a block of the size `Arc` will ask for: with
/// one CPU, in code that nothing preempts, with the block just freed, the
/// allocation inside `Arc::new` then finds it. SMP must replace this.
pub fn uncharged_arc<T>(value: T) -> Result<Arc<T>, OutOfMemory> {
    #[cfg(feature = "qemu-test")]
    if FAIL_NEXT_ARC.swap(false, core::sync::atomic::Ordering::Relaxed) {
        return Err(OutOfMemory);
    }
    let (layout, _) = Layout::new::<[usize; 2]>()
        .extend(Layout::new::<T>())
        .map_err(|_| OutOfMemory)?;
    let daif = arch::save_and_mask_interrupts();
    let fits = can_allocate(layout.pad_to_align());
    let arc = fits.then(|| Arc::new(value));
    arch::restore_interrupts(daif);
    arc.ok_or(OutOfMemory)
}

/// Makes the next `uncharged_arc` fail, as an exhausted heap would.
#[cfg(feature = "qemu-test")]
pub static FAIL_NEXT_ARC: core::sync::atomic::AtomicBool =
    core::sync::atomic::AtomicBool::new(false);

/// Takes a block of `layout` and gives it back. Size zero needs no block.
pub fn can_allocate(layout: Layout) -> bool {
    if layout.size() == 0 {
        return true;
    }
    // SAFETY: the size is not zero.
    let block = unsafe { alloc(layout) };
    if block.is_null() {
        return false;
    }
    // SAFETY: allocated just above with this layout.
    unsafe { dealloc(block, layout) };
    true
}

/// The bytes the heap counts as live for a block of `layout`.
pub fn block_size(layout: Layout) -> usize {
    zoraya_heap::block_size(layout)
}

/// Maps ahead the pages that an allocation of `bytes` (aligned to 8) would
/// make the heap grow by, if no free span fits it, in steps of [`STEP`] pages
/// with a step point after each, so that the allocation that follows maps
/// none in its own masked interval (INV-0270-1). For an allocation whose size
/// comes from an argument or from what a process holds, made with no spinlock
/// held (a step point's rule): a memory object's list of frames, the list of
/// an address space that a step adds to. Without it a growth maps what the
/// request needs in the interval of the request: at most the least growth
/// (16 pages) for a request of up to that, the fixed work beside a step.
///
/// Never fails: a request the frames cannot cover, or one that other threads
/// keep taking the pages of, maps what is missing in the allocation, which
/// fails or not as it did before. The pages mapped ahead are what the
/// heap's next growth takes; [`stats`]'s `backing` counts them.
pub fn prepare(bytes: usize) {
    prepare_in_steps(bytes, steps::pause);
}

/// [`prepare`] with the step point given, for the test kernel, whose threads
/// are no process and so make none.
pub(crate) fn prepare_in_steps(bytes: usize, mut step_point: impl FnMut()) {
    let Ok(layout) = Layout::from_size_align(bytes, 8) else {
        return;
    };
    if bytes == 0 {
        return;
    }
    // What `zoraya_heap` asks of the backing for this layout: the size in
    // pages, at least a growth.
    let pages = bytes.max(Window::LEAST_GROWTH).div_ceil(PAGE_SIZE) as u64;
    if pages > frames::available() {
        return;
    }
    // The rounds are bounded, so that threads that take the pages at the
    // step points cannot keep this call going.
    for _ in 0..2 * pages.div_ceil(STEP as u64) + 1 {
        let mapped = {
            let mut heap = HEAP.lock();
            if heap.fits(layout) {
                return;
            }
            let window = heap.backing_mut();
            let ahead = (window.mapped - window.next) / PAGE_SIZE as u64;
            if ahead >= pages {
                return;
            }
            window.map_ahead((pages - ahead).min(STEP as u64) as usize)
        };
        if !mapped {
            return;
        }
        step_point();
    }
}

/// Byte counts. `backing` counts the pages mapped ahead by [`prepare`] too:
/// they are frames the heap has taken.
pub fn stats() -> Stats {
    let mut heap = HEAP.lock();
    let window = heap.backing_mut();
    let ahead = (window.mapped - window.next) as usize;
    let mut stats = heap.stats();
    stats.backing += ahead;
    stats
}

/// [`stats`] once the reaper has freed what was queued (it holds heap until
/// then); see `frames::settled_stats`.
#[cfg(feature = "qemu-test")]
pub fn settled_stats() -> Stats {
    crate::reaper::drain();
    stats()
}
