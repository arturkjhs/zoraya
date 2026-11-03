//! The native calls the library makes, as a trait: the `ffi` module puts
//! `zoraya-rt` behind it, and the host tests a fake kernel with the real
//! filesystem service. Nothing in the library calls the kernel otherwise.

#![forbid(unsafe_code)]

use alloc::vec::Vec;

use zoraya_abi::Status;

/// Memory of the program's: an address and a length the library never turns
/// into a reference. Streams hand it to the kernel, which checks it; a copy
/// goes through [`Sys::copy_in`] and [`Sys::copy_out`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct User {
    pub address: u64,
    pub length: usize,
}

impl User {
    pub const fn new(address: u64, length: usize) -> User {
        User { address, length }
    }

    /// `length` bytes from `offset`, within this.
    pub fn part(self, offset: usize, length: usize) -> User {
        let offset = offset.min(self.length);
        User {
            address: self.address.wrapping_add(offset as u64),
            length: length.min(self.length - offset),
        }
    }
}

/// The signals the library waits for (`zoraya_abi::channel_signals` and
/// `stream_signals` agree on these bits).
pub mod signals {
    pub const READABLE: u32 = 1 << 0;
    pub const WRITABLE: u32 = 1 << 1;
    pub const PEER_CLOSED: u32 = 1 << 2;
}

pub trait Sys {
    /// A handle.
    type H: Copy + Eq + core::fmt::Debug;

    fn close(&mut self, h: Self::H);
    /// A duplicate with the same rights, for the scratch buffer.
    fn duplicate(&mut self, h: Self::H) -> Result<Self::H, Status>;

    fn channel_create(&mut self) -> Result<(Self::H, Self::H), Status>;
    /// Sends a message, moving `moved`. Whatever happens, the moved handles
    /// are no longer the caller's.
    fn channel_write(
        &mut self,
        end: Self::H,
        bytes: &[u8],
        moved: &[Self::H],
    ) -> Result<(), Status>;
    /// Takes the next message into `bytes`, its handles onto `handles`, and
    /// gives its size; `WouldBlock` when there is none.
    fn channel_read(
        &mut self,
        end: Self::H,
        bytes: &mut [u8],
        handles: &mut Vec<Self::H>,
    ) -> Result<usize, Status>;
    /// Waits until `h` asserts one of `wanted`, and gives what it asserts.
    fn wait(&mut self, h: Self::H, wanted: u32) -> Result<u32, Status>;

    fn stream_create(&mut self) -> Result<(Self::H, Self::H), Status>;
    fn stream_read(&mut self, end: Self::H, into: User) -> Result<usize, Status>;
    fn stream_write(&mut self, end: Self::H, from: User) -> Result<usize, Status>;

    /// Copies the program's memory out; a bad address ends the process (a
    /// fault), as a C library's own copy would.
    fn copy_in(&mut self, from: User, out: &mut [u8]);
    fn copy_out(&mut self, to: User, bytes: &[u8]);
    /// Copies between two places of this process's memory.
    fn copy(&mut self, to: User, from: User);

    fn memory_create(&mut self, size: u64) -> Result<Self::H, Status>;
    /// Maps the whole object writable, and gives its address.
    fn memory_map(&mut self, object: Self::H, length: u64) -> Result<u64, Status>;
    fn memory_unmap(&mut self, address: u64, length: u64) -> Result<(), Status>;

    /// Nanoseconds on the monotonic clock.
    fn now(&mut self) -> u64;
}
