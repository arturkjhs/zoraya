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
use zoraya_abi::Disposition;
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
    /// The signals the thread waits for in `sigwait`/`sigtimedwait` (blocked
    /// on purpose): a process-directed one is routed to it as to a thread
    /// that has it unblocked.
    pub sigwait: u64,
    /// `pthread_setname_np`: up to 15 bytes and a NUL.
    pub name: [u8; 16],
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
                sigwait: 0,
                name: [0; 16],
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
            sigwait: 0,
            name: [0; 16],
        });
    }

    /// The thread is ending: its record, for the boundary to close the handle.
    pub fn thread_remove(&mut self, tp: usize) -> Option<ThreadRec<S::H>> {
        let i = self.threads.list.iter().position(|r| r.tp == tp)?;
        Some(self.threads.list.remove(i))
    }

    /// A `fork` child (ADR-0306 item 5): the table keeps the calling thread
    /// only, under a new handle (the copied one names nothing in the child:
    /// thread objects are not duplicable), with no wait port, nothing
    /// interrupted; the process-pending set is empty. Returns the number of
    /// records dropped. The caller's mask and thread-pending set are in
    /// `Signals`, as always.
    pub fn threads_after_fork(&mut self) -> usize {
        let cur = self.threads.cur;
        let before = self.threads.list.len();
        self.threads.list.retain(|r| r.tp == cur);
        let handle = self.sys.thread_self();
        if let Some(r) = self.threads.list.first_mut() {
            r.handle = handle;
            r.wait_port = None;
            r.interrupted = false;
            r.pending = 0;
        }
        self.signals.shared = 0;
        before - self.threads.list.len()
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
            // Only marked: musl's `pthread_kill` and `raise` hold locks (and
            // block every signal) around the call, and `__restore_sigs`
            // delivers it after (a handler must not run inside them).
            if self.signals.action(n).handler != SIG_IGN {
                self.signals.pending |= bit(n);
            }
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

    /// `pthread_setname_np` (musl: `ERANGE` over 15 bytes) for thread `tid`.
    pub fn set_thread_name(&mut self, tid: u32, name: &[u8]) -> Result<(), i32> {
        if name.len() > 15 {
            return Err(crate::errno::ERANGE);
        }
        let r = self
            .threads
            .list
            .iter_mut()
            .find(|r| r.tid == tid)
            .ok_or(ESRCH)?;
        r.name = [0; 16];
        r.name[..name.len()].copy_from_slice(name);
        Ok(())
    }

    /// `pthread_getname_np`: the name of thread `tid`, NUL padded.
    pub fn thread_name(&self, tid: u32) -> Result<[u8; 16], i32> {
        self.threads
            .list
            .iter()
            .find(|r| r.tid == tid)
            .map(|r| r.name)
            .ok_or(ESRCH)
    }

    /// The calling thread waits for `set` (0: it no longer does).
    pub fn set_sigwait(&mut self, set: u64) {
        let cur = self.threads.cur;
        if let Some(r) = self.threads.list.iter_mut().find(|r| r.tp == cur) {
            r.sigwait = set;
        }
    }

    /// Whether the calling thread is in a `sigwait`.
    pub fn sigwait_active(&self) -> bool {
        let cur = self.threads.cur;
        self.threads
            .list
            .iter()
            .any(|r| r.tp == cur && r.sigwait != 0)
    }

    /// The signals some thread waits for in a `sigwait`.
    pub fn sigwait_union(&self) -> u64 {
        self.threads.list.iter().fold(0, |a, r| a | r.sigwait)
    }

    /// The kernel disposition for signal `n`: its action's (ADR-0106 item 1),
    /// except that a signal with the default action which a thread waits for
    /// in `sigwait` is delivered to the library (ADR-0315 item 10, a deviation
    /// from ADR-0106 item 2): the default would end the process, or the
    /// kernel drop it, though the set is blocked and a thread waits for it.
    pub fn effective_disposition(&self, n: u32) -> Disposition {
        if self.signals.action(n).handler == crate::signals::SIG_DFL
            && self.sigwait_union() & bit(n) != 0
            && !zoraya_abi::posix::signal::is_stop_or_continue(n)
        {
            Disposition::Deliver
        } else {
            self.signals.kernel_disposition(n)
        }
    }

    /// `sigwait` begins: the calling thread waits for `set`, which is blocked
    /// meanwhile; the old mask is returned for [`Posix::sigwait_end`].
    pub fn sigwait_begin(&mut self, set: u64) -> Result<u64, i32> {
        self.set_sigwait(set);
        self.signals.mask(crate::signals::SIG_BLOCK, Some(set))
    }

    /// `sigwait` ends, on every path: the wait is over and the mask is back.
    pub fn sigwait_end(&mut self, old: u64) {
        self.set_sigwait(0);
        let _ = self.signals.mask(crate::signals::SIG_SETMASK, Some(old));
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
    /// (b), (c)): it stays in the process's pending set (`shared`), which
    /// every thread that has it unblocked can take from; and the thread of
    /// lowest tid that has it unblocked and is at a wait point, else the
    /// lowest tid with it unblocked, is interrupted to look. `was_pending`:
    /// the calling thread had the signal pending before `Signals::take`
    /// added it (a `pthread_kill` of this thread): the two coalesce and the
    /// thread-directed one is not touched.
    pub fn route(&mut self, n: u32, was_pending: bool) {
        if was_pending {
            return;
        }
        let cur = self.threads.cur;
        self.signals.pending &= !bit(n);
        self.signals.shared |= bit(n);
        let pick = self
            .threads
            .list
            .iter()
            .enumerate()
            .filter(|(_, r)| r.tp != cur && (r.blocked & bit(n) == 0 || r.sigwait & bit(n) != 0))
            .min_by_key(|(_, r)| (r.wait_port.is_none(), r.tid))
            .map(|(i, _)| i);
        if let Some(i) = pick {
            self.interrupt_at(i);
        }
    }
}
