//! A global allocator with a ceiling: an allocation that would take the
//! total past it fails, and the program aborts with Rust's allocation error.
//! The tool's binary and its unit tests use it, so that a bug or a mutant
//! (cargo-mutants) that allocates without bound stops at the ceiling instead
//! of exhausting the host's memory. The ceiling is far above what the tool
//! needs for the sysroot's `libc.a` (about 15 MB).

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

/// The ceiling of the tools: 1 GiB.
pub const LIMIT: usize = 1 << 30;

pub struct Capped {
    limit: usize,
    used: AtomicUsize,
}

impl Capped {
    pub const fn new(limit: usize) -> Capped {
        Capped {
            limit,
            used: AtomicUsize::new(0),
        }
    }

    /// Takes `size` bytes from the budget, or refuses.
    fn take(&self, size: usize) -> bool {
        let mut used = self.used.load(Ordering::Relaxed);
        loop {
            let Some(next) = used.checked_add(size).filter(|n| *n <= self.limit) else {
                return false;
            };
            match self
                .used
                .compare_exchange_weak(used, next, Ordering::Relaxed, Ordering::Relaxed)
            {
                Ok(_) => return true,
                Err(now) => used = now,
            }
        }
    }

    fn give(&self, size: usize) {
        self.used.fetch_sub(size, Ordering::Relaxed);
    }
}

// SAFETY: every method passes its arguments to `System`, which implements the
// trait's contract; the budget only decides whether to call it, and a refusal
// returns null, which the contract allows.
unsafe impl GlobalAlloc for Capped {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if !self.take(layout.size()) {
            return std::ptr::null_mut();
        }
        // SAFETY: the caller's layout is valid and non-zero-sized, as
        // `GlobalAlloc::alloc` requires of its caller.
        let p = unsafe { System.alloc(layout) };
        if p.is_null() {
            self.give(layout.size());
        }
        p
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: `ptr` came from this allocator, so from `System`, with
        // `layout`, as the caller of `dealloc` guarantees.
        unsafe { System.dealloc(ptr, layout) };
        self.give(layout.size());
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let old = layout.size();
        if new_size > old && !self.take(new_size - old) {
            return std::ptr::null_mut();
        }
        // SAFETY: `ptr` and `layout` are this allocator's (so `System`'s), and
        // `new_size` is valid, as the caller of `realloc` guarantees.
        let p = unsafe { System.realloc(ptr, layout, new_size) };
        if p.is_null() {
            if new_size > old {
                self.give(new_size - old);
            }
        } else if new_size < old {
            self.give(old - new_size);
        }
        p
    }
}

#[cfg(test)]
mod capped_tests {
    use super::*;

    #[test]
    fn the_ceiling_refuses_and_returns_what_is_freed() {
        let a = Capped::new(4096);
        let big = Layout::from_size_align(3000, 8).unwrap();
        // SAFETY: a valid non-zero layout; each block is freed with its layout.
        unsafe {
            let p = a.alloc(big);
            assert!(!p.is_null());
            assert!(a.alloc(big).is_null(), "over the ceiling");
            let p = a.realloc(p, big, 4000);
            assert!(!p.is_null());
            assert!(
                a.realloc(p, Layout::from_size_align(4000, 8).unwrap(), 5000)
                    .is_null()
            );
            let p = a.realloc(p, Layout::from_size_align(4000, 8).unwrap(), 100);
            assert!(!p.is_null());
            let q = a.alloc(big);
            assert!(!q.is_null(), "a shrink returns its bytes");
            a.dealloc(q, big);
            a.dealloc(p, Layout::from_size_align(100, 8).unwrap());
        }
        assert_eq!(a.used.load(Ordering::Relaxed), 0);
    }
}
