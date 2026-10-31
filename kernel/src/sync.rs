//! The kernel's lock.
//!
//! [`SpinLock`] masks IRQ and FIQ while held, so an interrupt handler on this
//! CPU can never find it taken by the code it interrupted, as long as handlers
//! also use it. With one CPU the lock can therefore only be found taken by a
//! bug, and spinning would hang silently, so that panics.
//!
//! For SMP: `lock` must spin with `wfe` instead of panicking, `Drop` needs
//! `sev`, and interrupts stay masked while spinning. The ordering already
//! suits SMP: acquire on taking, release on giving back.

use core::cell::UnsafeCell;
use core::ops::{Deref, DerefMut};
use core::sync::atomic::{AtomicBool, Ordering};

use crate::arch;

pub struct SpinLock<T> {
    locked: AtomicBool,
    value: UnsafeCell<T>,
}

// SAFETY: `lock` gives one holder at a time exclusive access to the value,
// which is what `T: Send` requires to be shared between CPUs and handlers.
unsafe impl<T: Send> Sync for SpinLock<T> {}

impl<T> SpinLock<T> {
    pub const fn new(value: T) -> SpinLock<T> {
        SpinLock {
            locked: AtomicBool::new(false),
            value: UnsafeCell::new(value),
        }
    }

    pub fn lock(&self) -> Guard<'_, T> {
        let daif = arch::save_and_mask_interrupts();
        assert!(
            self.locked
                .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
                .is_ok(),
            "lock taken twice on one CPU"
        );
        Guard { lock: self, daif }
    }
}

pub struct Guard<'a, T> {
    lock: &'a SpinLock<T>,
    daif: u64,
}

impl<T> Deref for Guard<'_, T> {
    type Target = T;

    fn deref(&self) -> &T {
        // SAFETY: the guard is the one holder of the lock.
        unsafe { &*self.lock.value.get() }
    }
}

impl<T> DerefMut for Guard<'_, T> {
    fn deref_mut(&mut self) -> &mut T {
        // SAFETY: the guard is the one holder of the lock, and `&mut self`
        // makes this the only reference through it.
        unsafe { &mut *self.lock.value.get() }
    }
}

impl<T> Drop for Guard<'_, T> {
    fn drop(&mut self) {
        self.lock.locked.store(false, Ordering::Release);
        arch::restore_interrupts(self.daif);
    }
}
