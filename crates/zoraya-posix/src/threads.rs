//! The library's thread table (ADR-0302 item 4, ADR-0304 items 1 to 3): one
//! record per live thread with its id, the handle that `thread_interrupt`
//! needs, its signal mask and its thread-pending set. The signal state in
//! [`Signals`](crate::signals::Signals) always holds the *calling* thread's
//! mask and pending set: on each entry of the library the boundary calls
//! [`Posix::switch_to`], which saves the previous thread's and loads this
//! one's, so the whole of ADR-0106's code works on "the thread's" state
//! unchanged.

#![forbid(unsafe_code)]

use alloc::vec::Vec;

use crate::errno::{EINVAL, ESRCH};
use crate::process::Posix;
use crate::signals::{SIG_IGN, bit};
use crate::sys::Sys;
use zoraya_abi::posix::signal;

/// One thread.
pub struct ThreadRec<H> {
    /// The thread pointer (`TPIDR_EL0`): how a thread finds its record.
    pub tp: usize,
    /// musl's tid: 1 for the main thread, then 2, 3, ...
    pub tid: u32,
    /// For `thread_interrupt`; `None` where the kernel gave none.
    pub handle: Option<H>,
    /// The mask and thread-pending set while the thread is not the one whose
    /// state `Signals` holds.
    pub blocked: u64,
    pub pending: u64,
    /// The port the thread is waiting on, if it is at a wait point: an
    /// interrupt also queues a packet there (the kernel's `port_wait` is not
    /// woken by `thread_interrupt`).
    pub wait_port: Option<H>,
    /// An interrupt came (a cancel, or a signal sent to it) since the thread
    /// last looked.
    pub interrupted: bool,
}

pub struct Threads<H> {
    pub list: Vec<ThreadRec<H>>,
    /// The thread whose mask and pending set `Signals` holds.
    pub cur: usize,
}

impl<H> Default for Threads<H> {
    fn default() -> Self {
        Threads {
            list: Vec::new(),
            cur: 0,
        }
    }
}

/// What a `tkill` did.
#[derive(Debug, PartialEq, Eq)]
pub enum Tkill {
    /// The target is the caller: the boundary raises the signal itself.
    Myself,
    /// Recorded, and the target interrupted.
    Sent,
}

impl<S: Sys> Posix<S> {
    /// Makes `tp` the thread whose state `Signals` holds. The first thread to
    /// call is the main thread (id 1).
    pub fn switch_to(&mut self, tp: usize) {
        if self.threads.cur == tp && !self.threads.list.is_empty() {
            return;
        }
        if self.threads.list.is_empty() {
            let handle = self.sys.thread_self();
            self.threads.list.push(ThreadRec {
                tp,
                tid: 1,
                handle,
                blocked: self.signals.blocked,
                pending: self.signals.pending,
                wait_port: None,
                interrupted: false,
            });
            self.threads.cur = tp;
            return;
        }
        // The only thread, under a new pointer: the main thread, first seen
        // before `__init_tls` set its thread pointer (zero then).
        if self.threads.list.len() == 1 && self.threads.list[0].tp != tp {
            self.threads.list[0].tp = tp;
            self.threads.cur = tp;
            return;
        }
        let (blocked, pending) = (self.signals.blocked, self.signals.pending);
        let old = self.threads.cur;
        if let Some(r) = self.threads.list.iter_mut().find(|r| r.tp == old) {
            r.blocked = blocked;
            r.pending = pending;
        }
        match self.threads.list.iter().find(|r| r.tp == tp) {
            Some(r) => {
                self.signals.blocked = r.blocked;
                self.signals.pending = r.pending;
            }
            // A thread the table does not know starts with the mask in force
            // and nothing pending.
            None => self.signals.pending = 0,
        }
        self.threads.cur = tp;
    }

    /// A thread was made by the running one: it starts with the mask in force
    /// (POSIX; musl then sets its own saved one).
    pub fn thread_add(&mut self, tp: usize, tid: u32, handle: Option<S::H>) {
        let blocked = self.signals.blocked;
        self.threads.list.push(ThreadRec {
            tp,
            tid,
            handle,
            blocked,
            pending: 0,
            wait_port: None,
            interrupted: false,
        });
    }

    /// The thread is ending: its record, for the boundary to close the handle.
    pub fn thread_remove(&mut self, tp: usize) -> Option<ThreadRec<S::H>> {
        let i = self.threads.list.iter().position(|r| r.tp == tp)?;
        Some(self.threads.list.remove(i))
    }

    /// The number of live threads.
    pub fn thread_count(&self) -> usize {
        self.threads.list.len()
    }

    fn interrupt_at(&mut self, i: usize) {
        let r = &mut self.threads.list[i];
        r.interrupted = true;
        let (h, port) = (r.handle, r.wait_port);
        if let Some(h) = h {
            self.sys.interrupt(h, port);
        }
    }

    /// `tkill` (ADR-0304 item 2): signal `n` for thread `tid`; 0 only checks
    /// that it exists.
    pub fn tkill(&mut self, tid: u32, n: u32) -> Result<Tkill, i32> {
        if n > signal::MAX {
            return Err(EINVAL);
        }
        let Some(i) = self.threads.list.iter().position(|r| r.tid == tid) else {
            return Err(ESRCH);
        };
        if n == 0 {
            return Ok(Tkill::Sent);
        }
        if self.threads.list[i].tp == self.threads.cur {
            return Ok(Tkill::Myself);
        }
        if self.signals.action(n).handler != SIG_IGN {
            self.threads.list[i].pending |= bit(n);
        }
        self.interrupt_at(i);
        Ok(Tkill::Sent)
    }

    /// Interrupts thread `tid` without a signal (a cancel, ADR-0305).
    pub fn interrupt_tid(&mut self, tid: u32) -> Result<(), i32> {
        let Some(i) = self.threads.list.iter().position(|r| r.tid == tid) else {
            return Err(ESRCH);
        };
        if self.threads.list[i].tp != self.threads.cur {
            self.interrupt_at(i);
        }
        Ok(())
    }

    /// The thread is about to wait on `port` (`None`: it has stopped). True if
    /// it should not wait: something is deliverable to it already, or it was
    /// interrupted since it last looked.
    pub fn set_wait_port(&mut self, port: Option<S::H>) -> bool {
        let cur = self.threads.cur;
        let ready = (self.signals.pending | self.signals.shared) & !self.signals.blocked != 0;
        let Some(r) = self.threads.list.iter_mut().find(|r| r.tp == cur) else {
            return ready;
        };
        r.wait_port = port;
        let interrupted = core::mem::take(&mut r.interrupted);
        interrupted && port.is_some() || ready
    }

    /// Whether the calling thread was interrupted since it last looked; and
    /// forgets it.
    pub fn take_interrupted(&mut self) -> bool {
        let cur = self.threads.cur;
        self.threads
            .list
            .iter_mut()
            .find(|r| r.tp == cur)
            .is_some_and(|r| core::mem::take(&mut r.interrupted))
    }

    /// A process-directed signal the calling thread blocks (ADR-0304 item 3
    /// (b)): goes to the thread of lowest tid that has it unblocked and is at
    /// a wait point if there is one, else the lowest tid with it unblocked;
    /// else stays in the process's pending set (c).
    pub fn route(&mut self, n: u32) {
        let cur = self.threads.cur;
        self.signals.pending &= !bit(n);
        let pick = self
            .threads
            .list
            .iter()
            .enumerate()
            .filter(|(_, r)| r.tp != cur && r.blocked & bit(n) == 0)
            .min_by_key(|(_, r)| (r.wait_port.is_none(), r.tid))
            .map(|(i, _)| i);
        match pick {
            Some(i) => {
                self.threads.list[i].pending |= bit(n);
                self.interrupt_at(i);
            }
            None => self.signals.shared |= bit(n),
        }
    }
}
