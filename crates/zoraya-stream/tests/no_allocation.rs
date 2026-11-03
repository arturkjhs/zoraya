//! INV-0101-4, host half: once a ring and its storage exist, transfers
//! allocate nothing. Alone in its binary, since it replaces the allocator.

use std::alloc::{GlobalAlloc, Layout};

use zoraya_test_alloc::{Capped, LIMIT};

/// What this file's counting allocator forwards to: capped, as every host
/// test is (docs/backlog.md).
static INNER: Capped = Capped::new(LIMIT);
use std::sync::atomic::{AtomicUsize, Ordering};

use zoraya_stream::{ATOMIC, CAPACITY, Ring, Storage};

struct Counting;

static ALLOCATIONS: AtomicUsize = AtomicUsize::new(0);

// SAFETY: forwards to the capped system allocator.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        // SAFETY: the caller's contract.
        unsafe { INNER.alloc(layout) }
    }

    unsafe fn dealloc(&self, block: *mut u8, layout: Layout) {
        // SAFETY: the caller's contract.
        unsafe { INNER.dealloc(block, layout) }
    }
}

#[global_allocator]
static GLOBAL: Counting = Counting;

struct Flat(Box<[u8]>);

impl Storage for Flat {
    fn chunk(&self, offset: usize, length: usize) -> &[u8] {
        &self.0[offset..offset + length]
    }

    fn chunk_mut(&mut self, offset: usize, length: usize) -> &mut [u8] {
        &mut self.0[offset..offset + length]
    }
}

#[test]
fn transfers_allocate_nothing() {
    let mut storage = Flat(vec![0; CAPACITY].into_boxed_slice());
    let mut ring = Ring::new();
    let source = [7u8; 3 * ATOMIC];
    let mut target = [0u8; 3 * ATOMIC];
    let before = ALLOCATIONS.load(Ordering::Relaxed);
    let mut moved = 0;
    for round in 0..10_000usize {
        let size = 1 + round * 7919 % source.len();
        let _ = ring.write(&mut storage, size, |at, chunk: &mut [u8]| {
            chunk.copy_from_slice(&source[at..at + chunk.len()]);
            Ok::<(), ()>(())
        });
        let room = 1 + round * 104_729 % target.len();
        if let Ok(n) = ring.read(&storage, room, |at, chunk| {
            target[at..at + chunk.len()].copy_from_slice(chunk);
            Ok::<(), ()>(())
        }) {
            moved += n;
        }
        let _ = (ring.read_signals(), ring.write_signals());
    }
    ring.close_reader();
    ring.close_writer();
    assert_eq!(ALLOCATIONS.load(Ordering::Relaxed), before);
    assert!(moved > 0);
    assert!(target.iter().all(|&b| b == 7 || b == 0));
}
