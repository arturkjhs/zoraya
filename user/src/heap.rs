//! The heap of a user program: `zoraya-heap` over memory objects (ADR-0080).
//! A process has one thread, so nothing here needs a lock.

use core::alloc::{GlobalAlloc, Layout};
use core::cell::UnsafeCell;

use zoraya_abi::{MapFlags, MemoryFlags};
use zoraya_heap::{Backing, Heap, PAGE_SIZE};
use zoraya_rt::{memory_create, memory_map};

struct Objects;

// SAFETY: a mapping of a new memory object is page-aligned, readable, writable
// and this process's own. The handle stays open and the mapping stays for the
// process's life, and the kernel chooses addresses that do not overlap.
unsafe impl Backing for Objects {
    fn grow(&mut self, pages: usize) -> Option<usize> {
        let size = (pages * PAGE_SIZE) as u64;
        let object = memory_create(size, MemoryFlags::NONE).ok()?;
        let address = memory_map(object, 0, size, MapFlags::WRITE).ok()?;
        Some(address as usize)
    }
}

struct UserHeap(UnsafeCell<Heap<Objects>>);

// SAFETY: a process has one thread.
unsafe impl Sync for UserHeap {}

// SAFETY: `Heap` returns blocks for the layout asked, or null.
unsafe impl GlobalAlloc for UserHeap {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // SAFETY: one thread, and `Heap` calls nothing that allocates.
        unsafe { (*self.0.get()).alloc(layout) }
    }

    unsafe fn dealloc(&self, block: *mut u8, layout: Layout) {
        // SAFETY: as in `alloc`, and the caller's contract is `Heap::dealloc`'s.
        unsafe { (*self.0.get()).dealloc(block, layout) }
    }
}

#[global_allocator]
static HEAP: UserHeap = UserHeap(UnsafeCell::new(Heap::new(Objects)));
