//! The client-facing half of the NIC driver (ADR-0280, ADR-0221 item 2): the
//! messages of `zoraya-proto-nic` in, the messages out, over a [`Host`] that
//! is the channel, the pin and the clock. It is logic only, so that it runs
//! on the host under test; `ethd` supplies the [`Host`].
//!
//! Every message from the client is hostile (INV-0221-2). One that does not
//! decode, or has an unknown ordinal, ends the driver ([`Flow::Stop`]).
//!
//! **Nothing pushed is dropped on a full channel**, and memory stays
//! bounded. What the driver owes the client is kept as state, never as a
//! queue, and is sent again by [`Server::flush`] when the channel takes
//! messages (`ethd` calls it on every event and when the channel is
//! writable):
//! - the transmit slots the device finished: one bit each, and a slot is not
//!   free ([`Net::reported`]) until the `Sent` that names it was written, so
//!   it is not reused before the client knows;
//! - at most one `InfoReply` (the newest request's transaction) and one
//!   `Done` (likewise);
//! - at most one pending refusal reason for each transmit slot;
//! - one "link changed" flag; the message carries the link state at the time
//!   it is written.
//!
//! A `Received` that cannot be written is undone: the slots go back to the
//! device and the frames are counted dropped ([`Net::undo`]); frames are
//! best effort, slots are not.

use zoraya_proto_nic::{
    Attach, Done, Frame, Info, InfoReply, LinkChanged, MAX_BATCH, Received, Refusal, Rejected,
    Release, Sent, Slot, Transmit, reason, status,
};
use zoraya_protocol::{Below, Data, Header, Message, NO_REPLY, Seq, decode, encode};
use zoraya_virtio::{Registers, Shared};

use crate::{Net, Refused, SLOTS};

const _: () = assert!(SLOTS as u32 == zoraya_proto_nic::SLOTS);
const _: () = assert!(crate::OBJECT_SIZE == zoraya_proto_nic::OBJECT_SIZE);
const _: () = assert!(SLOTS <= 64, "the finished slots are one u64");

/// The size of the buffers for one message each way.
pub const BUFFER: usize = 2048;
/// Messages `serve` reads in one call; the level-triggered channel brings
/// the rest, so that a flooding client cannot keep the driver here.
pub const MAX_MESSAGES: usize = 64;

/// The result of writing one message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Write {
    Done,
    /// The client's queue is full: try again when it is writable.
    Full,
    /// The client is gone.
    Closed,
}

/// The result of reading one message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Read<H> {
    Message {
        length: usize,
        handle: Option<H>,
    },
    Empty,
    /// The client is gone, or the channel is broken.
    Closed,
}

/// What the driver needs from the process around it.
pub trait Host {
    type Handle: Copy;
    /// Writes one message to the client.
    fn write(&mut self, bytes: &[u8]) -> Write;
    /// Reads one message of the client into `buffer`.
    fn read(&mut self, buffer: &mut [u8]) -> Read<Self::Handle>;
    /// Closes a handle that came with a message.
    fn close(&mut self, handle: Self::Handle);
    /// Pins the client's frames object for the device: its device address
    /// if it is one run of `OBJECT_SIZE` bytes or more that the device may
    /// write, else `None`. The pin is kept until [`Host::unpin`].
    fn pin(&mut self, handle: Self::Handle) -> Option<u64>;
    /// Releases the pin of the last successful [`Host::pin`].
    fn unpin(&mut self);
    /// The monotonic time, nanoseconds.
    fn now(&mut self) -> u64;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Flow {
    Go,
    /// End the driver (reset, unpin, exit).
    Stop,
    /// As `Stop`, because the device broke the protocol.
    Broken,
}

pub struct Server<R, S, H> {
    pub net: Net<R, S>,
    pub host: H,
    pinned: bool,
    info: Option<u32>,
    done: Option<(u32, u32)>,
    /// A reason from `reason` for each transmit slot, or 0.
    rejected: [u32; SLOTS],
    /// Bit `i`: transmit slot `SLOTS + i` finished, not yet reported.
    finished: u64,
    link: bool,
    out: [u8; BUFFER],
}

fn put<'a, H: Host, M: Message<'a>>(host: &mut H, out: &mut [u8; BUFFER], m: &M) -> Write {
    match encode(m, NO_REPLY, out) {
        Ok(n) => host.write(&out[..n]),
        // A message that cannot be built cannot be sent later either.
        Err(_) => Write::Done,
    }
}

impl<R: Registers, S: Shared, H: Host> Server<R, S, H> {
    pub fn new(net: Net<R, S>, host: H) -> Self {
        Server {
            net,
            host,
            pinned: false,
            info: None,
            done: None,
            rejected: [0; SLOTS],
            finished: 0,
            link: false,
            out: [0; BUFFER],
        }
    }

    /// Whether something is still to be written: the channel's `WRITABLE`
    /// signal is then what `ethd` waits for.
    pub fn pending(&self) -> bool {
        self.info.is_some()
            || self.done.is_some()
            || self.finished != 0
            || self.link
            || self.rejected.iter().any(|r| *r != 0)
    }

    fn attach(&mut self, handle: H::Handle) -> u32 {
        if self.pinned {
            return status::BAD_OBJECT;
        }
        let Some(address) = self.host.pin(handle) else {
            return status::BAD_OBJECT;
        };
        if self.net.attach(address).is_err() {
            self.host.unpin();
            return status::BAD_OBJECT;
        }
        self.pinned = true;
        status::OK
    }

    /// Reads and carries out the client's messages, then writes what is
    /// pending.
    pub fn serve(&mut self) -> Flow {
        let mut buffer = [0u8; BUFFER];
        for _ in 0..MAX_MESSAGES {
            match self.host.read(&mut buffer) {
                Read::Empty => break,
                Read::Closed => return Flow::Stop,
                Read::Message { length, handle } => {
                    let bytes = &buffer[..length.min(BUFFER)];
                    if self.handle(bytes, handle) == Flow::Stop {
                        return Flow::Stop;
                    }
                }
            }
        }
        self.flush()
    }

    fn handle(&mut self, bytes: &[u8], memory: Option<H::Handle>) -> Flow {
        let Ok((header, _)) = Header::decode(bytes) else {
            if let Some(h) = memory {
                self.host.close(h);
            }
            return Flow::Stop;
        };
        let t = header.transaction;
        // Only `Attach` takes a handle; any other is closed, not kept.
        if let Some(h) = memory
            && header.ordinal != 2
        {
            self.host.close(h);
        }
        match header.ordinal {
            1 if decode::<Info>(bytes, 0).is_ok() => self.info = Some(t),
            2 => match memory {
                Some(h) => {
                    if decode::<Attach>(bytes, 1).is_err() {
                        self.host.close(h);
                        return Flow::Stop;
                    }
                    let s = self.attach(h);
                    self.host.close(h);
                    self.done = Some((t, s));
                }
                None => self.done = Some((t, status::BAD_OBJECT)),
            },
            3 => {
                let Ok((_, m)) = decode::<Transmit>(bytes, 0) else {
                    return Flow::Stop;
                };
                for f in m.frames.iter() {
                    let Err(why) = self.net.transmit(f.slot, f.length) else {
                        continue;
                    };
                    // A slot out of range is only counted: the client has
                    // no such slot to free.
                    let Some(index) = (f.slot as usize).checked_sub(SLOTS).filter(|i| *i < SLOTS)
                    else {
                        continue;
                    };
                    self.rejected[index] = match why {
                        Refused::NotAttached => reason::NOT_ATTACHED,
                        Refused::BadLength => reason::BAD_LENGTH,
                        Refused::WrongState => reason::STILL_QUEUED,
                        Refused::BadSlot | Refused::Attached => reason::BAD_SLOT,
                    };
                }
            }
            4 => {
                let Ok((_, m)) = decode::<Release>(bytes, 0) else {
                    return Flow::Stop;
                };
                for s in m.slots.iter() {
                    let _ = self.net.release(s.slot);
                }
            }
            _ => return Flow::Stop,
        }
        Flow::Go
    }

    /// Takes what the device has finished, tells the client, and writes
    /// what is pending. The caller acknowledges the interrupt line after.
    pub fn device_event(&mut self) -> Flow {
        let woke = self.host.now();
        self.net.acknowledge();
        let mut frames = [Frame { slot: 0, length: 0 }; SLOTS];
        let mut count = 0;
        let mut finished = 0u64;
        let polled = self.net.poll(
            &mut |slot, length| {
                if count < SLOTS {
                    frames[count] = Frame { slot, length };
                    count += 1;
                }
                true
            },
            &mut |slot| finished |= 1 << (slot as usize - SLOTS),
        );
        self.finished |= finished;
        if polled.is_err() {
            return Flow::Broken;
        }
        // The slots are the client's from here; if the message cannot be
        // written, they are given back so that the device does not run out
        // of buffers.
        for chunk in frames[..count].chunks(MAX_BATCH) {
            let m = Received {
                at: woke,
                frames: Seq::Of(chunk),
            };
            match put(&mut self.host, &mut self.out, &m) {
                Write::Done => {}
                Write::Full => {
                    for f in chunk {
                        let _ = self.net.undo(f.slot);
                    }
                }
                Write::Closed => return Flow::Stop,
            }
        }
        if self.net.link_changed().is_some() {
            self.link = true;
        }
        self.flush()
    }

    /// Writes what is pending, until the channel is full.
    pub fn flush(&mut self) -> Flow {
        if let Some((_, s)) = self.done {
            match put(&mut self.host, &mut self.out, &Done { status: Below(s) }) {
                Write::Done => self.done = None,
                Write::Full => return Flow::Go,
                Write::Closed => return Flow::Stop,
            }
        }
        if self.info.is_some() {
            let mac = self.net.mac();
            let m = InfoReply {
                status: Below(status::OK),
                link: self.net.link(),
                mac: Data(&mac),
            };
            match put(&mut self.host, &mut self.out, &m) {
                Write::Done => self.info = None,
                Write::Full => return Flow::Go,
                Write::Closed => return Flow::Stop,
            }
        }
        let mut refusals = [Refusal { slot: 0, reason: 0 }; SLOTS];
        let mut count = 0;
        for (i, r) in self.rejected.iter().enumerate() {
            if *r != 0 {
                refusals[count] = Refusal {
                    slot: (SLOTS + i) as u32,
                    reason: *r,
                };
                count += 1;
            }
        }
        if count > 0 {
            let m = Rejected {
                slots: Seq::Of(&refusals[..count]),
            };
            match put(&mut self.host, &mut self.out, &m) {
                Write::Done => self.rejected = [0; SLOTS],
                Write::Full => return Flow::Go,
                Write::Closed => return Flow::Stop,
            }
        }
        if self.finished != 0 {
            let mut slots = [Slot { slot: 0 }; SLOTS];
            let mut count = 0;
            for i in 0..SLOTS {
                if self.finished & (1 << i) != 0 {
                    slots[count] = Slot {
                        slot: (SLOTS + i) as u32,
                    };
                    count += 1;
                }
            }
            let m = Sent {
                slots: Seq::Of(&slots[..count]),
            };
            match put(&mut self.host, &mut self.out, &m) {
                Write::Done => {
                    for s in &slots[..count] {
                        self.net.reported(s.slot);
                    }
                    self.finished = 0;
                }
                Write::Full => return Flow::Go,
                Write::Closed => return Flow::Stop,
            }
        }
        if self.link {
            let m = LinkChanged {
                link: self.net.link(),
            };
            match put(&mut self.host, &mut self.out, &m) {
                Write::Done => self.link = false,
                Write::Full => return Flow::Go,
                Write::Closed => return Flow::Stop,
            }
        }
        Flow::Go
    }
}

#[cfg(test)]
mod tests;
