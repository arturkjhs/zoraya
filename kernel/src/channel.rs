//! Channels (ADR-0050) over `zoraya-channel`. An end has one handle, so the
//! end's closing is that handle's.

use alloc::sync::Arc;

use zoraya_channel::{Channel, Message, Pile, Refused};
use zoraya_events::SourceId;

use zoraya_abi::{Status, spawn::Transfer};

use crate::memory::heap::{try_arc, try_box};
use crate::object::{self, Entry, Object};
use crate::port;
use crate::sync::SpinLock;

pub use zoraya_channel::{MAX_MESSAGE, MAX_MESSAGE_HANDLES};

pub struct Pair {
    channel: SpinLock<Channel<Entry>>,
    sources: [SourceId; 2],
    /// What each end's source shows, so that only changes reach ports.
    shown: SpinLock<[u32; 2]>,
}

impl Pair {
    pub fn new() -> Result<Arc<Pair>, Status> {
        let channel = Channel::new();
        let first = port::new_source(channel.signals(0))?;
        let second = match port::new_source(channel.signals(1)) {
            Ok(source) => source,
            Err(e) => {
                port::free_source(first);
                return Err(e);
            }
        };
        let shown = [channel.signals(0), channel.signals(1)];
        // On failure the value was dropped, and its `Drop` freed the sources.
        try_arc(Pair {
            channel: SpinLock::new(channel),
            sources: [first, second],
            shown: SpinLock::new(shown),
        })
        .map_err(|_| Status::NoMemory)
    }

    pub fn source(&self, end: usize) -> SourceId {
        self.sources[end]
    }

    /// Brings both ends' signals up to date.
    fn show(&self, channel: &Channel<Entry>) {
        let mut shown = self.shown.lock();
        for end in 0..2 {
            let bits = channel.signals(end);
            let (rising, falling) = (bits & !shown[end], shown[end] & !bits);
            if falling != 0 {
                port::clear_signals(self.sources[end], falling);
            }
            if rising != 0 {
                port::set_signals(self.sources[end], rising);
            }
            shown[end] = bits;
        }
    }

    pub fn holds_handles(&self, end: usize) -> bool {
        self.channel.lock().holds_handles(end)
    }

    /// Checks that a message can go, and makes room.
    pub fn prepare(&self, end: usize, bytes: usize) -> Result<(), Status> {
        self.channel
            .lock()
            .prepare(end, bytes)
            .map_err(|e| match e {
                Refused::Full => Status::WouldBlock,
                Refused::PeerClosed => Status::PeerClosed,
                Refused::NoMemory => Status::NoMemory,
            })
    }

    /// After [`Pair::prepare`], with nothing between.
    pub fn write(&self, end: usize, message: alloc::boxed::Box<Message<Entry>>) {
        let mut channel = self.channel.lock();
        channel.write(end, message);
        self.show(&channel);
    }

    pub fn peek(&self, end: usize) -> Result<Option<(usize, usize)>, Status> {
        let channel = self.channel.lock();
        match channel.peek(end) {
            Some(sizes) => Ok(Some(sizes)),
            None if channel.is_closed(1 - end) => Err(Status::PeerClosed),
            None => Ok(None),
        }
    }

    /// Calls `f` with the next message's bytes, leaving the message.
    pub fn copy_front(
        &self,
        end: usize,
        f: impl FnOnce(&[u8]) -> Result<(), Status>,
    ) -> Result<(), Status> {
        let channel = self.channel.lock();
        f(channel.front_bytes(end).ok_or(Status::WouldBlock)?)
    }

    pub fn read(&self, end: usize) -> Option<alloc::boxed::Box<Message<Entry>>> {
        let mut channel = self.channel.lock();
        let message = channel.read(end);
        self.show(&channel);
        message
    }
}

impl Drop for Pair {
    fn drop(&mut self) {
        port::free_source(self.sources[0]);
        port::free_source(self.sources[1]);
    }
}

/// A message's bytes and handles, boxed.
pub fn message(
    bytes: alloc::vec::Vec<u8>,
    handles: alloc::vec::Vec<Entry>,
) -> Result<alloc::boxed::Box<Message<Entry>>, Status> {
    try_box(Message::new(bytes, handles)).map_err(|_| Status::NoMemory)
}

/// How a listed handle's table entry reaches a message.
pub fn needed_right(transfer: Transfer) -> zoraya_abi::Rights {
    match transfer {
        Transfer::Move => zoraya_abi::Rights::TRANSFER,
        Transfer::Duplicate => zoraya_abi::Rights::DUPLICATE,
    }
}

/// Ends one end. Everything queued for it goes too, ends included, and the
/// ends in those messages lose what is queued for them in turn: the work list
/// runs through the messages themselves, so the depth costs no stack
/// (INV-0050-6).
pub fn close(pair: &Pair, end: usize) {
    let mut work = close_one(pair, end);
    while let Some(mut message) = work.pop() {
        while let Some(entry) = message.handles.pop() {
            object::end_subscriptions(&entry);
            match &entry.object {
                Object::Channel(other, other_end) => {
                    let mut more = close_one(other, *other_end);
                    work.take_from(&mut more);
                }
                Object::Port(port) => port.close(),
                _ => {}
            }
        }
    }
}

fn close_one(pair: &Pair, end: usize) -> Pile<Entry> {
    let mut channel = pair.channel.lock();
    let pile = channel.close(end);
    pair.show(&channel);
    pile
}
