//! The platform of a graphical client (ADR-0165), apart from `zoraya-app` so that a library
//! that only speaks to the compositor (`zoraya-surface`, in libposix) does not link the widgets.

#![no_std]

/// What the application runs on: the compositor's channel and memory
/// objects. The program's is a thin layer over the kernel; the tests' is a
/// fake compositor.
pub trait Platform {
    /// A handle to give with a message.
    type End: Copy;
    /// Sends one message, with a memory object's handle or none.
    fn send(&mut self, bytes: &[u8], handle: Option<Self::End>) -> bool;
    /// The next message from the compositor, waiting for it; `None` when
    /// the compositor is gone.
    fn receive(&mut self, bytes: &mut [u8]) -> Option<usize>;
    /// A memory object of `pixels` pixels, mapped: its handle to give and
    /// its number for [`Platform::pixels`].
    fn buffer(&mut self, pixels: usize) -> Option<(Self::End, usize)>;
    fn pixels(&mut self, buffer: usize) -> &mut [u32];
    fn free(&mut self, buffer: usize);
    /// The handle the message last received carried, taken (a drop's
    /// entry, ADR-0170); the platform closes one nobody took before it
    /// receives the next. By default messages carry none.
    fn handle(&mut self) -> Option<Self::End> {
        None
    }
    /// The next message if one is there, without waiting, for a program
    /// that waits on other channels too (the chooser). By default it waits.
    fn poll(&mut self, bytes: &mut [u8]) -> Poll {
        match self.receive(bytes) {
            Some(n) => Poll::Message(n),
            None => Poll::Gone,
        }
    }
}

/// What [`Platform::poll`] found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Poll {
    Message(usize),
    /// Nothing yet.
    Empty,
    /// The compositor is gone.
    Gone,
}
