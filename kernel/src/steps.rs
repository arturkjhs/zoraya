//! Bounded kernel work (ADR-0270): a long call proceeds in steps of at most
//! [`STEP`] pages or objects and calls [`between`] after each, holding no
//! spinlock. The kernel stays non-preemptive (ADR-0030): a call yields only
//! at these points, as a blocking call does.
//!
//! The test kernel also keeps a counter of the longest time the CPU stayed
//! with IRQ masked (INV-0270-1), which `masked_max_ns` reads.

use zoraya_abi::Status;

use crate::sched;

/// Pages, page-table entries' worth of range or objects in one step
/// (ADR-0270 point 2, `[assumption]` until measured).
pub const STEP: usize = 64;

/// Units of work (frames, messages, objects, mappings) done since the last
/// step point of a teardown that spans many objects (see [`before`]).
static PENDING: core::sync::atomic::AtomicUsize = core::sync::atomic::AtomicUsize::new(0);

/// Announces `units` of work (at most [`STEP`]) that the caller is about to
/// do, and pauses first if the work since the last pause plus these would
/// pass a step. A teardown of many small objects (a handle table, a list of
/// mappings, a queue of messages) calls it once per object, so that the
/// bound counts the work of the whole exit and not of each object
/// (INV-0270-1). Like [`pause`], with no spinlock held and never failing.
pub fn before(units: usize) {
    use core::sync::atomic::Ordering;
    let units = units.min(STEP);
    if PENDING.load(Ordering::Relaxed) + units > STEP {
        PENDING.store(0, Ordering::Relaxed);
        pause();
    }
    PENDING.fetch_add(units, Ordering::Relaxed);
}

/// A pause that also starts the count again: the end of a teardown's first
/// part, before a part that may not yield (an exit after its table is off),
/// so that the part left is counted from nothing and not from the units the
/// first part had gathered since its last pause.
pub fn flush() {
    PENDING.store(0, core::sync::atomic::Ordering::Relaxed);
    pause();
}

/// As [`before`] for one object that costs one unit besides the frames it
/// frees (a handle, a mapping, a message), which counts it too.
pub fn object() {
    before(1);
    count(1);
}

/// Counts `units` of work done with IRQ masked, for the test kernel's check
/// that no interval between two step points held more than a step of work,
/// on any host (INV-0270-1's machine-independent half).
#[inline(always)]
#[track_caller]
pub fn count(units: usize) {
    #[cfg(feature = "qemu-test")]
    masked::count(units);
    #[cfg(not(feature = "qemu-test"))]
    let _ = units;
}

/// The end of a step that frees or undoes: lets a pending interrupt run and
/// switches away if the slice ended (ADR-0270 point 2). Nothing fails: a
/// teardown goes on to its end whatever the process was asked. A caller that
/// is not a user process (boot code, kernel threads) is not in a system call
/// and does nothing.
///
/// No spinlock may be held, because an interrupt handler that takes the same
/// lock would then panic. That is the caller's duty in every build; only the
/// test kernel checks it (it counts the locks held, `sync::held`), so a path
/// that no test reaches with a lock held is not caught (ADR-0270, open
/// questions).
pub fn pause() {
    #[cfg(feature = "qemu-test")]
    let kernel_steps = KERNEL_STEPS.load(core::sync::atomic::Ordering::Relaxed);
    #[cfg(not(feature = "qemu-test"))]
    let kernel_steps = false;
    if sched::current().process().is_none() && !kernel_steps {
        return;
    }
    #[cfg(feature = "qemu-test")]
    assert_eq!(
        crate::sync::held(),
        0,
        "a spinlock is held across a step (ADR-0270 point 2)"
    );
    #[cfg(feature = "qemu-test")]
    if ALWAYS_SWITCH.load(core::sync::atomic::Ordering::Relaxed) {
        crate::sched::request_resched();
    }
    // SAFETY: only a user process's system call gets here, which runs after
    // the vectors and the interrupt controller are set up, and the caller
    // holds no spinlock (its contract, checked above in the test kernel).
    unsafe { step_point(sched::preempt_point) };
}

/// Whether every [`pause`] asks the scheduler to switch (the test kernel's
/// `FORCE_SWITCH` call), so that walks and teardowns of different processes
/// interleave at each step point, whatever the slice. For tests of
/// interleaving only; one global flag, which the test that sets it clears.
#[cfg(feature = "qemu-test")]
static ALWAYS_SWITCH: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);

/// Sets [`ALWAYS_SWITCH`].
#[cfg(feature = "qemu-test")]
pub fn set_always_switch(on: bool) {
    ALWAYS_SWITCH.store(on, core::sync::atomic::Ordering::Relaxed);
}

/// Whether [`pause`] makes a step point in a kernel thread too: a test
/// that runs a call of the kernel's with IRQ masked, as a system call does.
#[cfg(feature = "qemu-test")]
static KERNEL_STEPS: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);

/// Sets [`KERNEL_STEPS`]. It is one global flag, for the test kernel only,
/// valid while those tests are single-actor.
#[cfg(feature = "qemu-test")]
pub fn set_kernel_steps(on: bool) {
    KERNEL_STEPS.store(on, core::sync::atomic::Ordering::Relaxed);
}

/// A window for interrupts, then `preempt` (the scheduler's preemption
/// point, which says whether it switched away), and a second window if it
/// did. The scheduler switches with IRQ masked, so a thread that resumes here
/// resumes inside the interval of the threads that ran meanwhile: without
/// the second window its next step (the tail of an exit, which cannot yield)
/// was added to theirs, two exits' tails in one interval, 110 to 116 units
/// against 80 on slow hosts (`t_cpu`, CI; INV-0270-1).
///
/// # Safety
///
/// As for [`crate::arch::unmask_interrupts`]: the vectors and the interrupt
/// controller are set up and the caller holds no spinlock.
pub(crate) unsafe fn step_point(preempt: impl FnOnce() -> bool) {
    // SAFETY: the caller's contract.
    unsafe { window() };
    if preempt() {
        // SAFETY: as above; the switch back holds no lock of this thread.
        unsafe { window() };
    }
}

/// Lets a pending interrupt run, and starts a new masked interval.
///
/// # Safety
///
/// As for [`step_point`].
unsafe fn window() {
    // SAFETY: the caller's contract. Interrupts are masked again below,
    // before anything that expects them masked.
    unsafe { crate::arch::unmask_interrupts() };
    // SAFETY: `isb` only synchronises the instruction stream, so that an
    // interrupt that is pending is taken here and not later.
    unsafe { core::arch::asm!("isb", options(nomem, nostack, preserves_flags)) };
    crate::arch::mask_interrupts();
    mask_began();
}

/// The end of a step of a call that builds something: as [`pause`], and fails
/// with `Cancelled` if the process was asked to end meanwhile (`job_kill`),
/// so that the call frees its partial work and returns (ADR-0270 point 3).
pub fn between() -> Result<(), Status> {
    pause();
    let thread = sched::current();
    match thread.process() {
        Some(process) if process.exit_requested() => {
            #[cfg(feature = "qemu-test")]
            CANCELLED.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
            Err(Status::Cancelled)
        }
        _ => Ok(()),
    }
}

/// How many times [`between`] ended a call with `Cancelled` (INV-0270-2's
/// test shows that a kill landed between two steps).
#[cfg(feature = "qemu-test")]
static CANCELLED: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);

/// The number of calls [`between`] cancelled so far.
#[cfg(feature = "qemu-test")]
pub fn cancelled() -> u64 {
    CANCELLED.load(core::sync::atomic::Ordering::Relaxed)
}

/// Counts the longest time with IRQ masked, in the test kernel.
#[cfg(feature = "qemu-test")]
mod masked {
    use core::sync::atomic::{AtomicU64, Ordering};

    /// Counter ticks when the mask began, or 0 while unmasked.
    static SINCE: AtomicU64 = AtomicU64::new(0);
    static MAX_TICKS: AtomicU64 = AtomicU64::new(0);

    fn ticks() -> u64 {
        let t: u64;
        // SAFETY: reading the virtual counter has no side effects.
        unsafe {
            core::arch::asm!("isb", "mrs {}, cntvct_el0", out(reg) t,
                options(nomem, nostack, preserves_flags))
        };
        t.max(1)
    }

    /// Units of work counted in the interval now running, and the most in
    /// any interval since the last reset.
    static UNITS: AtomicU64 = AtomicU64::new(0);
    static MAX_UNITS: AtomicU64 = AtomicU64::new(0);

    /// Where the units of the interval now running came from (the line of
    /// the call of `steps::count`), and the same for the interval with the
    /// most units so far: the test kernel's diagnosis when a bound fails.
    const SLOTS: usize = 12;
    static CUR: [[AtomicU64; 2]; SLOTS] = [const { [AtomicU64::new(0), AtomicU64::new(0)] }; SLOTS];
    static BEST: [[AtomicU64; 2]; SLOTS] =
        [const { [AtomicU64::new(0), AtomicU64::new(0)] }; SLOTS];

    #[track_caller]
    pub fn count(units: usize) {
        UNITS.fetch_add(units as u64, Ordering::Relaxed);
        let at = core::panic::Location::caller();
        let key = ((at.file().len() as u64) << 32) | u64::from(at.line());
        for slot in &CUR {
            let have = slot[0].load(Ordering::Relaxed);
            if have == key || have == 0 {
                slot[0].store(key, Ordering::Relaxed);
                slot[1].fetch_add(units as u64, Ordering::Relaxed);
                break;
            }
        }
    }

    /// Prints where the units of the busiest interval came from.
    pub fn diagnose() {
        crate::println!("steps: the busiest interval, units by call site (file length, line):");
        for slot in &BEST {
            let key = slot[0].load(Ordering::Relaxed);
            if key != 0 {
                crate::println!(
                    "steps:   {}:{} {}",
                    key >> 32,
                    key & 0xffff_ffff,
                    slot[1].load(Ordering::Relaxed)
                );
            }
        }
    }

    pub fn begin() {
        if SINCE.load(Ordering::Relaxed) == 0 {
            SINCE.store(ticks(), Ordering::Relaxed);
            UNITS.store(0, Ordering::Relaxed);
            for slot in &CUR {
                slot[0].store(0, Ordering::Relaxed);
                slot[1].store(0, Ordering::Relaxed);
            }
        }
    }

    pub fn end() {
        let since = SINCE.swap(0, Ordering::Relaxed);
        if since != 0 {
            MAX_TICKS.fetch_max(ticks().saturating_sub(since), Ordering::Relaxed);
            let units = UNITS.swap(0, Ordering::Relaxed);
            if units > MAX_UNITS.fetch_max(units, Ordering::Relaxed) {
                for (best, cur) in BEST.iter().zip(&CUR) {
                    best[0].store(cur[0].load(Ordering::Relaxed), Ordering::Relaxed);
                    best[1].store(cur[1].load(Ordering::Relaxed), Ordering::Relaxed);
                }
            }
        }
    }

    pub fn max_units(reset: bool) -> u64 {
        if reset {
            MAX_UNITS.swap(0, Ordering::Relaxed)
        } else {
            MAX_UNITS.load(Ordering::Relaxed)
        }
    }

    pub fn max_ns(reset: bool) -> u64 {
        let freq: u64;
        // SAFETY: reading the counter frequency has no side effects.
        unsafe {
            core::arch::asm!("mrs {}, cntfrq_el0", out(reg) freq,
                options(nomem, nostack, preserves_flags))
        };
        let t = if reset {
            MAX_TICKS.swap(0, Ordering::Relaxed)
        } else {
            MAX_TICKS.load(Ordering::Relaxed)
        };
        (u128::from(t) * 1_000_000_000 / u128::from(freq.max(1))) as u64
    }
}

/// The mask began (the CPU took an exception or code masked IRQ).
#[inline(always)]
pub fn mask_began() {
    #[cfg(feature = "qemu-test")]
    masked::begin();
}

/// The mask ended: IRQ is on again.
#[inline(always)]
pub fn mask_ended() {
    #[cfg(feature = "qemu-test")]
    masked::end();
}

/// Prints where the units of the busiest masked interval came from.
#[cfg(feature = "qemu-test")]
pub fn diagnose_units() {
    masked::diagnose();
}

/// The longest time with IRQ masked since the last reset, in nanoseconds.
#[cfg(feature = "qemu-test")]
pub fn masked_max_ns(reset: bool) -> u64 {
    masked::max_ns(reset)
}

/// The most units of work in one interval with IRQ masked since the last
/// reset (INV-0270-1, the half that does not depend on the host's speed).
#[cfg(feature = "qemu-test")]
pub fn masked_max_units(reset: bool) -> u64 {
    masked::max_units(reset)
}
