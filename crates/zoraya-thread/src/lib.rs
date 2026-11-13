//! The logic of threads in a process that needs no machine (ADR-0300,
//! ADR-0301): the bounded list of a process's threads with its two-phase end,
//! and the FIFO wait list keyed by address that `futex_wait` and `futex_wake`
//! use.
//!
//! Both are plain data. The kernel holds each under a spinlock of its own and
//! never allocates after the process is made: the lists are reserved to their
//! bounds at creation (`MAX_THREADS`), so a thread's creation, wait and end
//! cannot fail for lack of list space (ADR-0301 driver 3).
//!
//! Nothing here panics on any input and nothing allocates after `new`.

#![no_std]
#![forbid(unsafe_code)]

extern crate alloc;
#[cfg(test)]
extern crate std;

use alloc::collections::TryReserveError;
use alloc::vec::Vec;

/// Threads one process may have (ADR-0300 item 1): 128 stacks leave room in
/// the 256 mappings and the 1024 handle slots.
pub const MAX_THREADS: usize = 128;

/// Waiters or threads one wake step takes off a list before the lock is
/// dropped and a step point passes (ADR-0270, ADR-0301 item 7).
pub const WAKE_STEP: usize = 64;

/// Why a thread could not join a process.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JoinError {
    /// The process has `MAX_THREADS` threads.
    Full,
    /// The process's exit record is fixed: no thread may start (ADR-0300
    /// item 6).
    Ending,
}

/// What [`Members::leave`] found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Left {
    /// The thread was in the list.
    pub was_member: bool,
    /// Threads still in the list.
    pub remaining: usize,
    /// This leave emptied the list: the caller is the owner and runs the
    /// teardown (ADR-0300 item 6). True for exactly one leave of a process.
    pub owner: bool,
    /// The list was emptied by a process that was not ending: the end began
    /// with this leave (ADR-0300 item 3: the last thread's exit ends the
    /// process as `Exited` with code 0), and the caller fixes the record.
    pub began_end: bool,
}

/// The threads of one process, by id, in creation order, and whether the
/// process is ending.
///
/// The ending is in two phases (ADR-0300 item 6): the exit record is fixed
/// once ([`Members::begin_end`]); then every thread leaves, and the one whose
/// leave empties the list is the owner. After the first phase no thread joins.
#[derive(Debug, Clone)]
pub struct Members<T> {
    list: Vec<(u64, T)>,
    ending: bool,
    /// The owner was named: it is named once.
    owned: bool,
}

impl<T> Members<T> {
    /// A list with room for `MAX_THREADS`, or the allocation's failure.
    pub fn new() -> Result<Members<T>, TryReserveError> {
        let mut members = Members::new_small()?;
        members.reserve_all()?;
        Ok(members)
    }

    /// A list with room for the process's first thread only: a process that
    /// never makes a second pays for no more (`reserve_all` grows it, at the
    /// first `thread_create`).
    pub fn new_small() -> Result<Members<T>, TryReserveError> {
        let mut list = Vec::new();
        list.try_reserve_exact(1)?;
        Ok(Members {
            list,
            ending: false,
            owned: false,
        })
    }

    /// Makes room for `MAX_THREADS`, so that no join allocates. Idempotent.
    pub fn reserve_all(&mut self) -> Result<(), TryReserveError> {
        let more = MAX_THREADS.saturating_sub(self.list.len());
        self.list.try_reserve_exact(more)
    }

    /// Gives back the room `reserve_all` made, down to the live entries or
    /// one, whichever is more (a failed first `thread_create` undoes its
    /// growth). It builds the smaller list with `try_reserve_exact` and swaps
    /// it in, so a failed allocation is an `Err` that leaves the large list
    /// as it was (`Vec::shrink_to` would allocate through the infallible
    /// path, and the kernel allocator has no `realloc`; ADR-0022).
    pub fn shrink(&mut self) -> Result<(), TryReserveError> {
        let room = self.list.len().max(1);
        if self.list.capacity() <= room {
            return Ok(());
        }
        let mut smaller = Vec::new();
        smaller.try_reserve_exact(room)?;
        smaller.append(&mut self.list);
        self.list = smaller;
        Ok(())
    }

    /// Drops the list's memory: what a zombie keeps is its record, not the
    /// lists of its threads. For a list with no member left; the flags stay.
    pub fn release(&mut self) {
        debug_assert!(self.list.is_empty());
        self.list = Vec::new();
    }

    /// The room of the list, in entries (for tests of the charge).
    pub fn capacity(&self) -> usize {
        self.list.capacity()
    }

    /// The bytes of list space `new` reserves, for the charge.
    pub fn reserved_bytes() -> usize {
        MAX_THREADS * core::mem::size_of::<(u64, T)>()
    }

    pub fn len(&self) -> usize {
        self.list.len()
    }

    pub fn is_empty(&self) -> bool {
        self.list.is_empty()
    }

    pub fn is_ending(&self) -> bool {
        self.ending
    }

    /// Adds thread `id`. Nothing is allocated; a refusal changes nothing.
    pub fn join(&mut self, id: u64, thread: T) -> Result<(), JoinError> {
        if self.ending {
            return Err(JoinError::Ending);
        }
        if self.list.len() >= MAX_THREADS || self.list.len() >= self.list.capacity() {
            return Err(JoinError::Full);
        }
        self.list.push((id, thread));
        Ok(())
    }

    /// Whether a thread could join now (for the order of checks of
    /// `thread_create`, which refuses before it allocates).
    pub fn has_room(&self) -> Result<(), JoinError> {
        if self.ending {
            Err(JoinError::Ending)
        } else if self.list.len() >= MAX_THREADS {
            Err(JoinError::Full)
        } else {
            // Room in the list itself is made by `reserve_all`, which the
            // caller does before it joins a second thread.
            Ok(())
        }
    }

    /// Fixes the end. True for the first caller only.
    pub fn begin_end(&mut self) -> bool {
        !core::mem::replace(&mut self.ending, true)
    }

    /// Removes thread `id`.
    pub fn leave(&mut self, id: u64) -> Left {
        let was_member = match self.list.iter().position(|(i, _)| *i == id) {
            Some(at) => {
                self.list.remove(at);
                true
            }
            None => false,
        };
        let emptied = was_member && self.list.is_empty() && !self.owned;
        let began_end = emptied && self.begin_end();
        if emptied {
            self.owned = true;
        }
        Left {
            was_member,
            remaining: self.list.len(),
            owner: emptied,
            began_end,
        }
    }

    /// The first thread, the one an asker wakes (ADR-0300 item 6).
    pub fn first(&self) -> Option<&T> {
        self.list.first().map(|(_, t)| t)
    }

    /// Calls `f` on up to `max` threads starting at index `from`, and gives
    /// the index to go on from: a step of a walk that drops the lock between
    /// steps. `None` at the end.
    pub fn step<F: FnMut(&T)>(&self, from: usize, max: usize, mut f: F) -> Option<usize> {
        let end = self.list.len().min(from.saturating_add(max));
        for (_, t) in self.list.get(from..end).unwrap_or(&[]) {
            f(t);
        }
        (end < self.list.len()).then_some(end)
    }
}

/// A FIFO list of waiters, each with the key (an address) it waits on. The
/// capacity is fixed when it is made; a push past it is refused, never grows.
#[derive(Debug)]
pub struct WaitList<T> {
    nodes: Vec<(u64, T)>,
}

/// The list is full.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Full;

impl<T> WaitList<T> {
    /// A list with room for `capacity` waiters.
    pub fn new(capacity: usize) -> Result<WaitList<T>, TryReserveError> {
        let mut nodes = Vec::new();
        nodes.try_reserve_exact(capacity)?;
        Ok(WaitList { nodes })
    }

    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// Puts a waiter at the end. No allocation: `Full` if the room is used.
    pub fn push(&mut self, key: u64, waiter: T) -> Result<(), Full> {
        if self.nodes.len() >= self.nodes.capacity() {
            return Err(Full);
        }
        self.nodes.push((key, waiter));
        Ok(())
    }

    /// Takes off up to `max` waiters on `key`, oldest first, calling `f` on
    /// each, and says how many. One pass over the list (at most
    /// `MAX_THREADS` entries): a step of a wake.
    pub fn take<F: FnMut(T)>(&mut self, key: u64, max: usize, mut f: F) -> usize {
        let mut budget = max;
        let mut taken = 0;
        let due = self.nodes.extract_if(.., |(k, _)| {
            let wanted = budget > 0 && *k == key;
            if wanted {
                budget -= 1;
            }
            wanted
        });
        for (_, waiter) in due {
            taken += 1;
            f(waiter);
        }
        taken
    }

    /// Takes off the first waiter satisfying `matches`, if any.
    pub fn remove_where<F: FnMut(&T) -> bool>(&mut self, mut matches: F) -> Option<T> {
        let at = self.nodes.iter().position(|(_, t)| matches(t))?;
        Some(self.nodes.remove(at).1)
    }

    /// Takes the oldest waiter of any key.
    pub fn pop_front(&mut self) -> Option<T> {
        if self.nodes.is_empty() {
            None
        } else {
            Some(self.nodes.remove(0).1)
        }
    }

    /// Waiters on `key`.
    pub fn waiting_on(&self, key: u64) -> usize {
        self.nodes.iter().filter(|(k, _)| *k == key).count()
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
zoraya_test_alloc::cap_memory!();
