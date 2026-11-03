//! A channel between two ends (ADR-0050), as data: queues, their bounds, and
//! signal state. `H` is what a message carries for a handle. The kernel builds
//! a message before it asks to queue it, so nothing here allocates except the
//! one room [`Channel::prepare`] reserves.
//!
//! Closing an end gives its unread messages as a [`Pile`], a list threaded
//! through the messages. Tearing down a chain of ends in each other's queues
//! then needs no stack and no memory, however deep it is (INV-0050-6).

#![no_std]
#![forbid(unsafe_code)]

extern crate alloc;
#[cfg(test)]
extern crate std;

use alloc::boxed::Box;
use alloc::collections::VecDeque;
use alloc::vec::Vec;

/// Largest message, in bytes.
pub const MAX_MESSAGE: usize = 64 * 1024;
/// Most handles in a message.
pub const MAX_MESSAGE_HANDLES: usize = 64;
/// Messages a direction holds.
pub const MAX_QUEUED: usize = 64;
/// Bytes a direction holds.
pub const MAX_QUEUED_BYTES: usize = 256 * 1024;

/// Signal bits, as `zoraya_abi::channel_signals`.
pub const READABLE: u32 = 1 << 0;
pub const WRITABLE: u32 = 1 << 1;
pub const PEER_CLOSED: u32 = 1 << 2;

pub struct Message<H> {
    pub bytes: Vec<u8>,
    pub handles: Vec<H>,
    next: Option<Box<Message<H>>>,
}

impl<H> Message<H> {
    pub const fn new(bytes: Vec<u8>, handles: Vec<H>) -> Message<H> {
        Message {
            bytes,
            handles,
            next: None,
        }
    }
}

/// Messages to destroy. Dropping it takes no recursion.
pub struct Pile<H> {
    head: Option<Box<Message<H>>>,
}

impl<H> Pile<H> {
    pub const fn new() -> Pile<H> {
        Pile { head: None }
    }

    pub fn pop(&mut self) -> Option<Box<Message<H>>> {
        let mut message = self.head.take()?;
        self.head = message.next.take();
        Some(message)
    }

    fn push(&mut self, mut message: Box<Message<H>>) {
        message.next = self.head.take();
        self.head = Some(message);
    }

    /// Moves everything in `other` here.
    pub fn take_from(&mut self, other: &mut Pile<H>) {
        while let Some(message) = other.pop() {
            self.push(message);
        }
    }
}

impl<H> Default for Pile<H> {
    fn default() -> Self {
        Pile::new()
    }
}

impl<H> Drop for Pile<H> {
    fn drop(&mut self) {
        while self.pop().is_some() {}
    }
}

struct Direction<H> {
    queue: VecDeque<Box<Message<H>>>,
    bytes: usize,
    handles: usize,
}

impl<H> Direction<H> {
    const fn new() -> Direction<H> {
        Direction {
            queue: VecDeque::new(),
            bytes: 0,
            handles: 0,
        }
    }
}

/// Why a message cannot go now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refused {
    /// The queue is full: try when `WRITABLE`.
    Full,
    PeerClosed,
    NoMemory,
}

/// The ends are 0 and 1. A direction is named by the end that reads it.
pub struct Channel<H> {
    inbound: [Direction<H>; 2],
    closed: [bool; 2],
}

impl<H> Channel<H> {
    pub const fn new() -> Channel<H> {
        Channel {
            inbound: [Direction::new(), Direction::new()],
            closed: [false; 2],
        }
    }

    /// Checks that a message of `bytes` can be queued by `end`, and makes room
    /// so that [`Channel::write`] cannot fail. A message over the limits is
    /// the caller's to refuse first.
    pub fn prepare(&mut self, end: usize, bytes: usize) -> Result<(), Refused> {
        if self.closed[1 - end] {
            return Err(Refused::PeerClosed);
        }
        let target = &mut self.inbound[1 - end];
        if target.queue.len() >= MAX_QUEUED || target.bytes + bytes > MAX_QUEUED_BYTES {
            return Err(Refused::Full);
        }
        target.queue.try_reserve(1).map_err(|_| Refused::NoMemory)
    }

    /// Queues a message for the other end, after [`Channel::prepare`].
    pub fn write(&mut self, end: usize, message: Box<Message<H>>) {
        let target = &mut self.inbound[1 - end];
        target.bytes += message.bytes.len();
        target.handles += message.handles.len();
        target.queue.push_back(message);
    }

    /// The sizes of the next message for `end`.
    pub fn peek(&self, end: usize) -> Option<(usize, usize)> {
        let head = self.inbound[end].queue.front()?;
        Some((head.bytes.len(), head.handles.len()))
    }

    /// The bytes of the next message for `end`.
    pub fn front_bytes(&self, end: usize) -> Option<&[u8]> {
        Some(&self.inbound[end].queue.front()?.bytes)
    }

    pub fn read(&mut self, end: usize) -> Option<Box<Message<H>>> {
        let source = &mut self.inbound[end];
        let message = source.queue.pop_front()?;
        source.bytes -= message.bytes.len();
        source.handles -= message.handles.len();
        Some(message)
    }

    pub fn is_closed(&self, end: usize) -> bool {
        self.closed[end]
    }

    /// True if `end` has handles queued for reading, so that it cannot move.
    pub fn holds_handles(&self, end: usize) -> bool {
        self.inbound[end].handles != 0
    }

    /// Ends `end`, and gives its unread messages. Messages it sent stay for
    /// the other end to read.
    pub fn close(&mut self, end: usize) -> Pile<H> {
        self.closed[end] = true;
        let mut pile = Pile::new();
        let inbound = &mut self.inbound[end];
        while let Some(message) = inbound.queue.pop_front() {
            pile.push(message);
        }
        inbound.bytes = 0;
        inbound.handles = 0;
        pile
    }

    /// What `end` shows a port. `WRITABLE` means any message fits.
    pub fn signals(&self, end: usize) -> u32 {
        let mut bits = 0;
        if !self.inbound[end].queue.is_empty() {
            bits |= READABLE;
        }
        if self.closed[1 - end] {
            bits |= PEER_CLOSED;
        } else {
            let target = &self.inbound[1 - end];
            if target.queue.len() < MAX_QUEUED && target.bytes + MAX_MESSAGE <= MAX_QUEUED_BYTES {
                bits |= WRITABLE;
            }
        }
        bits
    }
}

impl<H> Default for Channel<H> {
    fn default() -> Self {
        Channel::new()
    }
}

/// The no-cycle rule (ADR-0050): may an end travel through a channel? `same`:
/// it is an end of that channel, the sender's own end or its peer. `holds`: it
/// has handles queued for reading. A cycle needs a last edge, and that edge
/// moves an end whose queue already holds a handle.
pub const fn may_travel(same: bool, holds: bool) -> bool {
    !same && !holds
}

#[cfg(test)]
mod tests;

// A runaway test fails at 512 MiB instead of taking the host (docs/backlog.md).
#[cfg(test)]
zoraya_test_alloc::cap_memory!();
