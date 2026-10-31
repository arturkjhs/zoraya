//! Interrupt dispatch (ADR-0024's table for IRQ).

use core::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

use crate::gic;

const NO_HANDLER: usize = 0;

/// Handlers by interrupt ID. A `fn()` as `usize`, so that handlers can be
/// read without a lock from interrupt context.
static HANDLERS: [AtomicUsize; gic::FIRST_SPECIAL as usize] =
    [const { AtomicUsize::new(NO_HANDLER) }; gic::FIRST_SPECIAL as usize];
static SPURIOUS: AtomicU64 = AtomicU64::new(0);

/// Sets the handler for `intid` and enables it. The handler runs with IRQ
/// masked and must leave the device no longer asserting the interrupt.
pub fn register(intid: u32, edge: bool, handler: fn()) {
    assert!(intid < gic::FIRST_SPECIAL, "bad interrupt ID {intid}");
    HANDLERS[intid as usize].store(handler as usize, Ordering::Release);
    gic::enable(intid, edge);
}

/// Disables `intid` and removes its handler.
#[cfg(feature = "qemu-test")]
pub fn unregister(intid: u32) {
    gic::disable(intid);
    HANDLERS[intid as usize].store(NO_HANDLER, Ordering::Release);
}

/// Spurious interrupts seen so far.
#[cfg(feature = "qemu-test")]
pub fn spurious_count() -> u64 {
    SPURIOUS.load(Ordering::Relaxed)
}

/// An interrupt arrived that nothing handles: the interrupt ID.
#[derive(Debug, Clone, Copy)]
pub struct Unexpected(pub u32);

/// Handles one interrupt, from the IRQ vector. An interrupt with no handler
/// is disabled and ended before it is returned, so that the caller's panic
/// leaves the GIC in a state where other interrupts still arrive.
pub fn dispatch() -> Result<(), Unexpected> {
    let intid = gic::acknowledge();
    if intid >= gic::FIRST_SPECIAL {
        SPURIOUS.fetch_add(1, Ordering::Relaxed);
        return Ok(());
    }
    let handler = HANDLERS[intid as usize].load(Ordering::Acquire);
    if handler == NO_HANDLER {
        gic::disable(intid);
        gic::end_of_interrupt(intid);
        return Err(Unexpected(intid));
    }
    // SAFETY: a non-zero entry was stored from a `fn()` by `register`.
    let handler: fn() = unsafe { core::mem::transmute(handler) };
    handler();
    gic::end_of_interrupt(intid);
    Ok(())
}
