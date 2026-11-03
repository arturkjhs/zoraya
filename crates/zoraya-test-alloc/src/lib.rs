//! A global allocator for host tests with a ceiling on live bytes.
//!
//! A test that grows without bound, as a mutant that never consumes a queue
//! can, would otherwise take the host's memory: four of them once took 63 GB
//! and stopped the machine. With this allocator an allocation past the
//! ceiling returns null, Rust's allocation error handler aborts the test
//! process at once, and the test fails like any other. Every host-tested
//! crate under `crates/` installs it with [`cap_memory!`] in its unit tests
//! and in each integration test file.
//!
//! The ceiling counts live bytes, not address space: macOS ignores
//! `ulimit -v`, so the process has to keep its own count.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

/// Live bytes a test process may hold.
pub const LIMIT: usize = 512 << 20;

/// The system allocator with a ceiling on live bytes.
pub struct Capped {
    limit: usize,
    live: AtomicUsize,
}

impl Capped {
    pub const fn new(limit: usize) -> Capped {
        Capped {
            limit,
            live: AtomicUsize::new(0),
        }
    }

    /// Live bytes now.
    pub fn live(&self) -> usize {
        self.live.load(Ordering::Relaxed)
    }

    /// Counts `size` more bytes, or refuses them.
    fn take(&self, size: usize) -> bool {
        let before = self.live.fetch_add(size, Ordering::Relaxed);
        if before
            .checked_add(size)
            .is_none_or(|after| after > self.limit)
        {
            self.live.fetch_sub(size, Ordering::Relaxed);
            return false;
        }
        true
    }

    fn give(&self, size: usize) {
        self.live.fetch_sub(size, Ordering::Relaxed);
    }
}

// SAFETY: every call forwards to the system allocator with the caller's
// arguments; the count only decides whether to refuse first, and refusing is
// returning null, which `GlobalAlloc` allows.
unsafe impl GlobalAlloc for Capped {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if !self.take(layout.size()) {
            return core::ptr::null_mut();
        }
        // SAFETY: the caller's contract.
        let block = unsafe { System.alloc(layout) };
        if block.is_null() {
            self.give(layout.size());
        }
        block
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        if !self.take(layout.size()) {
            return core::ptr::null_mut();
        }
        // SAFETY: the caller's contract.
        let block = unsafe { System.alloc_zeroed(layout) };
        if block.is_null() {
            self.give(layout.size());
        }
        block
    }

    unsafe fn dealloc(&self, block: *mut u8, layout: Layout) {
        // SAFETY: the caller's contract.
        unsafe { System.dealloc(block, layout) };
        self.give(layout.size());
    }

    unsafe fn realloc(&self, block: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let old = layout.size();
        if new_size > old && !self.take(new_size - old) {
            return core::ptr::null_mut();
        }
        // SAFETY: the caller's contract.
        let moved = unsafe { System.realloc(block, layout, new_size) };
        if moved.is_null() {
            if new_size > old {
                self.give(new_size - old);
            }
        } else if new_size < old {
            self.give(old - new_size);
        }
        moved
    }
}

/// Installs a [`Capped`] allocator with [`LIMIT`] as the global allocator of
/// this test binary.
#[macro_export]
macro_rules! cap_memory {
    () => {
        #[global_allocator]
        static ZORAYA_TEST_ALLOCATOR: $crate::Capped = $crate::Capped::new($crate::LIMIT);
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_ceiling_refuses_and_the_count_comes_back() {
        let capped = Capped::new(1 << 20);
        let small = Layout::from_size_align(4096, 8).unwrap();
        let big = Layout::from_size_align(2 << 20, 8).unwrap();
        // SAFETY: layouts of non-zero size; each block is freed with its own.
        unsafe {
            let a = capped.alloc(small);
            assert!(!a.is_null());
            assert_eq!(capped.live(), 4096);
            assert!(capped.alloc(big).is_null(), "past the ceiling");
            assert_eq!(capped.live(), 4096, "a refusal counts nothing");
            let grown = capped.realloc(a, small, 8192);
            assert!(!grown.is_null());
            assert_eq!(capped.live(), 8192);
            let grown_layout = Layout::from_size_align(8192, 8).unwrap();
            assert!(capped.realloc(grown, grown_layout, 4 << 20).is_null());
            assert_eq!(capped.live(), 8192, "a refused growth keeps the block");
            let shrunk = capped.realloc(grown, grown_layout, 100);
            assert_eq!(capped.live(), 100);
            capped.dealloc(shrunk, Layout::from_size_align(100, 8).unwrap());
            assert_eq!(capped.live(), 0);
            let z = capped.alloc_zeroed(small);
            assert!(!z.is_null() && *z == 0);
            capped.dealloc(z, small);
            assert_eq!(capped.live(), 0);
        }
    }
}

#[cfg(test)]
cap_memory!();
