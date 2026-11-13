//! The library's lock (ADR-0302 item 8): Drepper's three-state futex mutex
//! over a [`Wait`] trait, so that the algorithm is tested on the host against
//! a model of the kernel's wait on an address, and `ffi` puts `futex_wait`
//! and `futex_wake` behind the same trait.
//!
//! State 0 is free, 1 is held with no waiter, 2 is held and a waiter may
//! sleep. The fast path of both `lock` and `unlock` is one atomic operation
//! and no call at all (INV-0302-5).

#![forbid(unsafe_code)]

use core::sync::atomic::{AtomicU32, Ordering};

/// A wait on an address, as the kernel offers it (ADR-0301).
pub trait Wait {
    /// Blocks while `*word == expected`; may return early for no reason (the
    /// caller re-checks).
    fn wait(&self, word: &AtomicU32, expected: u32);
    /// Wakes one waiter on `word`, if any.
    fn wake(&self, word: &AtomicU32);
}

pub struct Lock {
    state: AtomicU32,
}

impl Lock {
    pub const fn new() -> Lock {
        Lock {
            state: AtomicU32::new(0),
        }
    }

    /// Takes the lock, waiting if another thread holds it.
    pub fn lock<W: Wait>(&self, w: &W) {
        if self
            .state
            .compare_exchange(0, 1, Ordering::Acquire, Ordering::Relaxed)
            .is_ok()
        {
            return;
        }
        // Held: say that someone waits, and sleep while it is held. Whoever
        // gets it from 0 keeps the "waiters" mark, which costs one spurious
        // wake at its unlock and never loses one.
        while self.state.swap(2, Ordering::Acquire) != 0 {
            w.wait(&self.state, 2);
        }
    }

    /// Takes the lock if it is free.
    pub fn try_lock(&self) -> bool {
        self.state
            .compare_exchange(0, 1, Ordering::Acquire, Ordering::Relaxed)
            .is_ok()
    }

    /// Releases the lock, which the caller holds.
    pub fn unlock<W: Wait>(&self, w: &W) {
        if self.state.swap(0, Ordering::Release) == 2 {
            w.wake(&self.state);
        }
    }

    /// Makes the lock free without waking anyone: a `fork` child, whose copy
    /// of the lock says "held" by a thread that does not exist in it
    /// (ADR-0306 item 5), and which has no waiter.
    pub fn reset(&self) {
        self.state.store(0, Ordering::Release);
    }

    /// True while held (for assertions and tests).
    pub fn is_locked(&self) -> bool {
        self.state.load(Ordering::Relaxed) != 0
    }
}

impl Default for Lock {
    fn default() -> Self {
        Lock::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;
    use std::sync::{Arc, Condvar, Mutex};
    use std::thread;

    /// The kernel's wait on an address, on the host: a mutex, a condition
    /// variable and a rule that a wake before the sleep is not lost because
    /// the word is checked under the mutex.
    struct Futex {
        m: Mutex<()>,
        c: Condvar,
        waits: AtomicUsize,
        wakes: AtomicUsize,
    }

    impl Futex {
        fn new() -> Futex {
            Futex {
                m: Mutex::new(()),
                c: Condvar::new(),
                waits: AtomicUsize::new(0),
                wakes: AtomicUsize::new(0),
            }
        }
    }

    impl Wait for Futex {
        fn wait(&self, word: &AtomicU32, expected: u32) {
            let g = self.m.lock().unwrap();
            if word.load(Ordering::SeqCst) == expected {
                self.waits.fetch_add(1, Ordering::Relaxed);
                drop(self.c.wait(g).unwrap());
            }
        }

        fn wake(&self, _word: &AtomicU32) {
            self.wakes.fetch_add(1, Ordering::Relaxed);
            let _g = self.m.lock().unwrap();
            self.c.notify_one();
        }
    }

    /// A wait that never sleeps: any early return is allowed.
    struct Spin;
    impl Wait for Spin {
        fn wait(&self, _: &AtomicU32, _: u32) {
            std::thread::yield_now();
        }
        fn wake(&self, _: &AtomicU32) {}
    }

    #[test]
    fn an_uncontended_lock_makes_no_wait_and_no_wake() {
        let f = Futex::new();
        let l = Lock::new();
        for _ in 0..1000 {
            l.lock(&f);
            assert!(l.is_locked());
            assert!(!l.try_lock());
            l.unlock(&f);
        }
        assert_eq!(f.waits.load(Ordering::Relaxed), 0);
        assert_eq!(f.wakes.load(Ordering::Relaxed), 0);
        assert!(l.try_lock());
        l.unlock(&f);
    }

    /// A non-atomic increment under the lock: two or more threads never in
    /// at once (INV-0302-3), with a real sleep and wake between them.
    fn exclusion<W: Wait + Send + Sync + 'static>(w: Arc<W>, threads: usize, rounds: usize) {
        let lock = Arc::new(Lock::new());
        let n = Arc::new(AtomicUsize::new(0));
        let inside = Arc::new(AtomicUsize::new(0));
        let handles: std::vec::Vec<_> = (0..threads)
            .map(|_| {
                let (w, lock, n, inside) = (w.clone(), lock.clone(), n.clone(), inside.clone());
                thread::spawn(move || {
                    for _ in 0..rounds {
                        lock.lock(&*w);
                        assert_eq!(inside.fetch_add(1, Ordering::SeqCst), 0, "two inside");
                        // A load and a store: not atomic as a pair, so a lost
                        // update shows if the lock lets two in.
                        n.store(n.load(Ordering::SeqCst) + 1, Ordering::SeqCst);
                        inside.fetch_sub(1, Ordering::SeqCst);
                        lock.unlock(&*w);
                    }
                })
            })
            .collect();
        for h in handles {
            h.join().unwrap();
        }
        assert_eq!(n.load(Ordering::SeqCst), threads * rounds);
        assert!(!lock.is_locked());
    }

    #[test]
    fn two_to_four_threads_are_never_in_at_once() {
        for threads in 2..=4 {
            exclusion(Arc::new(Futex::new()), threads, 3000);
        }
    }

    #[test]
    fn spurious_returns_from_a_wait_lose_nothing() {
        exclusion(Arc::new(Spin), 4, 2000);
    }

    #[test]
    fn a_waiter_is_woken_by_the_unlock() {
        let f = Arc::new(Futex::new());
        let l = Arc::new(Lock::new());
        l.lock(&*f);
        let (f2, l2) = (f.clone(), l.clone());
        let t = thread::spawn(move || {
            l2.lock(&*f2);
            l2.unlock(&*f2);
        });
        // Let the other thread reach the wait.
        while f.waits.load(Ordering::Relaxed) == 0 {
            thread::yield_now();
        }
        l.unlock(&*f);
        t.join().unwrap();
        assert!(f.wakes.load(Ordering::Relaxed) >= 1);
    }
}
