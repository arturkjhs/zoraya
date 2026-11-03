//! Byte streams (ADR-0101) over `zoraya-stream`.
//!
//! A stream is one ring in frames committed at creation, and two end objects.
//! A handle to an end holds an `Arc` of that end, wherever the handle is: a
//! table, a channel message, a snapshot's child. So the last handle of an end
//! going is that `Arc` dropping (ADR-0041), and its `Drop` is what asserts
//! `PEER_CLOSED` on the other end.

use alloc::sync::Arc;

use zoraya_abi::Status;
use zoraya_events::SourceId;
use zoraya_stream::{CAPACITY, Error, PAGE, Refused, Ring, Storage};

use crate::memory::heap::try_arc;
use crate::memory::object::MemoryObject;
use crate::memory::phys_to_virt;
use crate::port;
use crate::sync::SpinLock;
use crate::syscall::UserSlice;

const _: () = assert!(CAPACITY == zoraya_abi::STREAM_CAPACITY);
const _: () = assert!(zoraya_stream::ATOMIC == zoraya_abi::STREAM_ATOMIC);
const _: () = assert!(
    zoraya_stream::READABLE == zoraya_abi::stream_signals::READABLE
        && zoraya_stream::WRITABLE == zoraya_abi::stream_signals::WRITABLE
        && zoraya_stream::PEER_CLOSED == zoraya_abi::stream_signals::PEER_CLOSED
);
const _: () = assert!(PAGE as u64 == zoraya_pmm::FRAME_SIZE);

const READ_END: usize = 0;
const WRITE_END: usize = 1;

struct State {
    ring: Ring,
    /// What each end's source shows, so that only changes reach ports.
    shown: [u32; 2],
}

pub struct Stream {
    state: SpinLock<State>,
    /// The bytes. Never mapped and never named by a handle: only this
    /// stream reaches its frames, and only under `state`.
    memory: Arc<MemoryObject>,
    sources: [SourceId; 2],
}

/// The ring's storage: the object's frames, through the direct map.
struct Frames<'a>(&'a MemoryObject);

impl Frames<'_> {
    fn address(&self, offset: usize) -> u64 {
        let page = offset - offset % PAGE;
        phys_to_virt(self.0.page(page as u64)) + (offset % PAGE) as u64
    }
}

impl Storage for Frames<'_> {
    fn chunk(&self, offset: usize, length: usize) -> &[u8] {
        // SAFETY: `zoraya-stream` gives chunks inside one page of the ring,
        // which is a frame of `memory`, mapped by the direct map and alive
        // while the stream is. Only the stream touches it, under its lock.
        unsafe { core::slice::from_raw_parts(self.address(offset) as *const u8, length) }
    }

    fn chunk_mut(&mut self, offset: usize, length: usize) -> &mut [u8] {
        // SAFETY: as in `chunk`; the lock makes this the only reference.
        unsafe { core::slice::from_raw_parts_mut(self.address(offset) as *mut u8, length) }
    }
}

fn refused(e: Refused) -> Status {
    match e {
        Refused::WouldBlock => Status::WouldBlock,
        Refused::PeerClosed => Status::PeerClosed,
    }
}

impl Stream {
    /// A stream and its two ends, everything committed now (INV-0101-4).
    pub fn create() -> Result<(Arc<ReadEnd>, Arc<WriteEnd>), Status> {
        let memory = MemoryObject::new(CAPACITY as u64, false)?;
        let ring = Ring::new();
        let shown = [ring.read_signals(), ring.write_signals()];
        let read = port::new_source(shown[READ_END])?;
        let write = match port::new_source(shown[WRITE_END]) {
            Ok(source) => source,
            Err(e) => {
                port::free_source(read);
                return Err(e);
            }
        };
        // On failure the value was dropped, and its `Drop` freed the sources.
        let stream = try_arc(Stream {
            state: SpinLock::new(State { ring, shown }),
            memory,
            sources: [read, write],
        })
        .map_err(|_| Status::NoMemory)?;
        // A failed end closes its side as it drops, which only shows.
        let reader = try_arc(ReadEnd(stream.clone())).map_err(|_| Status::NoMemory)?;
        let writer = try_arc(WriteEnd(stream)).map_err(|_| Status::NoMemory)?;
        Ok((reader, writer))
    }

    /// Brings both ends' signals up to date.
    fn show(&self, state: &mut State) {
        let bits = [state.ring.read_signals(), state.ring.write_signals()];
        for end in [READ_END, WRITE_END] {
            let (rising, falling) = (bits[end] & !state.shown[end], state.shown[end] & !bits[end]);
            if falling != 0 {
                port::clear_signals(self.sources[end], falling);
            }
            if rising != 0 {
                port::set_signals(self.sources[end], rising);
            }
            state.shown[end] = bits[end];
        }
    }

    /// Copies what fits from `source` and gives the count (ADR-0101).
    fn write(&self, source: &UserSlice) -> Result<usize, Status> {
        let mut state = self.state.lock();
        let mut frames = Frames(&self.memory);
        let written = state
            .ring
            .write(&mut frames, source.len(), |at, chunk| {
                source.read_into(at as u64, chunk)
            })
            .map_err(|e| match e {
                Error::Refused(r) => refused(r),
                Error::Copy(status) => status,
            })?;
        self.show(&mut state);
        Ok(written)
    }

    /// Copies what is there to `target`, up to its length; 0 is end of file.
    fn read(&self, target: &UserSlice) -> Result<usize, Status> {
        let mut state = self.state.lock();
        let frames = Frames(&self.memory);
        let taken = state
            .ring
            .read(&frames, target.len(), |at, chunk| {
                target.write_at(at as u64, chunk)
            })
            .map_err(|e| match e {
                Error::Refused(r) => refused(r),
                Error::Copy(status) => status,
            })?;
        self.show(&mut state);
        Ok(taken)
    }

    fn close(&self, end: usize) {
        let mut state = self.state.lock();
        match end {
            READ_END => state.ring.close_reader(),
            _ => state.ring.close_writer(),
        }
        self.show(&mut state);
    }
}

impl Drop for Stream {
    fn drop(&mut self) {
        port::free_source(self.sources[READ_END]);
        port::free_source(self.sources[WRITE_END]);
    }
}

/// What every handle to a read end holds.
pub struct ReadEnd(Arc<Stream>);

/// What every handle to a write end holds.
pub struct WriteEnd(Arc<Stream>);

impl ReadEnd {
    pub fn source(&self) -> SourceId {
        self.0.sources[READ_END]
    }

    pub fn read(&self, target: &UserSlice) -> Result<usize, Status> {
        self.0.read(target)
    }
}

impl WriteEnd {
    pub fn source(&self) -> SourceId {
        self.0.sources[WRITE_END]
    }

    pub fn write(&self, source: &UserSlice) -> Result<usize, Status> {
        self.0.write(source)
    }
}

impl Drop for ReadEnd {
    fn drop(&mut self) {
        self.0.close(READ_END);
    }
}

impl Drop for WriteEnd {
    fn drop(&mut self) {
        self.0.close(WRITE_END);
    }
}
