//! INV-0041-3: asserting a signal allocates nothing. Alone in its binary,
//! since it replaces the allocator.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

use zoraya_abi::Mode;
use zoraya_events::Events;

struct Counting;

static ALLOCATIONS: AtomicUsize = AtomicUsize::new(0);

// SAFETY: forwards to the system allocator.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        // SAFETY: the caller's contract.
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, block: *mut u8, layout: Layout) {
        // SAFETY: the caller's contract.
        unsafe { System.dealloc(block, layout) }
    }
}

#[global_allocator]
static GLOBAL: Counting = Counting;

#[test]
fn asserting_a_signal_allocates_nothing() {
    let mut e: Events<u32> = Events::new();
    let port = e.new_port().unwrap();
    let source = e.new_source(0).unwrap();
    for key in 0..1000 {
        let mode = if key % 2 == 0 {
            Mode::Level
        } else {
            Mode::Edge
        };
        e.subscribe(port, source, 1, key, mode, 0).unwrap();
    }
    // User packets keep the room they found.
    for i in 0..1024 {
        e.queue(port, i, i, &mut |_| {}).unwrap();
        assert!(e.has_room(port), "{i}");
    }
    let mut drained = [zoraya_abi::Packet::default(); 2048];
    e.take(port, &mut drained).unwrap();
    let before = ALLOCATIONS.load(Ordering::Relaxed);
    let mut packets = [zoraya_abi::Packet::default(); 64];
    for _ in 0..100 {
        e.set(source, 1, &mut |_| {});
        e.clear(source, 1);
        e.set(source, 1, &mut |_| {});
        e.take(port, &mut packets).unwrap();
        e.clear(source, 1);
    }
    assert_eq!(ALLOCATIONS.load(Ordering::Relaxed), before);
}
