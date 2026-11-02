//! A byte stream's ring (ADR-0101), as data: where the bytes are, how much a
//! write or read takes, and the signals of each end. Nothing here owns the
//! bytes. A [`Storage`] does, so that the kernel can keep them in frames and
//! copy between them and user memory without a buffer of its own.
//!
//! The rules, from ADR-0101:
//!
//! - A write of at most [`ATOMIC`] bytes goes in whole or not at all, and it
//!   needs [`ATOMIC`] bytes free, which is what `WRITABLE` promises. A longer
//!   write takes what fits, and is refused only when nothing does.
//! - A read takes what is there, up to its room. An empty ring is
//!   [`Refused::WouldBlock`] while a write end exists, and end of file (0)
//!   after.
//! - A write with no read end is [`Refused::PeerClosed`] and changes nothing.
//!
//! A transfer of zero bytes does nothing and returns 0, except that a write
//! with no read end is still refused.

#![no_std]
#![forbid(unsafe_code)]

#[cfg(test)]
extern crate std;

/// Bytes a stream holds (ADR-0101: Linux's default pipe size).
pub const CAPACITY: usize = 64 * 1024;
/// Writes up to this size are all or nothing (Linux's and musl's `PIPE_BUF`).
pub const ATOMIC: usize = 4096;
/// No chunk crosses a multiple of this, so each lies in one page of storage.
pub const PAGE: usize = 4096;

const _: () = assert!(CAPACITY.is_multiple_of(PAGE) && ATOMIC <= CAPACITY);

/// Signal bits, as `zoraya_abi::stream_signals`.
pub const READABLE: u32 = 1 << 0;
pub const WRITABLE: u32 = 1 << 1;
pub const PEER_CLOSED: u32 = 1 << 2;

/// Why a transfer moved nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refused {
    /// Try again when signalled.
    WouldBlock,
    /// No read end exists: nothing will ever read this.
    PeerClosed,
}

/// Why a transfer through [`Ring::write`] or [`Ring::read`] moved nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error<E> {
    Refused(Refused),
    /// The copy failed; the ring is as it was.
    Copy(E),
}

/// Where a stream's bytes live: [`CAPACITY`] bytes, reached a chunk at a time.
/// No chunk crosses a multiple of [`PAGE`].
pub trait Storage {
    fn chunk(&self, offset: usize, length: usize) -> &[u8];
    fn chunk_mut(&mut self, offset: usize, length: usize) -> &mut [u8];
}

/// The pieces of `length` bytes from `offset` on, around the ring, each within
/// one page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Chunks {
    offset: usize,
    left: usize,
}

impl Iterator for Chunks {
    /// `(offset, length)`.
    type Item = (usize, usize);

    fn next(&mut self) -> Option<(usize, usize)> {
        if self.left == 0 {
            return None;
        }
        let page_end = (self.offset / PAGE + 1) * PAGE;
        let length = self.left.min(page_end - self.offset);
        let chunk = (self.offset, length);
        self.offset = (self.offset + length) % CAPACITY;
        self.left -= length;
        Some(chunk)
    }
}

/// One stream's state. Both ends start open, and each closes once.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ring {
    /// Where the oldest byte is.
    start: usize,
    length: usize,
    reader: bool,
    writer: bool,
}

impl Default for Ring {
    fn default() -> Ring {
        Ring::new()
    }
}

impl Ring {
    pub const fn new() -> Ring {
        Ring {
            start: 0,
            length: 0,
            reader: true,
            writer: true,
        }
    }

    /// Bytes waiting to be read.
    pub fn len(&self) -> usize {
        self.length
    }

    pub fn is_empty(&self) -> bool {
        self.length == 0
    }

    pub fn free(&self) -> usize {
        CAPACITY - self.length
    }

    /// The last read-end handle went.
    pub fn close_reader(&mut self) {
        self.reader = false;
    }

    /// The last write-end handle went.
    pub fn close_writer(&mut self) {
        self.writer = false;
    }

    pub fn reader_open(&self) -> bool {
        self.reader
    }

    pub fn writer_open(&self) -> bool {
        self.writer
    }

    /// How many bytes of a write of `requested` would go in.
    pub fn accept(&self, requested: usize) -> Result<usize, Refused> {
        if !self.reader {
            return Err(Refused::PeerClosed);
        }
        let free = self.free();
        match requested {
            0 => Ok(0),
            small if small <= ATOMIC => {
                if free >= ATOMIC {
                    Ok(small)
                } else {
                    Err(Refused::WouldBlock)
                }
            }
            large => {
                if free == 0 {
                    Err(Refused::WouldBlock)
                } else {
                    Ok(large.min(free))
                }
            }
        }
    }

    /// How many bytes a read with `room` would take: 0 is end of file, or a
    /// read of nothing.
    pub fn available(&self, room: usize) -> Result<usize, Refused> {
        if room == 0 {
            return Ok(0);
        }
        if self.length == 0 {
            return if self.writer {
                Err(Refused::WouldBlock)
            } else {
                Ok(0)
            };
        }
        Ok(room.min(self.length))
    }

    /// Where the next `count` bytes written go. At most the free space.
    pub fn free_chunks(&self, count: usize) -> Chunks {
        Chunks {
            offset: (self.start + self.length) % CAPACITY,
            left: count.min(self.free()),
        }
    }

    /// Where the next `count` bytes to read are. At most what is there.
    pub fn data_chunks(&self, count: usize) -> Chunks {
        Chunks {
            offset: self.start,
            left: count.min(self.length),
        }
    }

    /// Makes `count` bytes written into [`Ring::free_chunks`] readable.
    pub fn commit_write(&mut self, count: usize) {
        self.length += count.min(self.free());
    }

    /// Drops `count` bytes from the front, after they were read.
    pub fn commit_read(&mut self, count: usize) {
        let count = count.min(self.length);
        self.start = (self.start + count) % CAPACITY;
        self.length -= count;
    }

    /// The read end's signals.
    pub fn read_signals(&self) -> u32 {
        let mut bits = 0;
        if self.length > 0 || !self.writer {
            bits |= READABLE;
        }
        if !self.writer {
            bits |= PEER_CLOSED;
        }
        bits
    }

    /// The write end's signals.
    pub fn write_signals(&self) -> u32 {
        let mut bits = 0;
        if self.reader && self.free() >= ATOMIC {
            bits |= WRITABLE;
        }
        if !self.reader {
            bits |= PEER_CLOSED;
        }
        bits
    }

    /// Writes up to `requested` bytes. `fill(at, chunk)` fills `chunk` with the
    /// source's bytes from `at` on. If it fails, nothing is written.
    pub fn write<S: Storage, E>(
        &mut self,
        storage: &mut S,
        requested: usize,
        mut fill: impl FnMut(usize, &mut [u8]) -> Result<(), E>,
    ) -> Result<usize, Error<E>> {
        let count = self.accept(requested).map_err(Error::Refused)?;
        let mut at = 0;
        for (offset, length) in self.free_chunks(count) {
            fill(at, storage.chunk_mut(offset, length)).map_err(Error::Copy)?;
            at += length;
        }
        self.commit_write(count);
        Ok(count)
    }

    /// Reads up to `room` bytes. `drain(at, chunk)` takes `chunk` to the
    /// target's place `at`. If it fails, nothing is taken.
    pub fn read<S: Storage, E>(
        &mut self,
        storage: &S,
        room: usize,
        mut drain: impl FnMut(usize, &[u8]) -> Result<(), E>,
    ) -> Result<usize, Error<E>> {
        let count = self.available(room).map_err(Error::Refused)?;
        let mut at = 0;
        for (offset, length) in self.data_chunks(count) {
            drain(at, storage.chunk(offset, length)).map_err(Error::Copy)?;
            at += length;
        }
        self.commit_read(count);
        Ok(count)
    }
}

#[cfg(test)]
mod tests;
