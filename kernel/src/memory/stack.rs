//! Kernel thread stacks in the stack window (ADR-0030): each is
//! [`THREAD_STACK_SIZE`] bytes with an unmapped guard page below it.

use alloc::vec::Vec;

use zoraya_pmm::FRAME_SIZE;

use super::heap::OutOfMemory;
use super::layout::{STACK_WINDOW, STACK_WINDOW_SIZE};
use super::{frames, paging};
use crate::arch::{FRAME_SIZE as EXCEPTION_FRAME, THREAD_STACK_SIZE, write_user_frame};
use crate::sync::SpinLock;

const SLOT: u64 = THREAD_STACK_SIZE as u64 + FRAME_SIZE;
const PAGES: u64 = THREAD_STACK_SIZE as u64 / FRAME_SIZE;

struct Slots {
    next: u64,
    free: Vec<u64>,
}

static SLOTS: SpinLock<Slots> = SpinLock::new(Slots {
    next: 0,
    free: Vec::new(),
});

/// A mapped stack. Dropping it unmaps it and frees its frames.
pub struct KernelStack {
    /// Lowest mapped address.
    bottom: u64,
}

impl KernelStack {
    pub fn new() -> Result<KernelStack, OutOfMemory> {
        let slot = {
            let mut slots = SLOTS.lock();
            // Room to give the slot back, so that dropping cannot fail.
            let total = slots.next as usize + 1;
            slots.free.try_reserve(total).map_err(|_| OutOfMemory)?;
            match slots.free.pop() {
                Some(slot) => slot,
                None => {
                    let slot = slots.next;
                    if (slot + 1) * SLOT > STACK_WINDOW_SIZE {
                        return Err(OutOfMemory);
                    }
                    slots.next += 1;
                    slot
                }
            }
        };
        // Slot layout: the guard page, then the stack.
        let bottom = STACK_WINDOW + slot * SLOT + FRAME_SIZE;
        for i in 0..PAGES {
            let va = bottom + i * FRAME_SIZE;
            let mapped = frames::alloc().map_err(|_| OutOfMemory).and_then(|f| {
                paging::map_page(va, f).map_err(|_| {
                    frames::free(f);
                    OutOfMemory
                })
            });
            if let Err(e) = mapped {
                for j in 0..i {
                    frames::free(paging::unmap_page(bottom + j * FRAME_SIZE));
                }
                SLOTS.lock().free.push(slot);
                return Err(e);
            }
        }
        Ok(KernelStack { bottom })
    }

    pub fn top(&self) -> u64 {
        self.bottom + THREAD_STACK_SIZE as u64
    }

    /// The value for `SP_EL0` while the kernel runs on this stack.
    pub fn limit(&self) -> u64 {
        self.bottom + EXCEPTION_FRAME as u64
    }

    /// Lowest mapped address.
    #[cfg(feature = "qemu-test")]
    pub fn bottom(&self) -> u64 {
        self.bottom
    }

    /// The program counter in the frame [`KernelStack::push_user_frame`] wrote.
    #[cfg(feature = "qemu-test")]
    pub fn first_pc(&self) -> u64 {
        let frame = (self.top() - EXCEPTION_FRAME as u64) as *const crate::arch::Frame;
        // SAFETY: a user thread's stack has its frame at the top, mapped for
        // as long as `self` lives. Only a thread that never ran leaves the
        // value meaningful; on another it is merely stale.
        unsafe { (*frame).elr }
    }

    /// Writes a copy of `from` with x0 and x1 replaced at the top of the stack,
    /// and returns its address.
    pub fn push_user_frame_copy(&self, from: &crate::arch::Frame, x0: u64, x1: u64) -> u64 {
        let frame = self.top() - EXCEPTION_FRAME as u64;
        // SAFETY: as in `push_user_frame`.
        unsafe { crate::arch::write_user_frame_copy(frame, from, x0, x1) };
        frame
    }

    /// Sets `x0` in the frame [`KernelStack::push_user_frame`] wrote: the
    /// argument of a thread `thread_create` made (ADR-0300 item 2).
    pub fn set_first_argument(&self, x0: u64) {
        let frame = (self.top() - EXCEPTION_FRAME as u64) as *mut crate::arch::Frame;
        // SAFETY: as in `push_user_frame`: the frame at the top of a mapped
        // stack that no thread has run on yet, so nothing else reads it.
        unsafe { (*frame).x[0] = x0 };
    }

    /// Writes the frame for a first return to EL0 at the top of the stack, and
    /// returns its address.
    pub fn push_user_frame(&self, pc: u64, sp: u64) -> u64 {
        let frame = self.top() - EXCEPTION_FRAME as u64;
        // SAFETY: the top of a mapped stack that no thread runs on yet, with
        // room for a frame, and 16-byte aligned.
        unsafe { write_user_frame(frame, pc, sp) };
        frame
    }
}

impl Drop for KernelStack {
    fn drop(&mut self) {
        for i in 0..PAGES {
            frames::free(paging::unmap_page(self.bottom + i * FRAME_SIZE));
        }
        let slot = (self.bottom - FRAME_SIZE - STACK_WINDOW) / SLOT;
        SLOTS.lock().free.push(slot);
    }
}
