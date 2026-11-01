//! What a handle can hold (ADR-0040, ADR-0041).

use alloc::sync::Arc;
use core::sync::atomic::{AtomicU64, Ordering};

use zoraya_abi::{ObjectType, Waiting};
use zoraya_events::SourceId;

use crate::job::Job;
use crate::memory::object::MemoryObject;
use crate::port::{self, Port};
use crate::process::Process;

pub type HandleTable = zoraya_handle::Table<Object>;
pub type Entry = zoraya_handle::Entry<Object>;

/// A program in the ramdisk.
#[derive(Clone, Copy)]
pub struct Image {
    pub name: &'static str,
    pub file: &'static [u8],
}

#[derive(Clone)]
pub enum Object {
    Console,
    // Only the normal kernel starts a first process, which holds this.
    #[cfg_attr(feature = "qemu-test", allow(dead_code))]
    Power,
    Directory,
    Image(Image),
    Process(Arc<Process>),
    Job(Arc<Job>),
    Port(Arc<Port>),
    Memory(Arc<MemoryObject>),
}

impl Object {
    /// Where its signals are, and which bits exist: the kernel's side of
    /// ADR-0006 rule 1.
    pub fn signals(&self) -> Option<(SourceId, u32)> {
        let (source, waiting) = match self {
            Object::Process(p) => (p.source(), zoraya_abi::ProcessObject::WAITING),
            Object::Job(j) => (j.source(), zoraya_abi::JobObject::WAITING),
            Object::Console => (
                crate::console::source()?,
                zoraya_abi::ConsoleObject::WAITING,
            ),
            Object::Power
            | Object::Directory
            | Object::Image(_)
            | Object::Port(_)
            | Object::Memory(_) => {
                return None;
            }
        };
        match waiting {
            Waiting::Signals(bits) => Some((source, bits)),
            Waiting::NotWaitable => None,
        }
    }
}

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

/// A new handle's id, unique for the kernel's life.
pub fn next_id() -> u64 {
    NEXT_ID.fetch_add(1, Ordering::Relaxed)
}

/// What taking a handle out of a table means for its subscriptions, which
/// belong to it there. A moved handle ends them too (ADR-0040).
pub fn end_subscriptions(entry: &Entry) {
    if let Some((source, _)) = entry.object.signals() {
        port::EVENTS.lock().remove_owner(source, entry.id);
    }
}

/// Ends a handle. A port has one handle, so that closes the port.
pub fn close(entry: Entry) {
    end_subscriptions(&entry);
    if let Object::Port(port) = &entry.object {
        port.close();
    }
}
