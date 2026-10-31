//! Kernel heap allocator logic (ADR-0022), independent of `GlobalAlloc`.
//!
//! Requests of up to [`MAX_SMALL`] bytes go to a power-of-two size class.
//! Each class is a free list threaded through its free blocks and refilled
//! from a [`CHUNK_SIZE`] span. Larger requests take whole pages from an
//! address-ordered list of free spans, threaded through the spans, which
//! coalesces on free. The span list grows from a [`Backing`].
//!
//! The caller passes the layout to `dealloc`, so blocks have no headers.
//! Memory given to a size class stays in that class.

#![no_std]

use core::alloc::Layout;
use core::ptr;

/// Page size of the backing memory.
pub const PAGE_SIZE: usize = 4096;
/// Largest request served by a size class.
pub const MAX_SMALL: usize = 2048;
/// Span size a size class is refilled with.
pub const CHUNK_SIZE: usize = 64 * 1024;
/// Least memory asked of the backing at a time.
pub const GROW_SIZE: usize = 1024 * 1024;

const MIN_CLASS_SHIFT: u32 = 4;
const CLASSES: usize = (MAX_SMALL.trailing_zeros() - MIN_CLASS_SHIFT + 1) as usize;

/// Where the heap gets memory.
///
/// # Safety
///
/// Every address `grow` returns must be page-aligned and start `pages` pages
/// of memory that are readable, writable, not used by anything else and
/// valid for as long as the heap exists. Ranges from separate calls must not
/// overlap.
pub unsafe trait Backing {
    /// More memory, or `None` if there is none. The heap coalesces ranges
    /// that happen to be adjacent, but does not rely on it.
    fn grow(&mut self, pages: usize) -> Option<usize>;
}

/// Byte counts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Stats {
    /// Taken from the backing so far.
    pub backing: usize,
    /// In live blocks, counted at their class or page-rounded size.
    pub live: usize,
    /// Highest `live` so far.
    pub peak_live: usize,
    /// In free spans.
    pub free_spans: usize,
    /// Calls to `Backing::grow` that succeeded.
    pub grows: usize,
}

#[repr(C)]
struct FreeBlock {
    next: *mut FreeBlock,
}

/// Header in the first bytes of a free span.
#[repr(C)]
struct Span {
    size: usize,
    next: *mut Span,
}

enum Fit {
    Small(usize),
    /// Bytes (page multiple) and alignment.
    Large(usize, usize),
}

fn fit(layout: Layout) -> Option<Fit> {
    let size = layout.size().max(layout.align()).max(1);
    if size <= MAX_SMALL {
        let rounded = size.next_power_of_two().max(1 << MIN_CLASS_SHIFT);
        return Some(Fit::Small(
            (rounded.trailing_zeros() - MIN_CLASS_SHIFT) as usize,
        ));
    }
    let bytes = layout.size().max(1).checked_next_multiple_of(PAGE_SIZE)?;
    Some(Fit::Large(bytes, layout.align().max(PAGE_SIZE)))
}

const fn class_size(class: usize) -> usize {
    1 << (class as u32 + MIN_CLASS_SHIFT)
}

/// The allocator. It holds raw pointers into memory only it uses.
pub struct Heap<B> {
    backing: B,
    classes: [*mut FreeBlock; CLASSES],
    spans: *mut Span,
    stats: Stats,
}

// SAFETY: every pointer in the heap refers to backing memory that the heap
// owns exclusively (`Backing`'s contract); moving the heap to another thread
// moves that ownership with it.
unsafe impl<B: Send> Send for Heap<B> {}

impl<B: Backing> Heap<B> {
    pub const fn new(backing: B) -> Heap<B> {
        Heap {
            backing,
            classes: [ptr::null_mut(); CLASSES],
            spans: ptr::null_mut(),
            stats: Stats {
                backing: 0,
                live: 0,
                peak_live: 0,
                free_spans: 0,
                grows: 0,
            },
        }
    }

    pub fn stats(&self) -> Stats {
        self.stats
    }

    fn add_live(&mut self, bytes: usize) {
        self.stats.live += bytes;
        self.stats.peak_live = self.stats.peak_live.max(self.stats.live);
    }

    /// A block for `layout`, or null if the backing is exhausted. The
    /// contents are unspecified.
    pub fn alloc(&mut self, layout: Layout) -> *mut u8 {
        match fit(layout) {
            None => ptr::null_mut(),
            Some(Fit::Small(class)) => {
                if self.classes[class].is_null() && !self.refill(class) {
                    return ptr::null_mut();
                }
                let block = self.classes[class];
                // SAFETY: a non-null list head is a free block of this class:
                // aligned, inside backing memory, and holding its link.
                self.classes[class] = unsafe { (*block).next };
                self.add_live(class_size(class));
                block.cast()
            }
            Some(Fit::Large(bytes, align)) => {
                let Some(addr) = self.take_span(bytes, align) else {
                    return ptr::null_mut();
                };
                self.add_live(bytes);
                addr as *mut u8
            }
        }
    }

    /// Returns a block.
    ///
    /// # Safety
    ///
    /// `block` came from [`Heap::alloc`] on this heap with this `layout` and
    /// has not been returned since.
    pub unsafe fn dealloc(&mut self, block: *mut u8, layout: Layout) {
        match fit(layout) {
            // `alloc` cannot have returned a block for a layout that overflows.
            None => {}
            Some(Fit::Small(class)) => {
                let free: *mut FreeBlock = block.cast();
                // SAFETY: by the caller's contract the block is ours, unused
                // and aligned to its class, at least 16 bytes.
                unsafe { (*free).next = self.classes[class] };
                self.classes[class] = free;
                self.stats.live -= class_size(class);
            }
            Some(Fit::Large(bytes, _)) => {
                self.give_span(block as usize, bytes);
                self.stats.live -= bytes;
            }
        }
    }

    fn refill(&mut self, class: usize) -> bool {
        let Some(chunk) = self.take_span(CHUNK_SIZE, PAGE_SIZE) else {
            return false;
        };
        let size = class_size(class);
        for offset in (0..CHUNK_SIZE).step_by(size) {
            let block = (chunk + offset) as *mut FreeBlock;
            // SAFETY: the chunk is CHUNK_SIZE bytes of free backing memory,
            // and offsets are multiples of the class size.
            unsafe { (*block).next = self.classes[class] };
            self.classes[class] = block;
        }
        true
    }

    /// Takes `bytes` at a multiple of `align` from the first span that fits,
    /// growing once if none does.
    fn take_span(&mut self, bytes: usize, align: usize) -> Option<usize> {
        if let Some(addr) = self.search_spans(bytes, align) {
            return Some(addr);
        }
        // The worst-case alignment gap is `align - PAGE_SIZE`.
        let want = bytes
            .checked_add(align - PAGE_SIZE)?
            .max(GROW_SIZE)
            .checked_next_multiple_of(PAGE_SIZE)?;
        let addr = self.backing.grow(want / PAGE_SIZE)?;
        self.stats.backing += want;
        self.stats.grows += 1;
        self.give_span(addr, want);
        self.search_spans(bytes, align)
    }

    fn search_spans(&mut self, bytes: usize, align: usize) -> Option<usize> {
        let mut prev: *mut Span = ptr::null_mut();
        let mut cur = self.spans;
        while !cur.is_null() {
            let start = cur as usize;
            // SAFETY: list members are free spans holding a valid header.
            let Span { size, next } = unsafe { cur.read() };
            let end = start + size;
            // Heap addresses can be near the top of the address space, so
            // the rounding itself can overflow: that span does not fit.
            let fitting = start
                .checked_next_multiple_of(align)
                .filter(|at| at.checked_add(bytes).is_some_and(|e| e <= end));
            if let Some(at) = fitting {
                self.unlink(prev, next);
                self.stats.free_spans -= size;
                if at > start {
                    self.give_span(start, at - start);
                }
                if at + bytes < end {
                    self.give_span(at + bytes, end - (at + bytes));
                }
                return Some(at);
            }
            prev = cur;
            cur = next;
        }
        None
    }

    fn unlink(&mut self, prev: *mut Span, next: *mut Span) {
        if prev.is_null() {
            self.spans = next;
        } else {
            // SAFETY: `prev` is a member of the list.
            unsafe { (*prev).next = next };
        }
    }

    /// Inserts a free span in address order, merging with its neighbours.
    fn give_span(&mut self, addr: usize, bytes: usize) {
        self.stats.free_spans += bytes;
        let mut prev: *mut Span = ptr::null_mut();
        let mut next = self.spans;
        while !next.is_null() && (next as usize) < addr {
            prev = next;
            // SAFETY: list members hold a valid header.
            next = unsafe { (*next).next };
        }
        let mut size = bytes;
        let mut after = next;
        if !next.is_null() && addr + bytes == next as usize {
            // SAFETY: as above.
            let n = unsafe { next.read() };
            size += n.size;
            after = n.next;
        }
        if !prev.is_null() {
            // SAFETY: as above.
            let p = unsafe { prev.read() };
            if prev as usize + p.size == addr {
                // SAFETY: as above; the span grows over memory now free.
                unsafe {
                    (*prev).size = p.size + size;
                    (*prev).next = after;
                }
                return;
            }
        }
        let span = addr as *mut Span;
        // SAFETY: the caller gave us `bytes` >= one page of free backing
        // memory at the page-aligned `addr`.
        unsafe { span.write(Span { size, next: after }) };
        self.unlink(prev, span);
    }
}
