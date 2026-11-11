//! Ports (ADR-0006) over `zoraya-events`, and the one lock over all signal
//! state (ADR-0041).

use alloc::sync::Arc;

use zoraya_abi::{Mode, Packet, Status};
use zoraya_events::{Error, Events, PortId, SourceId};
use zoraya_time::Deadline;

use crate::charge::{self, Paid};
use crate::job::Job;
use crate::sched::{self, Thread};
use crate::sync::SpinLock;
use crate::{arch, timer};

pub static EVENTS: SpinLock<Events<Arc<Thread>, Paid>> = SpinLock::new(Events::new());

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

/// The bits asserted on `source` now.
pub fn signals(source: SourceId) -> u32 {
    EVENTS.lock().signals(source)
}

pub fn clear_signals(source: SourceId, bits: u32) {
    EVENTS.lock().clear(source, bits);
}

pub struct Port {
    id: PortId,
    /// The creator pays for the port and, in steps, for the packets it holds.
    paid: SpinLock<Charged>,
}

/// What a port has paid for: `paid`, and how many ready-list slots of it
/// (`PACKET_STEP` at a time, never given back before the port ends).
struct Charged {
    paid: Paid,
    slots: usize,
}

/// What a port costs beside its record: its entry in the event tables, and a
/// first `PACKET_STEP` packets of 24 bytes. The ready list grows from there
/// as packets are queued, and `Port::queue` charges each step first.
const PORT_EXTRA: u64 = 128 + PACKET_STEP as u64 * 24;

/// Packets charged for at a time.
const PACKET_STEP: usize = 64;

impl Port {
    /// A port paid for by `payer` (`None`: the kernel's).
    pub fn new(payer: Option<&Arc<Job>>) -> Result<Arc<Port>, Status> {
        let mut paid = Paid::new(payer);
        paid.kernel(charge::arc_cost::<Port>() + PORT_EXTRA)?;
        let id = EVENTS.lock().new_port().map_err(status)?;
        // On failure the value was dropped, and its `Drop` freed the port and
        // gave the charge back.
        charge::prepaid_arc(Port {
            id,
            paid: SpinLock::new(Charged {
                paid,
                slots: PACKET_STEP,
            }),
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
        paid: Paid,
    ) -> Result<(), Status> {
        EVENTS
            .lock()
            .subscribe_paid(self.id, source, mask, key, mode, owner, paid)
            .map_err(status)
    }

    pub fn unsubscribe(&self, key: u64) -> Result<(), Status> {
        EVENTS.lock().unsubscribe(self.id, key).map_err(status)
    }

    pub fn queue(&self, key: u64, data: u64) -> Result<(), Status> {
        // The charge first, so that a refused one changes nothing. The ready
        // list is an upper bound of the user packets, which it also holds.
        {
            let mut charged = self.paid.lock();
            let needed = EVENTS.lock().ready_len(self.id) + 1;
            while charged.slots < needed {
                charged.paid.kernel(PACKET_STEP as u64 * 24)?;
                charged.slots += PACKET_STEP;
            }
        }
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
