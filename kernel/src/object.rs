//! What a handle can hold (ADR-0040, ADR-0041).

use alloc::sync::Arc;
use core::sync::atomic::{AtomicU64, Ordering};

use zoraya_abi::{ObjectType, Waiting};
use zoraya_events::SourceId;

use crate::channel::{self, Pair};
use crate::device::{Device, Interrupt, Pin};
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
    Power,
    Directory,
    Image(Image),
    Process(Arc<Process>),
    Job(Arc<Job>),
    Port(Arc<Port>),
    Memory(Arc<MemoryObject>),
    /// Its identity is all it has (ADR-0080).
    Event(Arc<u8>),
    Device(Arc<Device>),
    Interrupt(Arc<Interrupt>),
    Pin(Arc<Pin>),
    /// One end of a channel: `end` is 0 or 1.
    Channel(Arc<Pair>, usize),
}

impl Object {
    /// True if both name one object.
    pub fn same(&self, other: &Object) -> bool {
        use Object as O;
        match (self, other) {
            (O::Console, O::Console) | (O::Power, O::Power) | (O::Directory, O::Directory) => true,
            (O::Event(a), O::Event(b)) => Arc::ptr_eq(a, b),
            (O::Memory(a), O::Memory(b)) => Arc::ptr_eq(a, b),
            (O::Process(a), O::Process(b)) => Arc::ptr_eq(a, b),
            (O::Job(a), O::Job(b)) => Arc::ptr_eq(a, b),
            (O::Port(a), O::Port(b)) => Arc::ptr_eq(a, b),
            (O::Device(a), O::Device(b)) => Arc::ptr_eq(a, b),
            (O::Channel(a, x), O::Channel(b, y)) => Arc::ptr_eq(a, b) && x == y,
            _ => false,
        }
    }

    /// Its row in `OBJECT_TYPES`.
    pub fn type_index(&self) -> u64 {
        let name = match self {
            Object::Console => "console",
            Object::Power => "power",
            Object::Directory => "directory",
            Object::Image(_) => "image",
            Object::Process(_) => "process",
            Object::Job(_) => "job",
            Object::Port(_) => "port",
            Object::Memory(_) => "memory",
            Object::Event(_) => "event",
            Object::Device(_) => "device",
            Object::Interrupt(_) => "interrupt",
            Object::Pin(_) => "pin",
            Object::Channel(..) => "channel",
        };
        let row = zoraya_abi::OBJECT_TYPES
            .iter()
            .position(|(n, _, _)| *n == name);
        row.map_or(u64::MAX, |row| row as u64)
    }

    /// Where its signals are, and which bits exist: the kernel's side of
    /// ADR-0006 rule 1.
    pub fn signals(&self) -> Option<(SourceId, u32)> {
        let (source, waiting) = match self {
            Object::Channel(pair, end) => (pair.source(*end), zoraya_abi::ChannelObject::WAITING),
            Object::Interrupt(i) => (i.source(), zoraya_abi::InterruptObject::WAITING),
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
            | Object::Memory(_)
            | Object::Event(_)
            | Object::Device(_)
            | Object::Pin(_) => {
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

/// Ends a handle. A port or a channel end has one handle, so that closes it.
pub fn close(entry: Entry) {
    end_subscriptions(&entry);
    match &entry.object {
        Object::Port(port) => port.close(),
        Object::Channel(pair, end) => channel::close(pair, *end),
        _ => {}
    }
}
