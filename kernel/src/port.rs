//! Ports (ADR-0006) over `zoraya-events`, and the one lock over all signal
//! state (ADR-0041).

use alloc::sync::Arc;

use zoraya_abi::{Mode, Packet, Status};
use zoraya_events::{Error, Events, PortId, SourceId};
use zoraya_time::Deadline;

use crate::memory::heap::try_arc;
use crate::sched::{self, Thread};
use crate::sync::SpinLock;
use crate::{arch, timer};

pub static EVENTS: SpinLock<Events<Arc<Thread>>> = SpinLock::new(Events::new());

fn status(e: Error) -> Status {
    match e {
        Error::NoMemory => Status::NoMemory,
        Error::Closed => Status::BadState,
        Error::KeyInUse => Status::InvalidArgument,
        Error::NoSuchKey => Status::NotFound,
        Error::Full => Status::LimitReached,
    }
}

/// A new source, with `bits` asserted.
pub fn new_source(bits: u32) -> Result<SourceId, Status> {
    EVENTS.lock().new_source(bits).map_err(status)
}

pub fn free_source(source: SourceId) {
    EVENTS.lock().free_source(source);
}

pub fn set_signals(source: SourceId, bits: u32) {
    EVENTS.lock().set(source, bits, &mut sched::wake);
}

pub fn clear_signals(source: SourceId, bits: u32) {
    EVENTS.lock().clear(source, bits);
}

pub struct Port {
    id: PortId,
}

impl Port {
    pub fn new() -> Result<Arc<Port>, Status> {
        let id = EVENTS.lock().new_port().map_err(status)?;
        try_arc(Port { id }).map_err(|_| {
            EVENTS.lock().free_port(id, &mut sched::wake);
            Status::NoMemory
        })
    }

    /// Ends all waits and subscriptions (ADR-0006 rule 5).
    pub fn close(&self) {
        EVENTS.lock().close_port(self.id, &mut sched::wake);
    }

    pub fn subscribe(
        &self,
        source: SourceId,
        mask: u32,
        key: u64,
        mode: Mode,
        owner: u64,
    ) -> Result<(), Status> {
        EVENTS
            .lock()
            .subscribe(self.id, source, mask, key, mode, owner)
            .map_err(status)
    }

    pub fn unsubscribe(&self, key: u64) -> Result<(), Status> {
        EVENTS.lock().unsubscribe(self.id, key).map_err(status)
    }

    pub fn queue(&self, key: u64, data: u64) -> Result<(), Status> {
        EVENTS
            .lock()
            .queue(self.id, key, data, &mut sched::wake)
            .map_err(status)
    }

    /// Blocks for at least one packet. Only a packet, the deadline, the
    /// port's closing, or the process ending returns from this (INV-0006-6).
    pub fn wait(&self, deadline: Deadline, out: &mut [Packet]) -> Result<usize, Status> {
        let daif = arch::save_and_mask_interrupts();
        let result = self.wait_masked(deadline, out);
        arch::restore_interrupts(daif);
        result
    }

    fn wait_masked(&self, deadline: Deadline, out: &mut [Packet]) -> Result<usize, Status> {
        let current = sched::current();
        loop {
            match EVENTS.lock().take(self.id, out) {
                Err(_) => return Err(Status::Cancelled),
                Ok(0) => {}
                Ok(n) => return Ok(n),
            }
            if current.process().is_some_and(|p| p.exit_requested()) {
                return Err(Status::Cancelled);
            }
            if let Deadline::At(at) = deadline
                && timer::now() >= at
            {
                return Err(Status::DeadlineExceeded);
            }
            EVENTS
                .lock()
                .add_waiter(self.id, current.clone())
                .map_err(|_| Status::NoMemory)?;
            let blocked = sched::block_until(deadline);
            let listed = EVENTS
                .lock()
                .remove_waiter(self.id, |t| Arc::ptr_eq(t, &current));
            drop(listed);
            blocked.map_err(|_| Status::NoMemory)?;
        }
    }
}

impl Drop for Port {
    fn drop(&mut self) {
        EVENTS.lock().free_port(self.id, &mut sched::wake);
    }
}
