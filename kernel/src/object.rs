//! What a handle can hold (ADR-0040, ADR-0041).

use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};

use zoraya_abi::{Duplication, ObjectType, Waiting};
use zoraya_elf::Program;
use zoraya_events::SourceId;

use crate::channel::{self, Pair};
use crate::charge::Paid;
use crate::device::{Device, Interrupt, Pin};
use crate::job::Job;
use crate::memory::object::MemoryObject;
use crate::port::{self, Port};
use crate::process::Process;
use crate::stream::{ReadEnd, WriteEnd};

pub type HandleTable = zoraya_handle::Table<Object>;
pub type Entry = zoraya_handle::Entry<Object>;

/// A program `spawn` can start (ADR-0043, ADR-0108).
#[derive(Clone)]
pub enum Image {
    /// In the ramdisk, parsed at each `spawn`.
    Ramdisk {
        name: &'static str,
        file: &'static [u8],
    },
    /// Made by `image_create`: checked once, and held with its object.
    Memory(Arc<MemoryImage>),
}

/// An image from a memory object (ADR-0108 item 1). The headers were checked
/// when it was made and are kept, so W^X and the ranges hold whatever the
/// object's bytes become; `spawn` copies the segments' bytes as they are then.
pub struct MemoryImage {
    pub memory: Arc<MemoryObject>,
    pub program: Program,
    /// The file's whole program header table, for the startup block.
    pub table: Vec<u8>,
    /// The creator pays for the record and the table (ADR-0261 point 6).
    pub _paid: Paid,
}

/// What `event_create` makes: an identity, and what it costs its creator.
pub struct Event {
    pub _paid: Paid,
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
    Event(Arc<Event>),
    Device(Arc<Device>),
    Interrupt(Arc<Interrupt>),
    Pin(Arc<Pin>),
    /// One end of a channel: `end` is 0 or 1.
    Channel(Arc<Pair>, usize),
    /// A byte stream's read end (ADR-0101); each handle holds the `Arc`.
    StreamRead(Arc<ReadEnd>),
    StreamWrite(Arc<WriteEnd>),
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
            (O::StreamRead(a), O::StreamRead(b)) => Arc::ptr_eq(a, b),
            (O::StreamWrite(a), O::StreamWrite(b)) => Arc::ptr_eq(a, b),
            (O::Image(Image::Memory(a)), O::Image(Image::Memory(b))) => Arc::ptr_eq(a, b),
            _ => false,
        }
    }

    fn type_name(&self) -> &'static str {
        match self {
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
            Object::StreamRead(_) => "stream-read",
            Object::StreamWrite(_) => "stream-write",
        }
    }

    /// Its row in `OBJECT_TYPES`.
    pub fn type_index(&self) -> u64 {
        let name = self.type_name();
        let row = zoraya_abi::OBJECT_TYPES.iter().position(|t| t.name == name);
        row.map_or(u64::MAX, |row| row as u64)
    }

    /// Whether `snapshot_process` may duplicate it, from `OBJECT_TYPES`
    /// (ADR-0102 item 3). A type missing there is not.
    pub fn duplicable(&self) -> bool {
        let name = self.type_name();
        zoraya_abi::OBJECT_TYPES
            .iter()
            .any(|t| t.name == name && t.duplication == Duplication::Duplicable)
    }

    /// Where its signals are, and which bits exist: the kernel's side of
    /// ADR-0006 rule 1.
    pub fn signals(&self) -> Option<(SourceId, u32)> {
        let (source, waiting) = match self {
            Object::Channel(pair, end) => (pair.source(*end), zoraya_abi::ChannelObject::WAITING),
            Object::Interrupt(i) => (i.source(), zoraya_abi::InterruptObject::WAITING),
            Object::Process(p) => (p.source(), zoraya_abi::ProcessObject::WAITING),
            Object::Job(j) => (j.source(), zoraya_abi::JobObject::WAITING),
            Object::StreamRead(e) => (e.source(), zoraya_abi::StreamReadObject::WAITING),
            Object::StreamWrite(e) => (e.source(), zoraya_abi::StreamWriteObject::WAITING),
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
        // The source's subscribers count as the units `weight` announced.
        crate::steps::count(port::source_subscriptions(source) / SUBSCRIPTIONS_PER_UNIT);
        port::EVENTS.lock().remove_owner(source, entry.id);
    }
}

/// Subscriptions that cost one unit of a step to end: a subscription leaves
/// its port's list (a search and a move of up to 1024 entries) and its
/// source's, about 0.3 ms for a port of 1024 (the review's host figure, about 20 ms
/// for 64; `t_bound`'s `exit_with_full_ports` checks it). 16 makes a full port
/// 65 units, a whole step by itself (`steps::before` caps it at `STEP`).
const SUBSCRIPTIONS_PER_UNIT: usize = 16;

/// The work of closing a handle in units of a step (`steps::before`), the
/// handle's own unit included (the caller counts it with `steps::count(1)`).
/// A port counts its subscriptions and an object with a source those of the
/// source, since the handle's ending walks them (ADR-0299: the bound per port
/// and per source does not bound a whole exit that closes 64 handles).
pub fn weight(entry: &Entry) -> usize {
    let subscriptions = match &entry.object {
        Object::Memory(object) => return crate::memory::object::weight(object),
        Object::Port(p) => p.subscriptions(),
        object => object
            .signals()
            .map_or(0, |(source, _)| port::source_subscriptions(source)),
    };
    1 + subscriptions / SUBSCRIPTIONS_PER_UNIT
}

/// Ends a handle. A port or a channel end has one handle, so that closes it.
pub fn close(entry: Entry) {
    end_subscriptions(&entry);
    match &entry.object {
        Object::Port(p) => {
            crate::steps::count(p.subscriptions() / SUBSCRIPTIONS_PER_UNIT);
            p.close();
        }
        Object::Channel(pair, end) => channel::close(pair, *end),
        _ => {}
    }
}
