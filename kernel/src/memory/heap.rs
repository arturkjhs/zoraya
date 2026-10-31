//! The kernel heap (ADR-0022): `zoraya-heap` behind `GlobalAlloc`, growing
//! through the heap window by mapping frames one at a time.
//!
//! Infallible allocation (`Box::new`, `Vec::push`) panics when memory runs
//! out. It is allowed only at boot and at bounded call sites no user action
//! can influence. Anything a system call can reach uses [`try_box`] and
//! `Vec::try_reserve` and returns the failure.

use alloc::alloc::alloc;
use alloc::boxed::Box;
use core::alloc::{GlobalAlloc, Layout};
use core::ptr;

use zoraya_heap::{Backing, Heap, PAGE_SIZE, Stats};
use zoraya_pmm::OutOfFrames;

use super::layout::{HEAP_WINDOW, HEAP_WINDOW_SIZE};
use super::{frames, paging};
use crate::sync::SpinLock;

struct Window {
    next: u64,
}

impl Window {
    /// Undoes the first `done` pages of a growth.
    fn roll_back(start: u64, done: usize) {
        for i in 0..done {
            let frame = paging::unmap_page(start + i as u64 * PAGE_SIZE as u64);
            frames::free(frame);
        }
    }
}

// SAFETY: `grow` returns fresh addresses from a window nothing else maps,
// never the same twice, backed by frames that were allocated for it and that
// stay mapped read-write for the kernel's lifetime.
unsafe impl Backing for Window {
    fn grow(&mut self, pages: usize) -> Option<usize> {
        let bytes = (pages as u64).checked_mul(PAGE_SIZE as u64)?;
        let end = self.next.checked_add(bytes)?;
        if end > HEAP_WINDOW + HEAP_WINDOW_SIZE || pages as u64 > frames::stats().free {
            return None;
        }
        let start = self.next;
        for i in 0..pages {
            let va = start + i as u64 * PAGE_SIZE as u64;
            let mapped = frames::alloc().map(|frame| (frame, paging::map_page(va, frame)));
            match mapped {
                Ok((_, Ok(()))) => {}
                Ok((frame, Err(_))) => {
                    frames::free(frame);
                    Self::roll_back(start, i);
                    return None;
                }
                Err(OutOfFrames) => {
                    Self::roll_back(start, i);
                    return None;
                }
            }
        }
        self.next = end;
        Some(start as usize)
    }
}

static HEAP: SpinLock<Heap<Window>> = SpinLock::new(Heap::new(Window { next: HEAP_WINDOW }));

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
pub fn try_box<T>(value: T) -> Result<Box<T>, OutOfMemory> {
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

/// Byte counts.
pub fn stats() -> Stats {
    HEAP.lock().stats()
}
